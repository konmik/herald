import { strict as assert } from "node:assert"
import { readFile, writeFile, appendFile } from "node:fs/promises"
import { join } from "node:path"
import { randomUUID } from "node:crypto"
import { OpenCode } from "@opencode/client"
import { Service } from "@opencode/client/service"

const [host, scenario, model, scratch, evidence, root, claude, app = root, claudePlugin] = process.argv.slice(2)
if (!host || !scenario || !scratch || !evidence || !root) throw new Error("Missing host verification arguments")
const proof = join(evidence, "host-proof.jsonl")
const marker = process.env.CIVILIZED_AGENT_HOST_MARKER!
const summaryMarker = process.env.CIVILIZED_AGENT_HOST_SUMMARY_MARKER
type ProofRow = {
  type: string
  at: number
  sessionID?: string
  hasPriorContext?: boolean
  hasConfiguredPrompt?: boolean
  prompt?: string
  result?: { isAnswered: boolean }
  event?: { agentId?: string; type?: string; created?: number; data?: { sessionID?: string } }
  background?: { status: string; type?: string }[]
}
type AnnouncementHistoryEntry = { sessionID: string; id: string; text: string }
async function readJsonLines<T>(path: string): Promise<T[]> {
  const text = await readFile(path, "utf8").catch(() => "")
  return text.slice(0, text.lastIndexOf("\n") + 1).split("\n").filter(Boolean).map((line) => JSON.parse(line))
}
const readHostProof = () => readJsonLines<ProofRow>(proof)
const readAnnouncementHistory = () => readJsonLines<AnnouncementHistoryEntry>(join(scratch, "data/history.jsonl"))
async function verifyPlaybackFinished() {
  const report = await readFile(join(evidence, "report.json"), "utf8").then((text) => JSON.parse(text)).catch(() => undefined)
  if (!report || report.finished < 1) return false
  assert.equal(report.shown, 1)
  assert.equal(report.finished, 1)
  assert.equal(report.speechStarted, 0)
  assert.equal(report.passiveWindow, true)
  assert.equal(report.focusChecked, true)
  assert.equal(report.focusUnchanged, true)
  assert.ok(report.decodedVideoFrames > 0)
  return true
}
const log = (value: object) => appendFile(join(evidence, "actions.jsonl"), JSON.stringify({ at: Date.now(), ...value }) + "\n")
const runDeadline = Date.now() + 240_000
async function waitForEvidence<T>(label: string, check: () => Promise<T | undefined>, seconds = 180): Promise<T> {
  const deadline = Math.min(runDeadline, Date.now() + seconds * 1000)
  while (Date.now() < deadline) {
    const value = await check()
    if (value !== undefined) return value
    await Bun.sleep(200)
  }
  throw new Error("Timed out waiting for " + label)
}
const seconds = host === "Claude" ? 65 : scenario === "CancelRestart" ? 90 : 30
const backgroundPrompt = `This is an isolated lifecycle verification. Do not read or edit any files. Context marker ${marker}. Start exactly one background ${scenario === "Subagent" ? "subagent using the subagent/Agent tool. Its only task is to run a synchronous shell command" : "shell command"} that runs pwsh -NoProfile -Command "Start-Sleep -Seconds ${seconds}". ${scenario === "Subagent" ? "Launch the subagent in the background." : "Set the shell tool's background/run_in_background flag to true."} Immediately give a final reply saying Background work started. Do not wait or poll. When the completion notice later arrives, give a final reply saying Background work finished. Do not start any additional work.`
let passed = false
let removed = false
try {
  if (host === "OpenCode") {
    if (!model?.includes("/")) throw new Error("OpenCode verification requires -Model provider/model")
    const file = join(scratch, "state/opencode/service.json")
    const server = Bun.spawn(["pwsh", "-NoProfile", "-Command", "opencode serve --service --port 0"], { cwd: scratch, stdout: Bun.file(join(evidence, "server-output.txt")), stderr: Bun.file(join(evidence, "server-errors.txt")) })
    const request = () => ({ signal: AbortSignal.timeout(10_000) })
    const [providerID, id] = model.split("/", 2)
    let sessionID: string | undefined
    let client: ReturnType<typeof OpenCode.make> | undefined
    try {
      const endpoint = await waitForEvidence("private OpenCode service registration", async () => {
        if (server.exitCode !== null) throw new Error("Private server exited; inspect server-errors.txt")
        return Service.discover({ file })
      }, 30)
      client = OpenCode.make({ baseUrl: endpoint.url, headers: Service.headers(endpoint) })
      const info = await client.server.info(request())
      await writeFile(join(scratch, "server.json"), JSON.stringify({ pid: info.pid, file }))
      const session = await client.session.create({ location: { directory: scratch }, title: "Civilized host verification", model: { providerID, id } }, request())
      sessionID = session.id
      await writeFile(join(scratch, "session.json"), JSON.stringify({ sessionID }))
      await log({ action: "created", sessionID })
      await client.session.prompt({ sessionID, text: backgroundPrompt }, request())
      const waiting = await waitForEvidence("idle root with background work", async () => {
        const active = await client!.session.active(request())
        const shells = await client!.shell.list({ location: { directory: scratch } }, request())
        const children = await client!.session.list({ parentID: sessionID }, request())
        const work = scenario === "Subagent" ? children.data.some((child) => child.id in active) : shells.data.some((shell) => shell.status === "running" && shell.metadata.sessionID === sessionID)
        if (!(sessionID! in active) && work) return { active, shells: shells.data, children: children.data }
        return undefined
      })
      await log({ action: "idle-with-background-work", sessionID, state: waiting })
      assert.equal((await readHostProof()).filter((record) => record.type === "generate").length, 0, "Summary started before background work finished")
      assert.equal((await readAnnouncementHistory()).length, 0, "Background work produced an early announcement")
      if (scenario === "CancelRestart") {
        await client.session.prompt({ sessionID, text: 'Run pwsh -NoProfile -Command "Start-Sleep -Seconds 60" synchronously, then reply Done. Do not read or edit files.' }, request())
        await waitForEvidence("foreground execution", async () => {
          const shells = await client!.shell.list({ location: { directory: scratch } }, request())
          return shells.data.filter((shell) => shell.status === "running" && shell.metadata.sessionID === sessionID).length >= 2 ? true : undefined
        })
        await client.session.interrupt({ sessionID, resume: false }, request())
        await client.session.prompt({ sessionID, text: "Continue the same verification. Reply only Waiting for the original background work. Do not run any tools. When that work finishes, reply Background work finished." }, request())
        await log({ action: "cancel-and-restart", sessionID })
      }
      await waitForEvidence("one context-preserving main announcement", async () => {
        const shown = await readAnnouncementHistory()
        if (!shown.length) return undefined
        assert.equal(shown.length, 1, "Expected exactly one announcement")
        assert.equal(shown[0].sessionID, sessionID, "Child session announced")
        const generation = (await readHostProof()).filter((record) => record.type === "generate")
        assert.equal(generation.length, 1, "Expected one summary fork")
        assert.equal(generation[0].sessionID, sessionID)
        assert.equal(generation[0].hasPriorContext, true)
        assert.equal(generation[0].hasConfiguredPrompt, true)
        const active = await client!.session.active(request())
        assert.equal(sessionID! in active, false, "Root still running at announcement")
        const shells = await client!.shell.list({ location: { directory: scratch } }, request())
        assert.equal(shells.data.some((shell) => shell.status === "running"), false, "Shell still running at announcement")
        if (!(await verifyPlaybackFinished())) return undefined
        return shown
      })
    } finally {
      try {
        if (sessionID && client) {
          try {
            await writeFile(join(evidence, "transcript.json"), JSON.stringify(await client.session.context({ sessionID }, request()), null, 2))
          } finally {
            await client.session.remove({ sessionID }, request())
            removed = true
          }
        }
      } finally {
        await Service.stop({ file })
        if (server.exitCode === null) {
          Bun.spawnSync(["taskkill", "/PID", String(server.pid), "/T", "/F"])
          await server.exited
        }
      }
    }
  } else {
    const sessionID = randomUUID()
    const args = ["--print", "--verbose", "--input-format", "stream-json", "--output-format", "stream-json", "--include-hook-events", "--session-id", sessionID,
      "--setting-sources", "", "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}", "--permission-mode", "dontAsk", "--allowedTools", "Bash,PowerShell,Agent,Task,TaskOutput,TaskStop",
      "--plugin-dir", claudePlugin || join(app, "claude-plugin"), "--plugin-dir", join(root, "development_tools/host-verification/claude")]
    if (model) args.push("--model", model)
    const process = Bun.spawn([claude, ...args], { cwd: scratch, stdin: "pipe", stdout: Bun.file(join(evidence, "transcript.jsonl")), stderr: Bun.file(join(evidence, "host-errors.txt")) })
    const prompt = (text: string) => process.stdin.write(JSON.stringify({ type: "user", message: { role: "user", content: text } }) + "\n")
    try {
      prompt(backgroundPrompt)
      await waitForEvidence("background work in real Claude Stop hook", async () => {
        if (process.exitCode !== null) throw new Error("Claude exited before verification completed; inspect host-errors.txt")
        const rows = await readHostProof()
        assert.equal(rows.filter((row) => row.type === "fork").length, 0, "Claude forked while background work was running")
        return rows.find((row) => row.type === "stop" && row.background?.some((job) => ["running", "pending"].includes(job.status)))
      })
      await log({ action: "background-stop", sessionID })
      if (scenario === "CancelRestart") {
        prompt("Cancel the original background job using TaskStop or the shell's stop control. Then start a new background shell running pwsh -NoProfile -Command \"Start-Sleep -Seconds 65\". Give a final reply saying Restarted. When that command finishes, reply Restarted work finished. Do not read or edit files.")
        await log({ action: "cancel-and-restart", sessionID })
      }
      await waitForEvidence("Claude fork and rendered main announcement", async () => {
        const rows = await readHostProof()
        const fork = rows.filter((row) => row.type === "fork")
        assert.equal(rows.filter((row) => row.type === "standalone").length, 0, "Standalone model generation replaced the conversation fork")
        assert.ok(fork.length <= 1, "More than one summary was generated")
        const shown = await readAnnouncementHistory()
        if (!shown.length) return undefined
        assert.equal(shown.length, 1)
        assert.equal(shown[0].sessionID, "claude:" + sessionID)
        assert.equal(fork.length, 1)
        assert.equal(fork[0].sessionID, sessionID)
        assert.equal(fork[0].result?.isAnswered, true)
        if (summaryMarker) assert.equal(fork[0].prompt?.includes(summaryMarker), true, "Claude did not use the saved summary prompt")
        const final = rows.findLast((row) => row.type === "turn.complete" && !row.event?.agentId)
        assert.ok(final && fork[0].at >= final.at, "Fork ran before the main final report")
        if (!(await verifyPlaybackFinished())) return undefined
        return shown
      })
    } finally {
      process.stdin.end()
      const deadline = Date.now() + 10000
      while (process.exitCode === null && Date.now() < deadline) await Bun.sleep(100)
      if (process.exitCode === null) {
        Bun.spawnSync(["taskkill", "/PID", String(process.pid), "/T", "/F"])
        await process.exited
      }
      removed = true
    }
  }
  passed = true
} finally {
  await writeFile(join(evidence, "host-result.json"), JSON.stringify({ host, scenario, passed, sessionsRemoved: removed, customSummary: Boolean(summaryMarker), scope: "real host generation, completion timing, isolated native history", skipped: ["TUI tab visibility", "mouse dismissal", "audio quality"] }, null, 2))
}
console.log(`PASS: ${host} ${scenario}; real conversation fork and one final main announcement`)
