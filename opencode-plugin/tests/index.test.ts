import { afterAll, expect, mock, spyOn, test } from "bun:test"
import { copyFileSync, mkdtempSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

type Event = { type: string; id: string; created: number; data: Record<string, unknown> }
type Session = { id: string; parentID?: string; outcome?: string; location: { directory: string }; title: string }
const commands: Record<string, unknown>[] = []
const generated: string[] = []
const generatedPrompts: string[] = []
const spawned: string[][] = []
const binary = process.env.CIVILIZED_AGENT_BINARY
process.env.CIVILIZED_AGENT_BINARY = process.execPath
let stateClient: unknown
let discovered = true
let serverPID = process.pid
mock.module("@opencode/plugin", () => ({ Plugin: { define: (plugin: unknown) => plugin } }))
mock.module("node:child_process", () => ({ spawn: (_binary: string, args: string[]) => {
  spawned.push(args)
  return { on: () => {}, unref: () => {} }
} }))
mock.module("../bridge", () => ({ send: async (command: Record<string, unknown>) => { commands.push(command) } }))
mock.module("@opencode/client", () => ({ OpenCode: { make: () => stateClient } }))
mock.module("@opencode/client/service", () => ({ Service: {
  discover: async () => discovered ? { url: "http://localhost:12345" } : undefined,
  headers: () => ({ authorization: "test" }),
} }))
const { default: plugin } = await import("../index")
afterAll(() => {
  if (binary === undefined) delete process.env.CIVILIZED_AGENT_BINARY
  else process.env.CIVILIZED_AGENT_BINARY = binary
})

test("an installed OpenCode package ignores a checkout executable override", async () => {
  const temporary = fileURLToPath(new URL("../../temp/", import.meta.url))
  mkdirSync(temporary, { recursive: true })
  const directory = mkdtempSync(join(temporary, "installed-opencode-"))
  const previousExternal = process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION
  try {
    delete process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION
    mkdirSync(join(directory, "opencode-plugin"))
    mkdirSync(join(directory, "claude-plugin", "scripts"), { recursive: true })
    const source = fileURLToPath(new URL("..", import.meta.url))
    for (const name of readdirSync(source).filter(name => name.endsWith(".ts"))) copyFileSync(join(source, name), join(directory, "opencode-plugin", name))
    copyFileSync(fileURLToPath(new URL("../../claude-plugin/scripts/summary-prompt.mjs", import.meta.url)), join(directory, "claude-plugin", "scripts", "summary-prompt.mjs"))
    writeFileSync(join(directory, "bundle-manifest.json"), "{}")
    const { default: installed } = await import(pathToFileURL(join(directory, "opencode-plugin/index.ts")).href)
    await expect(installed.setup({})).rejects.toThrow("Reinstall the application bundle")
  } finally {
    if (previousExternal === undefined) delete process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION
    else process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION = previousExternal
    rmSync(directory, { recursive: true, force: true })
  }
})

async function fixture(stored?: unknown, minimumSeconds = 0) {
  commands.length = 0
  generated.length = 0
  generatedPrompts.length = 0
  spawned.length = 0
  discovered = true
  serverPID = process.pid
  const sessions = new Map<string, Session>([["root", { id: "root", title: "Root task", location: { directory: "workspace" } }]])
  const active = new Set<string>()
  const shells = new Map<string, { id: string; status: string; directory: string; metadata: { sessionID: string } }>()
  const inbox = new Map<string, unknown[]>()
  let finalReply = true
  let pageSize = 100
  let snapshot: unknown
  const queue: { event: Event; resolve: () => void }[] = []
  let take: ((item: typeof queue[number] | undefined) => void) | undefined
  const client = {
    server: { info: async () => ({ pid: serverPID }) },
    session: {
      get: async ({ sessionID }: { sessionID: string }) => {
        const session = sessions.get(sessionID)
        if (!session) throw new Error("Session not found")
        return session
      },
      list: async ({ parentID, cursor }: { parentID: string; cursor?: string }) => {
        const children = [...sessions.values()].filter((session) => session.parentID === parentID)
        const offset = Number(cursor ?? 0)
        return { data: children.slice(offset, offset + pageSize), cursor: { next: offset + pageSize < children.length ? String(offset + pageSize) : undefined } }
      },
      active: async () => Object.fromEntries([...active].map((id) => [id, { type: "running" }])),
      inbox: { list: async ({ sessionID }: { sessionID: string }) => inbox.get(sessionID) ?? [] },
      context: async () => finalReply ? [{ type: "assistant", finish: "stop", content: [{ type: "text", text: "Done." }] }] : [],
      generate: async ({ sessionID, prompt }: { sessionID: string; prompt: string }) => { generated.push(sessionID); generatedPrompts.push(prompt); return { text: "Root completed." } },
    },
    shell: { list: async ({ location }: { location: { directory: string } }) => ({ data: [...shells.values()].filter((shell) => shell.directory === location.directory) }) },
  }
  stateClient = client
  const cleanup = await (plugin as unknown as { setup: (context: unknown) => Promise<() => Promise<void>> }).setup({
    location: { directory: "workspace" }, options: { minimumSeconds }, session: client.session,
    storage: { get: async () => stored, set: async (_: string, value: unknown) => { snapshot = value } },
    event: { subscribe: ({ signal }: { signal: AbortSignal }) => (async function* () {
      while (!signal.aborted) {
        const item = queue.shift() ?? await new Promise<typeof queue[number] | undefined>((resolve) => {
          take = resolve
          signal.addEventListener("abort", () => resolve(undefined), { once: true })
        })
        if (!item || signal.aborted) return
        yield item.event
        item.resolve()
      }
    })() },
  })
  const emit = async (type: string, sessionID: string, created: number, extra: Record<string, unknown> = {}) => {
    if (type === "session.execution.started") active.add(sessionID)
    if (["session.execution.succeeded", "session.execution.failed", "session.execution.interrupted"].includes(type)) {
      active.delete(sessionID)
      const session = sessions.get(sessionID)
      if (session) session.outcome = type.split(".").at(-1)
    }
    await new Promise<void>((resolve) => {
      const item = { event: { type, id: `${sessionID}-${created}`, created, data: { sessionID, ...extra } }, resolve }
      if (take) { const deliver = take; take = undefined; deliver(item) } else queue.push(item)
    })
    await Bun.sleep(0)
  }
  const child = (id: string, parentID = "root", directory = "other-workspace") => {
    sessions.set(id, { id, parentID, title: id, location: { directory } })
    active.add(id)
  }
  const start = () => emit("session.inbox.enqueued", "root", 0, { item: { type: "user" } })
  const finish = (at = 70_000) => emit("session.execution.succeeded", "root", at)
  const fail = (at = 70_000) => emit("session.execution.failed", "root", at)
  const notices = () => commands.filter((command) => command.type === "notify")
  return { sessions, session: client.session, active, shells, inbox, emit, child, start, finish, fail, notices, cleanup, snapshot: () => snapshot, noFinalReply: () => { finalReply = false }, paginate: () => { pageSize = 1 } }
}

test("cancels generation during readiness checking and allows the next summary", async () => {
  const f = await fixture()
  const context = f.session.context
  let reads = 0
  let release!: () => void
  let loaded!: () => void
  const waiting = new Promise<void>(resolve => { release = resolve })
  const loading = new Promise<void>(resolve => { loaded = resolve })
  const reader = spyOn(f.session, "context").mockImplementation(async () => {
    if (++reads === 1) {
      loaded()
      await waiting
    }
    return context()
  })
  try {
    await f.start()
    await f.finish()
    await loading
    await f.emit("session.inbox.enqueued", "root", 80_000, { item: { type: "user" } })
    release()
    await Bun.sleep(0)
    expect(generated).toEqual([])
    await f.finish(150_000)
    expect(generated).toEqual(["root"])
    expect(f.notices()).toMatchObject([{ type: "notify", sessionID: "root", text: "Root completed." }])
  } finally {
    release()
    reader.mockRestore()
    await f.cleanup()
  }
})

test("sends the saved prompt unchanged and rereads it", async () => {
  const temporary = fileURLToPath(new URL("../../temp/", import.meta.url))
  mkdirSync(temporary, { recursive: true })
  const data = mkdtempSync(join(temporary, "summary-prompt-"))
  const previousData = process.env.CIVILIZED_AGENT_DATA
  const firstPrompt = "Report the task outcome.\nUnicode: Ω 😀\nBe explicit and concise."
  const secondPrompt = "Describe the result in one sentence."
  try {
    process.env.CIVILIZED_AGENT_DATA = data
    writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: "Global fallback.", characters: { herald: { selected: true, summaryPrompt: firstPrompt }, robot: { summaryPrompt: secondPrompt } } }))
    const f = await fixture()
    try {
      await f.start()
      await f.finish()
      expect(generatedPrompts).toEqual([firstPrompt])
      writeFileSync(join(data, "settings.json"), JSON.stringify({ summaryPrompt: "Global fallback.", characters: { herald: { summaryPrompt: firstPrompt }, robot: { selected: true, summaryPrompt: secondPrompt } } }))
      await f.start()
      await f.fail(80_000)
      expect(generatedPrompts).toEqual([
        firstPrompt,
        secondPrompt,
      ])
      expect(generated).toEqual(["root", "root"])
      expect(commands.filter(command => command.type === "notify").map(command => command.characterID)).toEqual(["herald", "robot"])
    } finally { await f.cleanup() }
  } finally {
    if (previousData === undefined) delete process.env.CIVILIZED_AGENT_DATA
    else process.env.CIVILIZED_AGENT_DATA = previousData
    rmSync(data, { recursive: true, force: true })
  }
})

