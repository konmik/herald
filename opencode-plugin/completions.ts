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
    private onSummarize: (sessionID: string, failed: boolean) => Promise<string>,
    private onPublish: (completion: Completion) => Promise<void>,
    private onCheckReady: (sessionID: string, failed: boolean) => Promise<boolean>,
    private minimumDuration = 60_000,
    private onReport: (sessionID: string, reason: string) => void = () => {},
  ) {}

  start(sessionID: string, started: number) {
    this.pending.delete(sessionID)
    this.runs.set(sessionID, { started, token: {} })
  }

  resume(sessionID: string, started: number) {
    this.invalidateSummary(sessionID)
    if (!this.runs.has(sessionID)) this.start(sessionID, started)
  }

  snapshot() {
    return { runs: [...this.runs].map(([sessionID, run]) => ({ sessionID, started: run.started })) }
  }

  restore(value: unknown) {
    if (!value || typeof value !== "object" || !("runs" in value) || !Array.isArray(value.runs)) return
    for (const run of value.runs) {
      if (typeof run?.sessionID === "string" && typeof run.started === "number" && Number.isFinite(run.started)) {
        this.start(run.sessionID, run.started)
      }
    }
  }

  invalidateSummary(sessionID: string) {
    const run = this.runs.get(sessionID)
    if (run && this.pending.has(sessionID)) this.runs.set(sessionID, { started: run.started, token: {} })
    this.pending.delete(sessionID)
  }

  cancel(sessionID: string) {
    this.invalidateSummary(sessionID)
    this.runs.delete(sessionID)
  }

  async finish(id: string, sessionID: string, completed: number, failed = false) {
    const run = this.runs.get(sessionID)
    if (!run || this.pending.has(sessionID)) return
    this.pending.set(sessionID, run.token)
    let consumed = false
    try {
      if (!(await this.onCheckReady(sessionID, failed))) {
        this.onReport(sessionID, "waiting-for-background-work")
        return
      }
      if (this.pending.get(sessionID) !== run.token) return
      if (completed - run.started < this.minimumDuration) {
        consumed = true
        this.onReport(sessionID, "below-minimum-duration")
        return
      }
      const text = (await this.onSummarize(sessionID, failed)).replace(/\s+/g, " ").trim()
      if (!text || this.pending.get(sessionID) !== run.token) return
      if (!(await this.onCheckReady(sessionID, failed)) || this.pending.get(sessionID) !== run.token) return
      await this.onPublish({ id, sessionID, completed, text, emotion: "neutral" })
      consumed = true
      this.onReport(sessionID, "announcement-sent")
    } finally {
      if (this.pending.get(sessionID) === run.token) this.pending.delete(sessionID)
      if (consumed && this.runs.get(sessionID)?.token === run.token) this.runs.delete(sessionID)
    }
  }
}
