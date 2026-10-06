import { expect, test } from "bun:test"
import { Completions, type Completion } from "../src/completions"

test("announces tasks after one minute and includes child sessions independently", async () => {
  const results: Completion[] = []
  const completions = new Completions(async (id) => `Finished ${id}.`, async (item) => { results.push(item) })
  completions.start("parent", 0)
  completions.start("child", 10)
  await completions.finish("a", "child", 60_010)
  await completions.finish("b", "parent", 80_000)
  expect(results.map((item) => item.sessionID)).toEqual(["child", "parent"])
  expect(results.every((item) => item.emotion === "neutral")).toBe(true)
})

test("ignores short, interrupted, and duplicate completions", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.start("short", 0)
  await completions.finish("a", "short", 59_999)
  completions.start("cancelled", 0)
  completions.cancel("cancelled")
  await completions.finish("b", "cancelled", 90_000)
  completions.start("long", 0)
  await completions.finish("c", "long", 60_000)
  await completions.finish("c", "long", 60_000)
  expect(results).toHaveLength(1)
})

test("opening or restarting a session cancels an in-flight summary", async () => {
  for (const action of ["view", "start"] as const) {
    const results: Completion[] = []
    const deferred = Promise.withResolvers<string>()
    const completions = new Completions(() => deferred.promise, async (item) => { results.push(item) })
    completions.start("session", 0)
    const pending = completions.finish("a", "session", 60_000)
    if (action === "view") completions.view("session")
    if (action === "start") completions.start("session", 70_000)
    deferred.resolve("Done.")
    await pending
    expect(results).toHaveLength(0)
  }
})

test("uses neutral expression for failure and preserves its summary", async () => {
  const results: Completion[] = []
  const completions = new Completions(async (_, failed) => failed ? "Tests failed." : "Done.", async (item) => { results.push(item) })
  completions.start("session", 0)
  await completions.finish("a", "session", 60_000, true)
  expect(results[0]?.emotion).toBe("neutral")
  expect(results[0]?.text).toBe("Tests failed.")
})