test("queries nested cross-location work even when its start events were missed", async () => {
  const f = await fixture()
  try {
    await f.start()
    f.child("child")
    f.child("nested", "child", "third-workspace")
    f.shells.set("shell", { id: "shell", status: "running", directory: "third-workspace", metadata: { sessionID: "nested" } })
    await f.finish()
    expect(generated).toEqual([])
    await f.emit("session.execution.succeeded", "nested", 80_000)
    await f.emit("session.execution.succeeded", "child", 90_000)
    await f.finish(100_000)
    expect(f.notices()).toEqual([])
    f.shells.delete("shell")
    await f.emit("shell.exited", "nested", 110_000, { id: "shell" })
    expect(f.notices()).toEqual([])
    await f.finish(120_000)
    expect(generated).toEqual(["root"])
    expect(f.notices()[0]?.completed).toBe(120_000)
    expect(f.notices()[0]?.presenceSessionID).toBe("root")
  } finally { await f.cleanup() }
})

test("main interruption does not hide surviving work from a new task", async () => {
  const f = await fixture()
  try {
    await f.start()
    f.child("child")
    await f.emit("session.execution.interrupted", "root", 20_000, { reason: "user" })
    await f.emit("session.inbox.enqueued", "root", 30_000, { item: { type: "user" } })
    await f.finish(100_000)
    expect(f.notices()).toEqual([])
    f.active.delete("child")
    await f.finish(110_000)
    expect(f.notices()).toHaveLength(1)
  } finally { await f.cleanup() }
})

