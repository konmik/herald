export type Completion = {
  id: string
  sessionID: string
  completed: number
  text: string
  emotion: "neutral"
}

export class Completions {
  private runs = new Map<string, { started: number; token: object }>()
  private pending = new Map<string, object>()

  constructor(
    private summarize: (sessionID: string, failed: boolean) => Promise<string>,
    private publish: (completion: Completion) => Promise<void>,
    private minimum = 60_000,
  ) {}

  start(sessionID: string, started: number) {
    this.pending.delete(sessionID)
    this.runs.set(sessionID, { started, token: {} })
  }

  view(sessionID: string) {
    this.pending.delete(sessionID)
  }

  cancel(sessionID: string) {
    this.view(sessionID)
    this.runs.delete(sessionID)
  }

  async finish(id: string, sessionID: string, completed: number, failed = false) {
    const run = this.runs.get(sessionID)
    if (!run) return
    this.runs.delete(sessionID)
    if (completed - run.started < this.minimum) return
    this.pending.set(sessionID, run.token)
    try {
      const text = (await this.summarize(sessionID, failed)).replace(/\s+/g, " ").trim()
      if (!text || this.pending.get(sessionID) !== run.token) return
      await this.publish({ id, sessionID, completed, text, emotion: "neutral" })
    } finally {
      if (this.pending.get(sessionID) === run.token) this.pending.delete(sessionID)
    }
  }
}
