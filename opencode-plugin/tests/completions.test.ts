import { expect, test } from "bun:test"
import { Completions, type Completion } from "../completions"

test("announces independent main sessions after one minute", async () => {
  const results: Completion[] = []
  const completions = new Completions(async (id) => `Finished ${id}.`, async (item) => { results.push(item) })
  completions.start("first", 0)
  completions.start("second", 10)
  await completions.finish("a", "second", 60_010)
  await completions.finish("b", "first", 80_000)
  expect(results.map((item) => item.sessionID)).toEqual(["second", "first"])
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

test("counts background time and announces once after the result reply", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Video completed.", async (item) => { results.push(item) })
  completions.start("session", 0)
  completions.jobStarted("job", "session", 10_000)
  await completions.finish("launch", "session", 32_000)
  expect(results).toHaveLength(0)
  completions.jobFinished("job")
  completions.resume("session", 150_000)
  await completions.finish("result", "session", 158_000)
  await completions.finish("duplicate", "session", 159_000)
  expect(results).toHaveLength(1)
  expect(results[0]?.id).toBe("result")
})

test("follow-up messages reset the timer without losing running jobs", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.start("session", 0)
  completions.jobStarted("job", "session", 10_000)
  await completions.finish("launch", "session", 32_000)
  completions.start("session", 130_000)
  await completions.finish("follow-up", "session", 135_000)
  completions.jobFinished("job")
  completions.resume("session", 150_000)
  await completions.finish("result", "session", 158_000)
  expect(results).toHaveLength(0)
})

test("waits for all jobs and preserves background tasks across reloads", async () => {
  const results: Completion[] = []
  const summarize = async () => "Done."
  const publish = async (item: Completion) => { results.push(item) }
  const before = new Completions(summarize, publish)
  before.start("session", 0)
  before.jobStarted("one", "session", 10_000)
  before.jobStarted("two", "session", 20_000)
  await before.finish("launch", "session", 70_000)
  const after = new Completions(summarize, publish)
  after.restore(before.snapshot())
  expect(after.sessionForJob("one")).toBe("session")
  after.jobFinished("one")
  after.resume("session", 80_000)
  await after.finish("first-result", "session", 90_000)
  expect(results).toHaveLength(0)
  after.jobFinished("two")
  after.resume("session", 100_000)
  await after.finish("last-result", "session", 110_000)
  expect(results).toHaveLength(1)
})

test("cancelling a background task prevents its later announcement", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.start("session", 0)
  completions.jobStarted("job", "session", 10_000)
  await completions.finish("launch", "session", 32_000)
  completions.cancel("session")
  expect(completions.sessionForJob("job")).toBeUndefined()
  completions.jobFinished("job")
  completions.resume("session", 150_000)
  await completions.finish("result", "session", 158_000)
  expect(results).toHaveLength(0)
})

test("announces when a background job lasts a minute after a follow-up reset", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.start("session", 0)
  completions.jobStarted("job", "session", 10_000)
  await completions.finish("launch", "session", 32_000)
  completions.start("session", 40_000)
  await completions.finish("follow-up", "session", 45_000)
  completions.jobFinished("job")
  completions.resume("session", 150_000)
  await completions.finish("result", "session", 158_000)
  expect(results).toHaveLength(1)
})

test("foreground commands do not defer or reset completion after they exit", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.start("session", 0)
  completions.resume("session", 100)
  completions.jobStarted("foreground", "session", 10_000)
  completions.jobFinished("foreground")
  await completions.finish("result", "session", 65_000)
  expect(results).toHaveLength(1)
})

test("a continuation invalidates an old summary without losing the task timer", async () => {
  const results: Completion[] = []
  const deferred = Promise.withResolvers<string>()
  let calls = 0
  const completions = new Completions(async () => ++calls === 1 ? deferred.promise : "Final result.", async (item) => { results.push(item) })
  completions.start("session", 0)
  const previous = completions.finish("previous", "session", 70_000)
  completions.resume("session", 75_000)
  deferred.resolve("Old result.")
  await previous
  await completions.finish("final", "session", 80_000)
  expect(results.map((item) => item.id)).toEqual(["final"])
})

test("a durable completion notice recovers a shell exit missed during reload", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.restore({ runs: [{ sessionID: "session", started: 0 }], jobs: [{ id: "shell", sessionID: "session" }] })
  completions.notice("other", { source: "shell", shellID: "shell", state: "completed" })
  expect(completions.hasJobs("session")).toBe(true)
  completions.notice("session", { source: "shell", shellID: "shell", state: "running" })
  expect(completions.hasJobs("session")).toBe(true)
  completions.notice("session", { source: "shell", shellID: "shell", state: "completed" })
  completions.resume("session", 150_000)
  await completions.finish("result", "session", 158_000)
  expect(results).toHaveLength(1)
})

test("a restored child can be deleted before ownership is looked up", async () => {
  const results: Completion[] = []
  const completions = new Completions(async () => "Done.", async (item) => { results.push(item) })
  completions.restore({ runs: [{ sessionID: "parent", started: 0 }], jobs: [{ id: "child", sessionID: "parent" }] })
  expect(completions.tracks("child")).toBe(true)
  completions.jobFinished("child")
  completions.cancel("child")
  expect(completions.tracks("child")).toBe(false)
  await completions.finish("parent-result", "parent", 80000)
  expect(results).toHaveLength(1)
})