test("late cancellation notices cannot release a restarted child or its shell", async () => {
  const f = await fixture()
  try {
    await f.start()
    f.child("child")
    await f.emit("session.execution.interrupted", "child", 20_000, { reason: "user" })
    await f.emit("session.execution.started", "child", 30_000)
    await f.emit("session.inbox.enqueued", "root", 40_000, { item: { type: "synthetic", payload: { metadata: { source: "subagent", childID: "child", state: "cancelled" } } } })
    await f.finish()
    expect(f.notices()).toEqual([])
    f.shells.set("shell", { id: "shell", status: "running", directory: "other-workspace", metadata: { sessionID: "child" } })
    await f.emit("session.execution.interrupted", "child", 80_000, { reason: "user" })
    await f.finish(90_000)
    expect(f.notices()).toEqual([])
  } finally { await f.cleanup() }
})

test("shutdown and old persisted jobs do not reset the task timer or block recovered work", async () => {
  const f = await fixture({ runs: [{ sessionID: "root", started: 0 }], jobs: [{ id: "stale", sessionID: "root" }] }, 60)
  try {
    await f.emit("session.execution.interrupted", "root", 65_000, { reason: "shutdown" })
    await f.emit("session.execution.started", "root", 70_000)
    await f.finish(80_000)
    expect(f.notices()).toHaveLength(1)
  } finally { await f.cleanup() }
})

