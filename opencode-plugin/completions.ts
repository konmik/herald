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
  private jobs = new Map<string, { sessionID: string; started?: number }>()

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
    if (!this.runs.has(sessionID)) return
    this.resume(sessionID, started)
    this.jobs.set(id, { sessionID, started })
  }

  jobFinished(id: string) {
    this.jobs.delete(id)
  }

  reparent(sessionID: string, rootID: string) {
    if (sessionID === rootID) return
    this.runs.delete(sessionID)
    this.pending.delete(sessionID)
    for (const [id, job] of this.jobs) {
      if (job.sessionID === sessionID) this.jobs.set(id, { ...job, sessionID: rootID })
    }
  }

  sessionForJob(id: string) {
    return this.jobs.get(id)?.sessionID
  }

  tracks(sessionID: string) {
    return this.runs.has(sessionID) || this.jobs.has(sessionID)
  }

  hasJobs(sessionID: string) {
    return [...this.jobs.values()].some((job) => job.sessionID === sessionID)
  }

  notice(sessionID: string, metadata: Record<string, unknown> | undefined, created?: number) {
    if (!metadata || !["completed", "cancelled", "error"].includes(String(metadata.state))) return
    const id = metadata.source === "shell" ? metadata.shellID : metadata.source === "subagent" ? metadata.childID : undefined
    if (typeof id !== "string") return
    const job = this.jobs.get(id)
    if (job?.sessionID === sessionID && (job.started === undefined || created === undefined || created >= job.started)) this.jobFinished(id)
  }

  snapshot() {
    return {
      runs: [...this.runs].map(([sessionID, run]) => ({ sessionID, started: run.started })),
      jobs: [...this.jobs].map(([id, job]) => ({ id, sessionID: job.sessionID, ...(job.started === undefined ? {} : { started: job.started }) })),
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
        this.jobs.set(job.id, {
          sessionID: job.sessionID,
          ...(typeof job.started === "number" && Number.isFinite(job.started) ? { started: job.started } : {}),
        })
      }
    }
  }

  view(sessionID: string) {
    this.pending.delete(sessionID)
  }

  cancel(sessionID: string) {
    this.view(sessionID)
    this.runs.delete(sessionID)
    for (const [id, job] of this.jobs) {
      if (job.sessionID === sessionID) this.jobs.delete(id)
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
