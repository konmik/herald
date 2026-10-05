import { setTimeout } from "node:timers/promises"

export async function consumeEvents<T>(
  subscribe: (signal: AbortSignal) => AsyncIterable<T>,
  handle: (event: T) => Promise<void>,
  signal: AbortSignal,
  report: (error: unknown) => void,
  retryMilliseconds = 1000,
) {
  let delay = retryMilliseconds
  while (!signal.aborted) {
    try {
      for await (const event of subscribe(signal)) {
        if (signal.aborted) return
        delay = retryMilliseconds
        try {
          await handle(event)
        } catch (error) {
          if (!signal.aborted) report(error)
        }
      }
    } catch (error) {
      if (!signal.aborted) report(error)
    }
    if (signal.aborted) return
    try {
      await setTimeout(delay, undefined, { signal })
    } catch (error) {
      if (signal.aborted) return
      throw error
    }
    delay = Math.min(delay * 2, 30_000)
  }
}
