import { afterAll, expect, mock, test } from "bun:test"

type Event = { type: string; id: string; created: number; data: Record<string, unknown> }
type Session = { id: string; parentID?: string; outcome?: string; location: { directory: string }; title: string }
const commands: Record<string, unknown>[] = []
const generated: string[] = []
const binary = process.env.CIVILIZED_AGENT_BINARY
process.env.CIVILIZED_AGENT_BINARY = process.execPath
let stateClient: unknown
let discovered = true
let serverPID = process.pid
mock.module("@opencode/plugin", () => ({ Plugin: { define: (plugin: unknown) => plugin } }))
mock.module("node:child_process", () => ({ spawn: () => ({ on: () => {}, unref: () => {} }) }))
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

async function fixture(stored?: unknown, minimumSeconds = 0) {
  commands.length = 0
  generated.length = 0
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
      generate: async ({ sessionID }: { sessionID: string }) => { generated.push(sessionID); return { text: "Root completed." } },
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
  const notices = () => commands.filter((command) => command.type === "notify")
  return { sessions, active, shells, inbox, emit, child, start, finish, notices, cleanup, snapshot: () => snapshot, noFinalReply: () => { finalReply = false }, paginate: () => { pageSize = 1 } }
}

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
    try {
      await f.start()
      discovered = mismatch
      serverPID = process.pid + 1
      await f.finish()
      expect(generated).toEqual([])
      expect(f.notices()).toEqual([])
      expect(f.snapshot()).toEqual({ runs: [{ sessionID: "root", started: 0 }] })
      discovered = true
      serverPID = process.pid
      await f.finish(80_000)
      expect(f.notices()).toHaveLength(1)
    } finally { await f.cleanup() }
  }
})
