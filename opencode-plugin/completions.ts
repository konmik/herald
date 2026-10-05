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
  private jobs = new Map<string, string>()

  constructor(
    private summarize: (sessionID: string, failed: boolean) => Promise<string>,
    private publish: (completion: Completion) => Promise<void>,
    private minimum = 60_000,
    private report: (sessionID: string, reason: string) => void = () => {},
  ) {}

  start(sessionID: string, started: number) {
    this.pending.delete(sessionID)
    this.runs.set(sessionID, { started, token: {} })
  }

  resume(sessionID: string, started: number) {
    const run = this.runs.get(sessionID)
    if (run && this.pending.has(sessionID)) this.runs.set(sessionID, { started: run.started, token: {} })
    this.pending.delete(sessionID)
    if (!this.runs.has(sessionID)) this.start(sessionID, started)
  }

  jobStarted(id: string, sessionID: string, started: number) {
    this.resume(sessionID, started)
    this.jobs.set(id, sessionID)
  }

  jobFinished(id: string) {
    this.jobs.delete(id)
  }

  sessionForJob(id: string) {
    return this.jobs.get(id)
  }

  hasJobs(sessionID: string) {
    return [...this.jobs.values()].includes(sessionID)
  }

  notice(sessionID: string, metadata: Record<string, unknown> | undefined) {
    if (!metadata || !["completed", "cancelled", "error"].includes(String(metadata.state))) return
    const id = metadata.source === "shell" ? metadata.shellID : metadata.source === "subagent" ? metadata.childID : undefined
    if (typeof id === "string" && this.jobs.get(id) === sessionID) this.jobFinished(id)
  }

  snapshot() {
    return {
      runs: [...this.runs].map(([sessionID, run]) => ({ sessionID, started: run.started })),
      jobs: [...this.jobs].map(([id, sessionID]) => ({ id, sessionID })),
    }
  }

  restore(value: unknown) {
    if (!value || typeof value !== "object" || !("runs" in value) || !("jobs" in value)) return
    if (!Array.isArray(value.runs) || !Array.isArray(value.jobs)) return
    for (const run of value.runs) {
      if (typeof run?.sessionID === "string" && typeof run.started === "number" && Number.isFinite(run.started)) {
        this.start(run.sessionID, run.started)
      }
    }
    for (const job of value.jobs) {
      if (typeof job?.id === "string" && typeof job.sessionID === "string" && this.runs.has(job.sessionID)) {
        this.jobs.set(job.id, job.sessionID)
      }
    }
  }

  view(sessionID: string) {
    this.pending.delete(sessionID)
  }

  cancel(sessionID: string) {
    this.view(sessionID)
    this.runs.delete(sessionID)
    for (const [id, owner] of this.jobs) {
      if (owner === sessionID) this.jobs.delete(id)
    }
  }

  async finish(id: string, sessionID: string, completed: number, failed = false) {
    const run = this.runs.get(sessionID)
    if (!run || this.pending.has(sessionID)) return
    if (this.hasJobs(sessionID)) {
      this.report(sessionID, "waiting-for-background-jobs")
      return
    }
    if (completed - run.started < this.minimum) {
      this.runs.delete(sessionID)
      this.report(sessionID, "below-minimum-duration")
      return
    }
    this.pending.set(sessionID, run.token)
    try {
      const text = (await this.summarize(sessionID, failed)).replace(/\s+/g, " ").trim()
      if (!text || this.pending.get(sessionID) !== run.token) return
      await this.publish({ id, sessionID, completed, text, emotion: "neutral" })
      this.report(sessionID, "announcement-sent")
    } finally {
      if (this.pending.get(sessionID) === run.token) this.pending.delete(sessionID)
      if (this.runs.get(sessionID)?.token === run.token) this.runs.delete(sessionID)
    }
  }
}
