import { Plugin } from "@opencode/plugin"
import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Completions } from "./completions"
import { send } from "./bridge"

export default Plugin.define({
  id: "civilized-agent",
  async setup(ctx) {
    const suffix = process.platform === "win32" ? ".exe" : ""
    const binary = process.env.CIVILIZED_AGENT_BINARY ?? fileURLToPath(new URL(`../native-announcer/bin/civilized-announcer-${process.platform}-${process.arch}${suffix}`, import.meta.url))
    if (!existsSync(binary)) throw new Error(`Civilized Agent native announcer is missing at ${binary}. Run npm run build:announcer on this platform.`)
    const assets = fileURLToPath(new URL("../native-announcer/resources", import.meta.url))
    const child = spawn(binary, ["--assets", assets], { detached: true, stdio: "ignore", windowsHide: true })
    child.on("error", console.error)
    child.unref()
    const completions = new Completions(
      async (sessionID, failed) => {
        const result = await ctx.session.generate({
          sessionID,
          prompt: `Summarize the most recently ${failed ? "failed" : "completed"} task in exactly one short spoken sentence of at most 30 words. Include the actual outcome and any important failure or remaining blocker. Use plain English, no Markdown, no introduction, no file paths, no greetings, no catchphrases, and no theatrical language. Do not claim success unless confirmed. Do not run tools. Output only that sentence.`,
        })
        return result.text
      },
      async (completion) => {
        const session = await ctx.session.get({ sessionID: completion.sessionID })
        await send({ type: "notify", ...completion, character: "opencode", title: session.title })
      },
      typeof ctx.options.minimumSeconds === "number" ? ctx.options.minimumSeconds * 1000 : 60_000,
      (sessionID, reason) => console.info(JSON.stringify({ plugin: "civilized-agent", sessionID, reason })),
    )
    completions.restore(await ctx.storage.get("completion-tasks"))
    const controller = new AbortController()
    const owned = new Set<string>()
    const tasks = new Set<Promise<void>>()
    let saved = Promise.resolve()
    const persist = () => {
      saved = saved.then(() => ctx.storage.set("completion-tasks", completions.snapshot())).catch(console.error)
      return saved
    }
    const owns = async (sessionID: string) => {
      if (owned.has(sessionID)) return true
      const session = await ctx.session.get({ sessionID })
      if (session.location.directory.toLowerCase() !== ctx.location.directory.toLowerCase()) return false
      owned.add(sessionID)
      return true
    }
    void (async () => {
      for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
        if (event.type === "shell.created") {
          const info = event.data.info
          const sessionID = info.metadata.sessionID
          if (typeof sessionID !== "string" || !(await owns(sessionID))) continue
          completions.jobStarted(info.id, sessionID, info.time.started)
          await persist()
          continue
        }
        if (event.type === "shell.exited" || event.type === "shell.deleted") {
          completions.jobFinished(event.data.id)
          await persist()
          continue
        }
        if (!["session.created", "session.inbox.enqueued", "session.execution.started", "session.execution.succeeded", "session.execution.failed", "session.execution.interrupted", "session.deleted"].includes(event.type)) continue
        if (!("sessionID" in event.data)) continue
        const sessionID = event.data.sessionID
        if (typeof sessionID !== "string") continue
        if (event.type === "session.created" && event.data.parentID && await owns(event.data.parentID)) {
          completions.jobStarted(sessionID, event.data.parentID, event.created)
        }
        if (event.type === "session.deleted" && !owned.has(sessionID)) continue
        if (!(await owns(sessionID))) continue
        if (event.type === "session.inbox.enqueued" && event.data.item.type === "user") {
          completions.start(sessionID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.inbox.enqueued" && event.data.item.type === "synthetic") {
          completions.notice(sessionID, event.data.item.payload.metadata)
        }
        if (event.type === "session.execution.started") {
          if (completions.hasJobs(sessionID)) {
            const messages = await ctx.session.context({ sessionID })
            for (const message of messages) {
              if (message.type === "synthetic") completions.notice(sessionID, message.metadata)
            }
          }
          completions.resume(sessionID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.deleted" || event.type === "session.execution.interrupted") {
          completions.jobFinished(sessionID)
          completions.cancel(sessionID)
          await send({ type: "discard", sessionID, at: event.created })
          if (event.type === "session.deleted") owned.delete(sessionID)
        }
        if (event.type === "session.execution.succeeded" || event.type === "session.execution.failed") {
          if (!completions.hasJobs(sessionID)) completions.jobFinished(sessionID)
        }
        await persist()
        if (event.type !== "session.execution.succeeded" && event.type !== "session.execution.failed") continue
        const task = completions.finish(event.id, sessionID, event.created, event.type === "session.execution.failed")
          .catch(console.error)
          .then(persist)
          .finally(() => tasks.delete(task))
        tasks.add(task)
      }
    })().catch((error) => {
      if (!controller.signal.aborted) console.error(error)
    })
    return async () => {
      controller.abort()
      owned.forEach((sessionID) => completions.view(sessionID))
      await Promise.allSettled(tasks)
      await persist()
    }
  },
})
