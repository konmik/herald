import { expect, test } from "bun:test"
import { consumeEvents } from "../events"

test("a failed event does not stop later events", async () => {
  const controller = new AbortController()
  const handled: number[] = []
  const errors: unknown[] = []
  await consumeEvents(async function* () {
    yield 1
    yield 2
  }, async (event) => {
    if (event === 1) throw new Error("Session lookup failed")
    handled.push(event)
    controller.abort()
  }, controller.signal, (error) => errors.push(error))
  expect(handled).toEqual([2])
  expect(errors).toHaveLength(1)
})

test("reconnects after a broken subscription", async () => {
  const controller = new AbortController()
  let subscriptions = 0
  const handled: number[] = []
  await consumeEvents(async function* () {
    if (++subscriptions === 1) throw new Error("Stream disconnected")
    yield 2
  }, async (event) => {
    handled.push(event)
    controller.abort()
  }, controller.signal, () => {}, 1)
  expect(subscriptions).toBe(2)
  expect(handled).toEqual([2])
})

test("reconnects after a subscription ends and cancels the retry on unload", async () => {
  const controller = new AbortController()
  let subscriptions = 0
  const task = consumeEvents(async function* () {
    subscriptions++
  }, async () => {}, controller.signal, () => {}, 60_000)
  await Bun.sleep(1)
  controller.abort()
  await task
  expect(subscriptions).toBe(1)
})
