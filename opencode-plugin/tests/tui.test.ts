import { expect, mock, spyOn, test } from "bun:test"

type Presence = { type: "presence"; clientID: string; sessionIDs: string[]; sequence: number; at: number }
const commands: Presence[] = []

mock.module("../bridge", () => ({ send: async (command: Presence) => { commands.push(command) } }))

const { default: exported } = await import("../tui")
const plugin = exported as unknown as { setup: (context: unknown) => () => Promise<void> }

test("keeps background tabs present, reports periodically, and clears presence when the window closes", async () => {
  commands.length = 0
  const timer = spyOn(globalThis, "setInterval")
  let cleanup: (() => Promise<void>) | undefined
  let tabs = [{ sessionID: "foreground" }, { sessionID: "background" }]
  try {
    cleanup = plugin.setup({
      ui: {
        router: { current: () => ({ type: "session", sessionID: "foreground" }) },
        tabs: { enabled: () => true, list: () => tabs },
      },
    })
    expect(timer).toHaveBeenCalledWith(expect.any(Function), 2000)
    const tick = timer.mock.calls[0]?.[0] as () => void
    const clientID = commands[0]?.clientID
    expect(commands).toEqual([
      { type: "presence", clientID, sessionIDs: ["foreground", "background"], sequence: 1, at: expect.any(Number) },
    ])
    tabs = [{ sessionID: "background" }]
    tick()
    expect(commands).toEqual([
      { type: "presence", clientID, sessionIDs: ["foreground", "background"], sequence: 1, at: expect.any(Number) },
      { type: "presence", clientID, sessionIDs: ["background"], sequence: 2, at: expect.any(Number) },
    ])
  } finally {
    timer.mockRestore()
    if (cleanup) await cleanup()
  }
  expect(commands.at(-1)).toEqual({
    type: "presence",
    clientID: commands[0]?.clientID,
    sessionIDs: [],
    sequence: 3,
    at: expect.any(Number),
  })
})

test("a child route keeps its owning session present without requiring focus", async () => {
  commands.length = 0
  const timer = spyOn(globalThis, "setInterval")
  let cleanup: (() => Promise<void>) | undefined
  try {
    cleanup = plugin.setup({
      ui: {
        router: { current: () => ({ type: "session", sessionID: "child" }) },
        tabs: { enabled: () => false, list: () => [] },
      },
      data: { session: { root: () => "parent" } },
    })
    const clientID = commands[0]?.clientID
    expect(commands).toEqual([
      { type: "presence", clientID, sessionIDs: ["parent"], sequence: 1, at: expect.any(Number) },
    ])
  } finally {
    timer.mockRestore()
    if (cleanup) await cleanup()
  }
  expect(commands.at(-1)).toEqual({
    type: "presence",
    clientID: commands[0]?.clientID,
    sessionIDs: [],
    sequence: 2,
    at: expect.any(Number),
  })
})
