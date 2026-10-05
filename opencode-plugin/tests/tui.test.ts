import { expect, mock, test } from "bun:test"

let update = () => {}
let cleanup = () => {}
const commands: { sessionIDs: string[]; sequence: number }[] = []

mock.module("@opencode/plugin/tui", () => ({ Plugin: { define: (value: unknown) => value } }))
mock.module("solid-js", () => ({
  createEffect: (effect: () => void) => { update = effect; effect() },
  onCleanup: (effect: () => void) => { cleanup = effect },
}))
mock.module("../bridge", () => ({ send: async (command: typeof commands[number]) => { commands.push(command) } }))

const { default: exported } = await import("../tui")
const plugin = exported as unknown as { setup: (context: unknown) => void }

test("keeps background tabs present and clears presence when the window closes", () => {
  commands.length = 0
  let tabs = [{ sessionID: "foreground" }, { sessionID: "background" }]
  plugin.setup({
    ui: {
      slot: ({ render }: { render: () => void }) => render(),
      router: { current: () => ({ type: "session", sessionID: "foreground" }) },
      tabs: { enabled: () => true, list: () => tabs },
    },
  })
  try {
    expect(commands[0]?.sessionIDs).toEqual(["foreground", "background"])
    tabs = [{ sessionID: "background" }]
    update()
    expect(commands.at(-1)?.sessionIDs).toEqual(["background"])
  } finally {
    cleanup()
  }
  expect(commands.at(-1)?.sessionIDs).toEqual([])
  expect(commands.map((command) => command.sequence)).toEqual([1, 2, 3])
})

test("a child route keeps its owning session present without requiring focus", () => {
  commands.length = 0
  plugin.setup({
    ui: {
      slot: ({ render }: { render: () => void }) => render(),
      router: { current: () => ({ type: "session", sessionID: "child" }) },
      tabs: { enabled: () => false },
    },
    data: { session: { root: () => "parent" } },
  })
  try {
    expect(commands[0]?.sessionIDs).toEqual(["parent"])
  } finally {
    cleanup()
  }
})