test("queued input and missing final replies block a main completion", async () => {
  const f = await fixture()
  try {
    await f.start()
    f.inbox.set("root", [{}])
    await f.finish()
    expect(f.notices()).toEqual([])
    f.inbox.clear()
    f.noFinalReply()
    await f.finish(80_000)
    expect(f.notices()).toEqual([])
  } finally { await f.cleanup() }
})

test("unrelated sessions and shells do not block the task", async () => {
  const f = await fixture()
  try {
    await f.start()
    f.active.add("unrelated")
    f.shells.set("other", { id: "other", status: "running", directory: "workspace", metadata: { sessionID: "unrelated" } })
    await f.finish()
    expect(f.notices()).toHaveLength(1)
  } finally { await f.cleanup() }
})

test("checks all pages of child sessions", async () => {
  const f = await fixture()
  try {
    await f.start()
    f.paginate()
    f.child("first")
    f.active.delete("first")
    f.child("second")
    await f.finish()
    expect(f.notices()).toEqual([])
    f.active.delete("second")
    await f.finish(80_000)
    expect(f.notices()).toHaveLength(1)
  } finally { await f.cleanup() }
})

test("missing discovery or a different server fails closed and retains the task", async () => {
  for (const mismatch of [false, true]) {
    const f = await fixture()
    const errors = spyOn(console, "error").mockImplementation(() => {})
    try {
      await f.start()
      discovered = mismatch
      serverPID = process.pid + 1
      await f.finish()
      expect(generated).toEqual([])
      expect(f.notices()).toEqual([])
      expect(f.snapshot()).toEqual({ runs: [{ sessionID: "root", started: 0 }] })
      expect(errors).toHaveBeenCalledTimes(1)
      expect(String(errors.mock.calls[0]?.[0])).toContain(mismatch
        ? "Announcement state belongs to another OpenCode server"
        : "Cannot discover the announcement server without starting a service")
      discovered = true
      serverPID = process.pid
      await f.finish(80_000)
      expect(f.notices()).toHaveLength(1)
    } finally { await f.cleanup(); errors.mockRestore() }
  }
})

test("an externally owned companion requires separate data and skips automatic launch", async () => {
  const oldExternal = process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION
  const oldData = process.env.CIVILIZED_AGENT_DATA
  const oldBinary = process.env.CIVILIZED_AGENT_BINARY
  try {
    process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION = "1"
    process.env.CIVILIZED_AGENT_BINARY = "not-an-executable"
    delete process.env.CIVILIZED_AGENT_DATA
    await expect(fixture()).rejects.toThrow("An external companion requires CIVILIZED_AGENT_DATA")
    process.env.CIVILIZED_AGENT_DATA = "isolated-proof"
    const f = await fixture()
    try {
      await f.start()
      await f.finish()
      expect(spawned).toEqual([])
      expect(f.notices()).toHaveLength(1)
    } finally { await f.cleanup() }
  } finally {
    if (oldExternal === undefined) delete process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION
    else process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION = oldExternal
    if (oldData === undefined) delete process.env.CIVILIZED_AGENT_DATA
    else process.env.CIVILIZED_AGENT_DATA = oldData
    if (oldBinary === undefined) delete process.env.CIVILIZED_AGENT_BINARY
    else process.env.CIVILIZED_AGENT_BINARY = oldBinary
  }
})
