import { Plugin } from "@opencode/plugin"
import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { Completions } from "./completions"
import { send } from "./bridge"
import { consumeEvents } from "./events"

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
          prompt: `Summarize the most recently ${failed ? "failed" : "completed"} task in exactly one short spoken sentence of at most 30 words. Include the actual outcome and any important failure or remaining blocker. Focus on work actually performed and its results. Omit statements about actions not taken, such as not deploying or not reloading. Use plain English, no Markdown, no introduction, no file paths, no greetings, no catchphrases, and no theatrical language. Do not claim success unless confirmed. Do not run tools. Output only that sentence.`,
        })
        return result.text
      },
      async (completion) => {
        const session = await ctx.session.get({ sessionID: completion.sessionID })
        let root = session
        const visited = new Set([root.id])
        while (root.parentID && !visited.has(root.parentID)) {
          visited.add(root.parentID)
          root = await ctx.session.get({ sessionID: root.parentID })
        }
        await send({ type: "notify", ...completion, presenceSessionID: root.id, character: "opencode", title: session.title })
      },
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
    const rootFor = async (sessionID: string) => {
      const known = roots.get(sessionID)
      if (known) return known
      const path: string[] = []
      const visited = new Set<string>()
      let current = sessionID
      let root: string | undefined
      while (!visited.has(current)) {
        const cached = roots.get(current)
        if (cached) {
          root = cached
          break
        }
        visited.add(current)
        path.push(current)
        const session = await ctx.session.get({ sessionID: current })
        if (!session?.parentID || visited.has(session.parentID)) {
          root = current
          break
        }
        current = session.parentID
      }
      root ??= current
      roots.set(root, root)
      for (const id of path) {
        roots.set(id, root)
        completions.reparent(id, root)
      }
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
    const restoredSessions = new Set([
      ...restored.runs.map((run) => run.sessionID),
      ...restored.jobs.map((job) => job.sessionID),
    ])
    for (const sessionID of restoredSessions) {
      try {
        await rootFor(sessionID)
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
          completions.jobStarted(info.id, await rootFor(sessionID), info.time.started)
          await persist()
          return
        }
        if (event.type === "shell.exited" || event.type === "shell.deleted") {
          completions.jobFinished(event.data.id)
          await persist()
          return
        }
        if (!["session.created", "session.inbox.enqueued", "session.execution.started", "session.execution.succeeded", "session.execution.failed", "session.execution.interrupted", "session.deleted"].includes(event.type)) return
        if (!("sessionID" in event.data)) return
        const sessionID = event.data.sessionID
        if (typeof sessionID !== "string") return
        if (event.type === "session.created") {
          if (typeof event.data.parentID !== "string" || !(await owns(event.data.parentID)) || !(await owns(sessionID))) return
          completions.jobStarted(sessionID, await rootFor(sessionID), event.created)
          await persist()
          return
        }
        if (event.type === "session.deleted") {
          if (!owned.has(sessionID) && !roots.has(sessionID) && !completions.tracks(sessionID)) return
        } else if (!(await owns(sessionID))) return
        let rootID = roots.get(sessionID)
        if (!rootID) {
          try {
            rootID = await rootFor(sessionID)
          } catch (error) {
            if (event.type !== "session.deleted") throw error
            rootID = completions.sessionForJob(sessionID) ?? sessionID
          }
        }
        const isRoot = rootID === sessionID
        if (event.type === "session.inbox.enqueued" && event.data.item.type === "user") {
          if (isRoot) completions.start(rootID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.inbox.enqueued" && event.data.item.type === "synthetic") {
          completions.notice(rootID, event.data.item.payload.metadata, event.created)
        }
        if (event.type === "session.execution.started") {
          if (!isRoot) completions.jobStarted(sessionID, rootID, event.created)
          if (completions.hasJobs(rootID)) {
            const messages = await ctx.session.context({ sessionID })
            for (const message of messages) {
              if (message.type === "synthetic") completions.notice(rootID, message.metadata, message.time?.created)
            }
          }
          if (isRoot) completions.resume(rootID, event.created)
          await send({ type: "discard", sessionID, at: event.created })
        }
        if (event.type === "session.deleted" || event.type === "session.execution.interrupted") {
          completions.jobFinished(sessionID)
          completions.cancel(isRoot ? rootID : sessionID)
          await send({ type: "discard", sessionID, at: event.created })
          if (event.type === "session.deleted") {
            owned.delete(sessionID)
            roots.delete(sessionID)
          }
        }
        if (event.type === "session.execution.succeeded" || event.type === "session.execution.failed") {
          completions.jobFinished(sessionID)
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
      owned.forEach((sessionID) => completions.view(sessionID))
      await subscription
      await Promise.allSettled(tasks)
      await persist()
    }
  },
})
