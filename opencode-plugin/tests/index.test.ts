import { afterAll, expect, mock, test } from "bun:test"

type Command = Record<string, unknown>
type Event = { type: string; id: string; created: number; data: Record<string, unknown> }
type Session = { id: string; parentID?: string; location: { directory: string }; title: string }
type QueueItem = { event: Event; resolve: () => void }

const commands: Command[] = []
const generated: { sessionID: string; prompt: string }[] = []
const binary = process.env.CIVILIZED_AGENT_BINARY
process.env.CIVILIZED_AGENT_BINARY = process.execPath

mock.module("@opencode/plugin", () => ({ Plugin: { define: (plugin: unknown) => plugin } }))
mock.module("node:child_process", () => ({
  spawn: () => ({ on: () => {}, unref: () => {} }),
}))
mock.module("../bridge", () => ({
  send: async (command: Command) => { commands.push(command) },
}))

const { default: plugin } = await import("../index")

afterAll(() => {
  if (binary === undefined) delete process.env.CIVILIZED_AGENT_BINARY
  else process.env.CIVILIZED_AGENT_BINARY = binary
})

function event(id: string, type: string, created: number, data: Record<string, unknown>): Event {
  return { id, type, created, data }
}

function sessionCreated(id: string, created: number, parentID?: string) {
  return event(`${id}-created`, "session.created", created, {
    sessionID: id,
    ...(parentID ? { parentID } : {}),
    location: { directory: "workspace" },
  })
}

function userMessage(id: string, created: number) {
  return event(`${id}-user`, "session.inbox.enqueued", created, {
    sessionID: id,
    inboxID: `${id}-inbox`,
    item: { type: "user", payload: { text: "continue" }, delivery: "queue" },
  })
}

function syntheticNotice(sessionID: string, childID: string, created: number) {
  return event(`${sessionID}-${childID}-notice`, "session.inbox.enqueued", created, {
    sessionID,
    inboxID: `${sessionID}-${childID}-notice-inbox`,
    item: {
      type: "synthetic",
      payload: { metadata: { source: "subagent", childID, state: "completed" } },
      delivery: "queue",
    },
  })
}

function executionStarted(id: string, created: number) {
  return event(`${id}-started`, "session.execution.started", created, { sessionID: id })
}

function executionSucceeded(id: string, created: number, eventID = `${id}-succeeded`) {
  return event(eventID, "session.execution.succeeded", created, { sessionID: id })
}

function shellCreated(id: string, sessionID: string, created: number) {
  return event(`${id}-created`, "shell.created", created, {
    info: { id, time: { started: created }, metadata: { sessionID } },
  })
}

function shellExited(id: string, created: number) {
  return event(`${id}-exited`, "shell.exited", created, { id })
}

function eventStream() {
  const queue: QueueItem[] = []
  let take: ((item: QueueItem | undefined) => void) | undefined

  const subscribe = (signal: AbortSignal) => (async function* () {
    while (!signal.aborted) {
      const item = queue.shift() ?? await new Promise<QueueItem | undefined>((resolve) => {
        take = resolve
        signal.addEventListener("abort", () => {
          if (take === resolve) {
            take = undefined
            resolve(undefined)
          }
        }, { once: true })
      })
      if (!item || signal.aborted) return
      yield item.event
      item.resolve()
    }
  })()

  const emit = (event: Event) => new Promise<void>((resolve) => {
    const item = { event, resolve }
    if (take) {
      const deliver = take
      take = undefined
      deliver(item)
    } else {
      queue.push(item)
    }
  })

  return { subscribe, emit }
}

function notificationCommands() {
  return commands.filter((command) => command.type === "notify")
}

test("announces only the completed root after child sessions and jobs finish", async () => {
  commands.length = 0
  generated.length = 0

  const sessions = new Map<string, Session>([
    ["root", { id: "root", location: { directory: "workspace" }, title: "Root task" }],
    ["child-one", { id: "child-one", parentID: "root", location: { directory: "workspace" }, title: "Child one" }],
    ["child-two", { id: "child-two", parentID: "root", location: { directory: "workspace" }, title: "Child two" }],
    ["nested", { id: "nested", parentID: "child-two", location: { directory: "workspace" }, title: "Nested child" }],
  ])
  const stream = eventStream()
  const cleanup = await (plugin as unknown as { setup: (context: unknown) => Promise<() => Promise<void>> }).setup({
    location: { directory: "workspace" },
    options: { minimumSeconds: 0 },
    event: { subscribe: ({ signal }: { signal: AbortSignal }) => stream.subscribe(signal) },
    session: {
      get: async ({ sessionID }: { sessionID: string }) => sessions.get(sessionID),
      context: async () => [],
      generate: async ({ sessionID, prompt }: { sessionID: string; prompt: string }) => {
        generated.push({ sessionID, prompt })
        return { text: sessionID === "root" ? "Root completed." : `${sessionID} completed.` }
      },
    },
    storage: {
      get: async () => undefined,
      set: async () => {},
    },
  })

  try {
    await stream.emit(userMessage("root", 0))
    await stream.emit(executionStarted("root", 1))
    await stream.emit(sessionCreated("child-one", 10, "root"))
    await stream.emit(userMessage("child-one", 11))
    await stream.emit(executionStarted("child-one", 12))
    await stream.emit(executionSucceeded("child-one", 60_000))
    await Bun.sleep(0)
    expect(generated).toEqual([])
    expect(notificationCommands()).toEqual([])

    await stream.emit(syntheticNotice("root", "child-one", 60_001))
    await stream.emit(sessionCreated("child-two", 70_000, "root"))
    await stream.emit(userMessage("child-two", 70_001))
    await stream.emit(executionStarted("child-two", 70_002))
    await stream.emit(shellCreated("child-shell", "child-two", 70_003))
    await stream.emit(sessionCreated("nested", 70_004, "child-two"))
    await stream.emit(userMessage("nested", 70_005))
    await stream.emit(executionStarted("nested", 70_006))
    await stream.emit(executionSucceeded("nested", 80_000))
    await stream.emit(syntheticNotice("child-two", "nested", 80_001))
    await stream.emit(executionSucceeded("child-two", 90_000))
    await stream.emit(syntheticNotice("root", "child-two", 90_001))
    await stream.emit(executionSucceeded("root", 100_000, "root-early"))
    await Bun.sleep(0)
    expect(generated).toEqual([])
    expect(notificationCommands()).toEqual([])

    await stream.emit(shellExited("child-shell", 110_000))
    await stream.emit(syntheticNotice("root", "child-two", 110_001))
    await stream.emit(executionSucceeded("root", 120_000, "root-completed"))
    await Bun.sleep(0)
  } finally {
    await cleanup()
  }

  expect(generated.map((item) => item.sessionID)).toEqual(["root"])
  expect(notificationCommands()).toEqual([{
    type: "notify",
    id: "root-completed",
    sessionID: "root",
    completed: 120_000,
    text: "Root completed.",
    emotion: "neutral",
    presenceSessionID: "root",
    character: "opencode",
    title: "Root task",
  }])
})
