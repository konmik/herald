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
    const binary = process.env.CIVILIZED_AGENT_BINARY ?? fileURLToPath(new URL(`../claude/bin/civilized-announcer-${process.platform}-${process.arch}${suffix}`, import.meta.url))
    if (!existsSync(binary)) throw new Error(`Civilized Agent native announcer is missing at ${binary}. Run npm run build:announcer on this platform.`)
    const assets = fileURLToPath(new URL("../claude/assets", import.meta.url))
    const child = spawn(binary, ["--assets", assets], { detached: true, stdio: "ignore", windowsHide: true })
    child.on("error", console.error)
    child.unref()
    const completions = new Completions(
      async (sessionID, failed) => {
        const result = await ctx.session.generate({
          sessionID,
          prompt: `Summarize the most recently ${failed ? "failed" : "completed"} task in exactly one short spoken sentence of at most 30 words. Include the actual outcome and any important failure or remaining blocker. Use plain English, no Markdown, no introduction, no file paths, and do not claim success unless confirmed. Do not run tools. Output only that sentence.`,
        })
        return result.text
      },
      async (completion) => {
        const session = await ctx.session.get({ sessionID: completion.sessionID })
        await send({ type: "notify", ...completion, character: "opencode", title: session.title })
      },
      typeof ctx.options.minimumSeconds === "number" ? ctx.options.minimumSeconds * 1000 : 60_000,
    )
    const controller = new AbortController()
    const owned = new Set<string>()
    const tasks = new Set<Promise<void>>()
    void (async () => {
      for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
        if (!["session.execution.started", "session.execution.succeeded", "session.execution.failed", "session.execution.interrupted", "session.viewed", "session.deleted"].includes(event.type)) continue
        if (!("sessionID" in event.data)) continue
        const sessionID = event.data.sessionID
        if (typeof sessionID !== "string") continue
        if (!owned.has(sessionID)) {
          if (event.type === "session.deleted") continue
          const session = await ctx.session.get({ sessionID })
          if (session.location.directory.toLowerCase() !== ctx.location.directory.toLowerCase()) continue
          owned.add(sessionID)
        }
        if (event.type === "session.execution.started") {
          completions.start(sessionID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.viewed") {
          completions.view(sessionID)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.deleted" || event.type === "session.execution.interrupted") {
          completions.cancel(sessionID)
          await send({ type: "discard", sessionID, at: event.created })
          if (event.type === "session.deleted") owned.delete(sessionID)
        }
        if (event.type !== "session.execution.succeeded" && event.type !== "session.execution.failed") continue
        const task = completions.finish(event.id, sessionID, event.created, event.type === "session.execution.failed")
          .catch(console.error)
          .finally(() => tasks.delete(task))
        tasks.add(task)
      }
    })().catch((error) => {
      if (!controller.signal.aborted) console.error(error)
    })
    return async () => {
      controller.abort()
      owned.forEach((sessionID) => completions.cancel(sessionID))
      await Promise.allSettled(tasks)
    }
  },
})
