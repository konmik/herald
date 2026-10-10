// Codex lifecycle hooks for Herald. Codex runs this script once per hook event with the event JSON on stdin.
// `summarize` and `presence` are detached helpers it starts, because hook processes are short-lived.
import { spawn, spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { existsSync, mkdirSync, openSync, readdirSync, readFileSync, realpathSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { basename, dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

export const MINIMUM_DURATION = 60_000
export const PRESENCE_INTERVAL = 2_000
// A finished `codex exec` closes its session before the summary is ready; keep it present long enough to be announced.
export const ANNOUNCEMENT_GRACE = 60_000

export const keyFor = (sessionID) => 'codex:' + sessionID

export function createHerald(host) {
  const update = (sessionID, change) => {
    const next = { ...host.read(sessionID), ...change }
    host.write(sessionID, next)
    return next
  }

  async function hook(event) {
    const sessionID = event.session_id
    if (typeof sessionID !== 'string' || !sessionID) return
    const key = keyFor(sessionID)
    const at = host.now()
    if (event.hook_event_name === 'SessionStart') {
      await host.bridge({ type: 'boot' })
      await host.bridge({ type: 'discard', sessionID: key, at })
      const state = update(sessionID, { ended: false })
      if (!host.alive(state.keeper)) update(sessionID, { keeper: host.start(['presence', JSON.stringify({ sessionID, pid: host.parent })]) })
    } else if (event.hook_event_name === 'UserPromptSubmit') {
      update(sessionID, { started: at, token: host.uuid(), pending: undefined })
      await host.bridge({ type: 'discard', sessionID: key, at })
    } else if (event.hook_event_name === 'Stop') {
      const state = host.read(sessionID)
      if (typeof state.started !== 'number' || state.pending) return
      if (at - state.started < MINIMUM_DURATION || !event.last_assistant_message?.trim()) {
        update(sessionID, { started: undefined })
        return
      }
      update(sessionID, { pending: state.token })
      host.start(['summarize', JSON.stringify({ sessionID, turnID: event.turn_id, token: state.token, at, model: event.model, cwd: event.cwd })])
    } else if (event.hook_event_name === 'Interrupt') {
      update(sessionID, { started: undefined, token: host.uuid(), pending: undefined })
    } else if (event.hook_event_name === 'SessionEnd') {
      update(sessionID, { ended: true })
    }
  }

  async function summarize(job) {
    const current = () => host.read(job.sessionID).token === job.token
    try {
      const profile = JSON.parse((await host.bridge({ type: 'read-announcement-profile' })).stdout)
      if (typeof profile.prompt !== 'string') throw new Error('Herald bridge returned an invalid summary prompt')
      if (!current()) return
      const text = (await host.fork(job, profile.prompt)).replace(/\s+/g, ' ').trim()
      if (!text || !current()) return
      const key = keyFor(job.sessionID)
      await host.bridge({ type: 'notify', id: key + ':' + job.turnID, sessionID: key, presenceSessionID: key, completed: job.at, text, title: host.title(job.sessionID) || basename(job.cwd ?? ''), character: 'codex', characterID: profile.characterID, emotion: 'neutral' })
      if (current()) update(job.sessionID, { started: undefined, notified: host.now() })
    } finally {
      if (host.read(job.sessionID).pending === job.token) update(job.sessionID, { pending: undefined })
    }
  }

  async function presence(job) {
    const key = keyFor(job.sessionID)
    const clientID = key + ':' + host.uuid()
    let sequence = 0
    for (;;) {
      const state = host.read(job.sessionID)
      if (state.keeper !== host.pid) return
      const open = !state.ended && host.alive(job.pid)
      const announcing = Boolean(state.pending) || host.now() - (state.notified ?? -Infinity) < ANNOUNCEMENT_GRACE
      if (!open && !announcing) break
      await host.bridge({ type: 'presence', clientID, sessionIDs: [key], sequence: ++sequence, at: host.now() })
      await host.sleep(PRESENCE_INTERVAL)
    }
    await host.bridge({ type: 'presence', clientID, sessionIDs: [], sequence: ++sequence, at: host.now() })
    update(job.sessionID, { keeper: undefined })
  }

  return { hook, summarize, presence }
}

function dataDirectory() {
  if (process.env.HERALD_DATA) return process.env.HERALD_DATA
  if (process.platform === 'win32') return join(process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local'), 'herald')
  if (process.platform === 'darwin') return join(homedir(), 'Library', 'Application Support', 'herald')
  return join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'herald')
}

function nativeRuntime(root) {
  const installed = join(root, 'native-announcer')
  const runtime = existsSync(installed) ? installed : join(root, '..', 'native-announcer')
  if (runtime !== installed && process.env.HERALD_BINARY) return { binary: process.env.HERALD_BINARY, assets: join(runtime, 'resources') }
  const names = readdirSync(join(runtime, 'bin')).filter(name => /^herald-(?:win32|darwin|linux)-(?:x64|arm64)(?:\.exe)?$/.test(name))
  if (names.length !== 1) throw new Error('Herald runtime is missing or ambiguous. Reinstall the application bundle.')
  return { binary: join(runtime, 'bin', names[0]), assets: join(runtime, 'resources') }
}

function sessionTitle(sessionID) {
  try {
    const index = readFileSync(join(process.env.CODEX_HOME ?? join(homedir(), '.codex'), 'session_index.jsonl'), 'utf8')
    let title = ''
    for (const line of index.split('\n')) {
      if (!line.includes(sessionID)) continue
      const entry = JSON.parse(line)
      if (entry.id === sessionID && typeof entry.thread_name === 'string') title = entry.thread_name
    }
    return title
  } catch {
    return ''
  }
}

function nodeHost() {
  const script = fileURLToPath(import.meta.url)
  const root = process.env.PLUGIN_ROOT ?? dirname(dirname(script))
  const runtime = nativeRuntime(root)
  const data = process.env.PLUGIN_DATA ?? join(dataDirectory(), 'codex')
  const sessions = join(data, 'sessions')
  const file = (sessionID) => join(sessions, sessionID.replace(/[^\w.-]/g, '_') + '.json')
  return {
    pid: process.pid,
    parent: process.ppid,
    now: () => Date.now(),
    uuid: () => randomUUID(),
    sleep: (ms) => new Promise(done => setTimeout(done, ms)),
    alive(pid) {
      if (typeof pid !== 'number') return false
      try { process.kill(pid, 0); return true } catch (error) { return error.code === 'EPERM' }
    },
    read(sessionID) {
      try { return JSON.parse(readFileSync(file(sessionID), 'utf8')) } catch { return {} }
    },
    write(sessionID, state) {
      mkdirSync(sessions, { recursive: true, mode: 0o700 })
      const temporary = file(sessionID) + '.' + randomUUID() + '.tmp'
      writeFileSync(temporary, JSON.stringify(state), { mode: 0o600 })
      renameSync(temporary, file(sessionID))
    },
    async bridge(command) {
      const result = spawnSync(runtime.binary, ['--bridge', '--assets', runtime.assets], { input: JSON.stringify(command), encoding: 'utf8', timeout: 10_000, windowsHide: true })
      if (result.status !== 0) throw new Error('Herald bridge failed: ' + (result.stderr || result.error))
      return result
    },
    start(args) {
      mkdirSync(data, { recursive: true, mode: 0o700 })
      const log = openSync(join(data, 'helpers.log'), 'a', 0o600)
      const child = spawn(process.execPath, [script, ...args], { detached: true, stdio: ['ignore', log, log], windowsHide: true, env: { ...process.env, PLUGIN_ROOT: root } })
      child.unref()
      return child.pid
    },
    async fork(job, prompt) {
      // Fork the finished conversation so the summary sees its full context without adding a turn to it.
      const output = join(tmpdir(), `herald-codex-${randomUUID()}.txt`)
      const args = ['exec', 'fork', job.sessionID, '-', '--ephemeral', '--disable', 'hooks', '--skip-git-repo-check', '-c', 'sandbox_mode="read-only"', '--output-last-message', output, ...(job.model ? ['--model', job.model] : [])]
      try {
        const result = spawnSync('codex', args, { cwd: job.cwd, input: prompt, encoding: 'utf8', timeout: 300_000, windowsHide: true, shell: process.platform === 'win32' })
        if (result.status !== 0) throw new Error('Codex fork failed: ' + (result.stderr || result.error))
        return readFileSync(output, 'utf8')
      } finally {
        rmSync(output, { force: true })
      }
    },
    title: sessionTitle,
  }
}

async function main() {
  const [mode, job] = process.argv.slice(2)
  const herald = createHerald(nodeHost())
  if (mode === 'summarize') return herald.summarize(JSON.parse(job))
  if (mode === 'presence') return herald.presence(JSON.parse(job))
  const chunks = []
  for await (const chunk of process.stdin) chunks.push(chunk)
  await herald.hook(JSON.parse(Buffer.concat(chunks).toString('utf8')))
}

if (process.argv[1] && fileURLToPath(import.meta.url) === realpathSync(process.argv[1])) {
  main().catch(error => {
    console.error('Herald: ' + (error?.message ?? error))
    process.exit(1)
  })
}
