import { Plugin } from "@opencode/plugin"
import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Completions } from "./completions"
import { send } from "./bridge"
import { consumeEvents } from "./events"
import { canAnnounceFromLocalServer } from "./state-client"

export default Plugin.define({
  id: "civilized-agent",
  async setup(ctx) {
    if (process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION === "1") {
      if (!process.env.CIVILIZED_AGENT_DATA) throw new Error("An external companion requires CIVILIZED_AGENT_DATA")
    } else {
      const suffix = process.platform === "win32" ? ".exe" : ""
      const binary = process.env.CIVILIZED_AGENT_BINARY ?? fileURLToPath(new URL(`../native-announcer/bin/civilized-announcer-${process.platform}-${process.arch}${suffix}`, import.meta.url))
      if (!existsSync(binary)) throw new Error(`Civilized Agent native announcer is missing at ${binary}. Run npm run build:announcer on this platform.`)
      const child = spawn(binary, ["--assets", fileURLToPath(new URL("../native-announcer/resources", import.meta.url))], { detached: true, stdio: "ignore", windowsHide: true })
      child.on("error", console.error)
      child.unref()
    }
    const completions = new Completions(
      async (sessionID, failed) => {
        const result = await ctx.session.generate({
          sessionID,
          prompt: `Summarize the most recently ${failed ? "failed" : "completed"} task in exactly one short spoken sentence of at most 30 words. Include the actual outcome and any important failure or remaining blocker. Focus on work actually performed and its results. Omit statements about actions not taken, such as not deploying or not reloading. Use plain English, no Markdown, no introduction, no file paths, no greetings, no catchphrases, and no theatrical language. Do not claim success unless confirmed. Do not run tools. Output only that sentence.`,
        })
        return result.text
      },
      async (completion) => {
        const session = await ctx.session.get({ sessionID: completion.sessionID })
        await send({ type: "notify", ...completion, presenceSessionID: completion.sessionID, character: "opencode", title: session.title })
      },
      canAnnounceFromLocalServer,
      typeof ctx.options.minimumSeconds === "number" ? ctx.options.minimumSeconds * 1000 : 60_000,
      (sessionID, reason) => console.info(JSON.stringify({ plugin: "civilized-agent", sessionID, reason })),
    )
    const controller = new AbortController()
    const owned = new Set<string>()
    const roots = new Map<string, string>()
    const tasks = new Set<Promise<void>>()
    let saved = Promise.resolve()
    const persist = () => {
      saved = saved.then(() => ctx.storage.set("completion-tasks", completions.snapshot())).catch(console.error)
      return saved
    }
    const rootFor = async (sessionID: string): Promise<string> => {
      const known = roots.get(sessionID)
      if (known) return known
      const session = await ctx.session.get({ sessionID })
      const root = session.parentID ? await rootFor(session.parentID) : sessionID
      roots.set(sessionID, root)
      return root
    }
    const owns = async (sessionID: string) => {
      if (owned.has(sessionID)) return true
      const session = await ctx.session.get({ sessionID })
      const rootID = await rootFor(sessionID)
      const root = rootID === sessionID ? session : await ctx.session.get({ sessionID: rootID })
      if (root.location.directory.toLowerCase() !== ctx.location.directory.toLowerCase()) return false
      owned.add(sessionID)
      owned.add(rootID)
      return true
    }
    completions.restore(await ctx.storage.get("completion-tasks"))
    const restored = completions.snapshot()
    const restoredSessions = new Set(restored.runs.map((run) => run.sessionID))
    for (const sessionID of restoredSessions) {
      try {
        if (await rootFor(sessionID) !== sessionID) completions.cancel(sessionID)
      } catch (error) {
        console.error(error)
      }
    }
    await persist()
    const subscription = consumeEvents(
      (signal) => ctx.event.subscribe({ signal }),
      async (event) => {
        if (event.type === "shell.created") {
          const info = event.data.info
          const sessionID = info.metadata.sessionID
          if (typeof sessionID !== "string" || !(await owns(sessionID))) return
          completions.invalidateSummary(await rootFor(sessionID))
          return
        }
        if (!["session.created", "session.inbox.enqueued", "session.execution.started", "session.execution.succeeded", "session.execution.failed", "session.execution.interrupted", "session.deleted"].includes(event.type)) return
        if (!("sessionID" in event.data)) return
        const sessionID = event.data.sessionID
        if (typeof sessionID !== "string") return
        if (event.type === "session.created") {
          if (typeof event.data.parentID !== "string" || !(await owns(event.data.parentID)) || !(await owns(sessionID))) return
          completions.invalidateSummary(await rootFor(sessionID))
          return
        }
        if (event.type === "session.deleted") {
          if (!owned.has(sessionID) && !roots.has(sessionID) && !restoredSessions.has(sessionID)) return
        } else if (!(await owns(sessionID))) return
        let rootID = roots.get(sessionID)
        if (!rootID) {
          try {
            rootID = await rootFor(sessionID)
          } catch (error) {
            if (event.type !== "session.deleted") throw error
            rootID = sessionID
          }
        }
        const isRoot = rootID === sessionID
        if (event.type === "session.inbox.enqueued" && event.data.item.type === "user") {
          if (isRoot) completions.start(rootID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.inbox.enqueued" && event.data.item.type === "synthetic") {
          completions.invalidateSummary(rootID)
        }
        if (event.type === "session.execution.started") {
          if (!isRoot) completions.invalidateSummary(rootID)
          if (isRoot) completions.resume(rootID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.deleted" || event.type === "session.execution.interrupted") {
          completions.invalidateSummary(rootID)
          if (isRoot && (event.type === "session.deleted" || event.data.reason !== "shutdown")) completions.cancel(rootID)
          await send({ type: "discard", sessionID, at: event.created })
          if (event.type === "session.deleted") {
            owned.delete(sessionID)
            roots.delete(sessionID)
          }
        }
        await persist()
        if (event.type !== "session.execution.succeeded" && event.type !== "session.execution.failed") return
        if (!isRoot) return
        const task = completions.finish(event.id, rootID, event.created, event.type === "session.execution.failed")
          .catch(console.error)
          .then(persist)
          .finally(() => tasks.delete(task))
        tasks.add(task)
      },
      controller.signal,
      console.error,
    )
    return async () => {
      controller.abort()
      owned.forEach((sessionID) => completions.invalidateSummary(sessionID))
      await subscription
      await Promise.allSettled(tasks)
      await persist()
    }
  },
})
