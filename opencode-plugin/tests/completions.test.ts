import { expect, test } from "bun:test"
import { Completions, type Completion } from "../completions"

test("announces independent main sessions after one minute", async () => {
  const results: Completion[] = []
  const c = new Completions(async (id) => `Finished ${id}.`, async (item) => { results.push(item) }, async () => true)
  c.start("first", 0)
  c.start("second", 10)
  await c.finish("a", "second", 60_010)
  await c.finish("b", "first", 80_000)
  expect(results.map((item) => item.sessionID)).toEqual(["second", "first"])
})

test("ignores short, interrupted, and duplicate completions", async () => {
  const results: Completion[] = []
  const c = new Completions(async () => "Done.", async (item) => { results.push(item) }, async () => true)
  c.start("short", 0)
  await c.finish("a", "short", 59_999)
  c.start("cancelled", 0)
  c.cancel("cancelled")
  await c.finish("b", "cancelled", 90_000)
  c.start("long", 0)
  await c.finish("c", "long", 60_000)
  await c.finish("c", "long", 60_000)
  expect(results).toHaveLength(1)
})

test("viewing, restarting, continuing, or cancelling invalidates an in-flight summary", async () => {
  for (const action of ["invalidateSummary", "start", "resume", "cancel"] as const) {
    const results: Completion[] = []
    const deferred = Promise.withResolvers<string>()
    const c = new Completions(() => deferred.promise, async (item) => { results.push(item) }, async () => true)
    c.start("root", 0)
    const pending = c.finish("a", "root", 60_000)
    await Bun.sleep(0)
    if (action === "invalidateSummary" || action === "cancel") c[action]("root")
    else c[action]("root", 70_000)
    deferred.resolve("Done.")
    await pending
    expect(results).toEqual([])
  }
})

test("checks current work before the minimum and preserves the timer until the final reply", async () => {
  const results: Completion[] = []
  let ready = false
  const c = new Completions(async () => "Done.", async (item) => { results.push(item) }, async () => ready)
  c.start("root", 0)
  await c.finish("launch", "root", 30_000)
  expect(c.snapshot().runs).toEqual([{ sessionID: "root", started: 0 }])
  ready = true
  expect(results).toEqual([])
  c.resume("root", 80_000)
  await c.finish("final", "root", 90_000)
  expect(results.map((item) => item.id)).toEqual(["final"])
})

test("restores timers from old snapshots without restoring obsolete job counts", async () => {
  const c = new Completions(async () => "Done.", async () => {}, async () => true)
  c.restore({ runs: [{ sessionID: "root", started: 0 }], jobs: [{ id: "stale", sessionID: "root" }] })
  expect(c.snapshot()).toEqual({ runs: [{ sessionID: "root", started: 0 }] })
})

test("new messages reset the timer while continuations preserve it", async () => {
  const results: Completion[] = []
  const c = new Completions(async () => "Done.", async (item) => { results.push(item) }, async () => true)
  c.start("root", 0)
  c.start("root", 70_000)
  c.resume("root", 90_000)
  await c.finish("short", "root", 100_000)
  expect(results).toEqual([])
})

test("work starting during summarization blocks delivery without losing the timer", async () => {
  const results: Completion[] = []
  let ready = true
  const c = new Completions(async () => { ready = false; return "Done." }, async (item) => { results.push(item) }, async () => ready, 0)
  c.start("root", 0)
  await c.finish("early", "root", 70_000)
  expect(results).toEqual([])
  expect(c.snapshot().runs).toHaveLength(1)
})

test("new input during a state query invalidates that completion", async () => {
  const deferred = Promise.withResolvers<boolean>()
  const results: Completion[] = []
  const c = new Completions(async () => "Done.", async (item) => { results.push(item) }, () => deferred.promise, 0)
  c.start("root", 0)
  const pending = c.finish("old", "root", 70_000)
  c.start("root", 80_000)
  deferred.resolve(true)
  await pending
  expect(results).toEqual([])
  expect(c.snapshot().runs).toEqual([{ sessionID: "root", started: 80_000 }])
})

test("invalidating a summary during delivery preserves the continuing task timer", async () => {
  const delivery = Promise.withResolvers<void>()
  const publishing = Promise.withResolvers<void>()
  const c = new Completions(async () => "Done.", async () => {
    publishing.resolve()
    await delivery.promise
  }, async () => true)
  c.start("root", 0)
  const pending = c.finish("old", "root", 70_000)
  await publishing.promise
  c.invalidateSummary("root")
  c.resume("root", 80_000)
  delivery.resolve()
  await pending
  expect(c.snapshot().runs).toEqual([{ sessionID: "root", started: 0 }])
})

test("state, summary, and delivery failures preserve the timer for another final completion", async () => {
  for (const stage of ["state", "summary", "delivery"] as const) {
    let fail = true
    const results: Completion[] = []
    const c = new Completions(
      async () => { if (fail && stage === "summary") throw new Error(stage); return "Tests failed." },
      async (item) => { if (fail && stage === "delivery") throw new Error(stage); results.push(item) },
      async () => { if (fail && stage === "state") throw new Error(stage); return true },
      0,
    )
    c.start("root", 0)
    await expect(c.finish("old", "root", 70_000, true)).rejects.toThrow(stage)
    expect(c.snapshot().runs).toHaveLength(1)
    fail = false
    await c.finish("final", "root", 80_000, true)
    expect(results[0]?.text).toBe("Tests failed.")
    expect(results[0]?.emotion).toBe("neutral")
  }
})
