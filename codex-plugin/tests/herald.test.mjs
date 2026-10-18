import { describe, expect, test } from 'bun:test'
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { spawn, spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { ANNOUNCEMENT_GRACE, codexAncestor, completedExecutions, createHerald } from '../hooks/herald.mjs'

function fakeHost({ summary = 'The parser now handles empty files.', profile = { prompt: 'Summarize.', characterID: 'royal' } } = {}) {
  const host = {
    clock: 1_000_000,
    sent: [],
    started: [],
    forks: [],
    states: new Map(),
    live: new Set([4242]),
    pid: 7,
    parent: 4242,
    uuids: 0,
    now: () => host.clock,
    uuid: () => 'token-' + ++host.uuids,
    sleep: async (ms) => { host.clock += ms; host.onSleep?.() },
    alive: (pid) => host.live.has(pid),
    read: (sessionID) => ({ ...host.states.get(sessionID) }),
    write: (sessionID, state) => host.states.set(sessionID, JSON.parse(JSON.stringify(state))),
    lock: async (_sessionID, action) => action(),
    completedExecutions: async () => new Set(),
    bridge: async (command) => {
      host.sent.push(command)
      return { stdout: command.type === 'read-announcement-profile' ? JSON.stringify(profile) : '' }
    },
    start: (args) => { host.started.push([args[0], JSON.parse(args[1])]); return 99 },
    fork: async (job, prompt) => { host.forks.push({ sessionID: job.sessionID, prompt }); host.onFork?.(); return summary },
    title: () => 'Fix the parser',
  }
  return host
}

const event = (name, extra = {}) => ({ session_id: 's1', turn_id: 't1', cwd: '/work', model: 'gpt-6.1-sol', hook_event_name: name, ...extra })
const processes = (rows) => new Map(rows.map(row => [row.pid, row]))
const withoutSQLiteWarning = stderr => stderr.replace(/^\(node:\d+\) ExperimentalWarning: SQLite is an experimental feature and might change at any time\r?\n\(Use `node --trace-warnings \.\.\.` to show where the warning was created\)\r?\n/gm, '')

async function runTask(host, herald, seconds) {
  await herald.hook(event('UserPromptSubmit', { prompt: 'Fix the parser' }))
  host.clock += seconds * 1000
  await herald.hook(event('Stop', { last_assistant_message: 'Done.' }))
}

async function nativeHookFixture(action) {
  const directory = mkdtempSync(fileURLToPath(new URL('../../temp/codex-lock-', import.meta.url)))
  const statePath = join(directory, 'sessions', 's1.json')
  const children = []
  const start = (id, pause = false) => {
    const child = spawn('node', ['--import', new URL('./fixtures/pause-state-write.mjs', import.meta.url).href, fileURLToPath(new URL('../hooks/herald.mjs', import.meta.url))], {
      env: { ...process.env, PLUGIN_ROOT: fileURLToPath(new URL('..', import.meta.url)), PLUGIN_DATA: directory, HERALD_BINARY: process.execPath, HERALD_TEST_PAUSE_STATE_WRITE: pause ? '1' : '' },
      stdio: ['pipe', 'ignore', 'pipe', 'ipc'], windowsHide: true,
    })
    let stderr = ''
    const messages = new Set()
    child.stderr.on('data', chunk => { stderr += chunk })
    child.on('message', message => messages.add(message))
    const timer = setTimeout(() => child.kill('SIGKILL'), 15_000)
    const result = new Promise(done => {
      child.on('error', error => done({ error }))
      child.on('close', (code, signal) => { clearTimeout(timer); done({ code, signal, stderr: withoutSQLiteWarning(stderr) }) })
    })
    const waitFor = async message => {
      const deadline = Date.now() + 5_000
      while (!messages.has(message)) {
        if (Date.now() >= deadline || child.exitCode !== null) throw new Error('Hook did not signal ' + message + ': ' + stderr)
        await new Promise(done => setTimeout(done, 10))
      }
    }
    children.push({ child, result })
    child.stdin.end(JSON.stringify(event('PreToolUse', { tool_name: 'Bash', tool_use_id: id })))
    return { child, result, waitFor }
  }
  mkdirSync(dirname(statePath), { recursive: true })
  const before = { started: 123, token: 'original', keeper: 456, work: { 'tool:existing': null }, agents: {}, spawns: {} }
  writeFileSync(statePath, JSON.stringify(before))
  try {
    await action({ directory, statePath, before, start, read: () => JSON.parse(readFileSync(statePath, 'utf8')) })
  } finally {
    for (const { child } of children) child.kill('SIGKILL')
    await Promise.all(children.map(({ result }) => result))
    rmSync(directory, { recursive: true, force: true })
  }
}

describe('codex hooks', () => {
  test('native hooks recover an abandoned legacy lock without losing session state', async () => {
    await nativeHookFixture(async ({ statePath, before, start, read }) => {
      mkdirSync(statePath + '.lock')
      expect(await start('next').result).toEqual({ code: 0, signal: null, stderr: '' })
      expect(read()).toEqual({ ...before, token: expect.any(String), work: { 'tool:existing': null, 'tool:next': null } })
    })
  }, 20_000)

  test('native hooks wait for a live owner and recover after it is killed', async () => {
    await nativeHookFixture(async ({ before, start, read }) => {
      const owner = start('owner', true)
      await owner.waitFor('written')
      const ownerState = read()
      expect(ownerState).toEqual({ ...before, token: expect.any(String), work: { 'tool:existing': null, 'tool:owner': null } })
      const contender = start('contender')
      await contender.waitFor('started')
      expect(await Promise.race([contender.result, new Promise(done => setTimeout(() => done('waiting'), 200))])).toBe('waiting')
      expect(read()).toEqual(ownerState)
      owner.child.kill('SIGKILL')
      await owner.result
      expect(await contender.result).toEqual({ code: 0, signal: null, stderr: '' })
      expect(read()).toEqual({ ...ownerState, token: expect.any(String), work: { 'tool:existing': null, 'tool:owner': null, 'tool:contender': null } })
    })
  }, 25_000)

  test('concurrent native hook processes retain every background task', async () => {
    const directory = mkdtempSync(fileURLToPath(new URL('../../temp/codex-lock-', import.meta.url)))
    const script = fileURLToPath(new URL('../hooks/herald.mjs', import.meta.url))
    const run = input => new Promise((done, fail) => {
      const child = spawn(process.execPath, [script], { env: { ...process.env, PLUGIN_ROOT: fileURLToPath(new URL('..', import.meta.url)), PLUGIN_DATA: directory, HERALD_BINARY: process.execPath }, stdio: ['pipe', 'ignore', 'pipe'] })
      let errors = ''
      child.stderr.on('data', chunk => { errors += chunk })
      child.on('error', fail)
      child.on('close', code => code === 0 ? done() : fail(new Error(errors)))
      child.stdin.end(JSON.stringify(input))
    })
    try {
      const ids = ['one', 'two', 'three', 'four']
      await Promise.all(ids.map(id => run(event('PreToolUse', { tool_name: 'Bash', tool_use_id: id }))))
      const state = () => JSON.parse(readFileSync(directory + '/sessions/s1.json', 'utf8'))
      expect(Object.keys(state().work).sort()).toEqual(['tool:four', 'tool:one', 'tool:three', 'tool:two'])
      await Promise.all(ids.map(id => run(event('PostToolUse', { tool_name: 'Bash', tool_use_id: id }))))
      expect(state().work).toEqual({})
    } finally {
      rmSync(directory, { recursive: true, force: true })
    }
  })

  test('registers main completion and background lifecycle hooks', () => {
    const config = JSON.parse(readFileSync(new URL('../hooks/hooks.json', import.meta.url), 'utf8'))
    expect(Object.keys(config.hooks)).toEqual(['UserPromptSubmit', 'SubagentStart', 'SubagentStop', 'PreToolUse', 'PostToolUse', 'Interrupt', 'Stop'])
  })

  test.skipIf(process.platform !== 'win32')('Windows hook commands run in PowerShell and cmd with spaces in the plugin path', () => {
    const directory = mkdtempSync(fileURLToPath(new URL('../../temp/codex hook ', import.meta.url)))
    const config = JSON.parse(readFileSync(new URL('../hooks/hooks.json', import.meta.url), 'utf8'))
    const commands = new Set(Object.values(config.hooks).flatMap(entries => entries.flatMap(entry => entry.hooks.map(hook => hook.commandWindows))))
    try {
      mkdirSync(join(directory, 'hooks'))
      copyFileSync(new URL('../hooks/herald.mjs', import.meta.url), join(directory, 'hooks', 'herald.mjs'))
      for (const command of commands) {
        for (const [shell, args] of [['pwsh', ['-NoProfile', '-Command', command]], ['cmd.exe', ['/d', '/c', command]]]) {
          const result = spawnSync(shell, args, {
            env: { ...process.env, PLUGIN_ROOT: directory, PLUGIN_DATA: directory, HERALD_BINARY: process.execPath },
            input: JSON.stringify(event('PreToolUse', { tool_name: 'Bash', tool_use_id: shell })),
            encoding: 'utf8', timeout: 10_000, windowsHide: true, windowsVerbatimArguments: shell === 'cmd.exe',
          })
          expect({ shell, status: result.status, stderr: withoutSQLiteWarning(result.stderr), error: result.error }).toEqual({ shell, status: 0, stderr: '', error: undefined })
          const state = JSON.parse(readFileSync(join(directory, 'sessions', 's1.json'), 'utf8'))
          expect(state.work['tool:' + shell]).toBeNull()
        }
      }
    } finally {
      rmSync(directory, { recursive: true, force: true })
    }
  })

  test('titles an unnamed exec session after its project folder', async () => {
    const host = fakeHost()
    host.title = () => ''
    const herald = createHerald(host)
    await herald.summarize({ sessionID: 's1', turnID: 't1', token: undefined, at: 1_000, model: 'gpt-6.1-sol', cwd: '/work/herald' })
    expect(host.sent.at(-1).title).toBe('herald')
  })

  test('announces a task that ran for at least a minute through a forked summary', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await runTask(host, herald, 61)
    expect(host.started).toEqual([['presence', { sessionID: 's1', pid: 4242 }], ['summarize', { sessionID: 's1', turnID: 't1', token: 'token-1', at: 1_061_000, model: 'gpt-6.1-sol', cwd: '/work' }]])
    expect(host.read('s1').started).toBe(1_000_000)
    await herald.summarize(host.started[1][1])
    expect(host.forks).toEqual([{ sessionID: 's1', prompt: 'Summarize.' }])
    expect(host.sent.at(-1)).toEqual({ type: 'notify', id: 'codex:s1:t1', sessionID: 'codex:s1', presenceSessionID: 'codex:s1', completed: 1_061_000, text: 'The parser now handles empty files.', title: 'Fix the parser', character: 'codex', characterID: 'royal', emotion: 'neutral' })
  })

  test('stays silent for tasks shorter than a minute', async () => {
    const host = fakeHost()
    await runTask(host, createHerald(host), 59)
    expect(host.started.map(([mode]) => mode)).toEqual(['presence'])
    expect(host.read('s1').started).toBeUndefined()
  })

  test('duplicate stops do not start another summary', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await runTask(host, herald, 60)
    await herald.hook(event('Stop', { last_assistant_message: 'Done.' }))
    expect(host.started.filter(([mode]) => mode === 'summarize')).toHaveLength(1)
  })

  for (const seconds of [30, 70]) {
    for (const kind of ['agent', 'shell']) {
      test(`${kind} work preserves a ${seconds}s task until the final main stop`, async () => {
        const host = fakeHost({ summary: 'All background work finished.' })
        const herald = createHerald(host)
        await herald.hook(event('UserPromptSubmit', { prompt: 'Run background work.' }))
        const start = kind === 'agent'
          ? event('SubagentStart', { agent_id: 'child', agent_type: 'worker' })
          : event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'shell', tool_input: { command: 'sleep 65' } })
        await herald.hook(start)
        host.clock += seconds * 1000
        await herald.hook(event('Stop', { last_assistant_message: 'Background work started.' }))
        expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
        host.clock += 40_000
        await herald.hook(kind === 'agent'
          ? event('SubagentStop', { agent_id: 'child', agent_type: 'worker', last_assistant_message: 'Finished.' })
          : event('PostToolUse', { tool_name: 'Bash', tool_use_id: 'shell', tool_response: { exit_code: 0 } }))
        expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
        await herald.hook(event('Stop', { turn_id: 'final', last_assistant_message: 'All background work finished.' }))
        const jobs = host.started.filter(([mode]) => mode === 'summarize')
        expect(jobs).toHaveLength(1)
        await herald.summarize(jobs[0][1])
        expect(host.sent.filter(command => command.type === 'notify')).toEqual([{
          type: 'notify', id: 'codex:s1:final', sessionID: 'codex:s1', presenceSessionID: 'codex:s1', completed: host.clock,
          text: 'All background work finished.', title: 'Fix the parser', character: 'codex', characterID: 'royal', emotion: 'neutral',
        }])
      })
    }
  }

  test('a new prompt resets the task timer and restarts a finished keeper', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await runTask(host, herald, 59)
    host.clock += 120_000
    await runTask(host, herald, 59)
    expect(host.started.map(([mode]) => mode)).toEqual(['presence', 'presence'])
    expect(host.read('s1').pending).toBeUndefined()
  })

  test('child prompts do not reset the main timer and sibling agents stay pending', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('SubagentStart', { agent_id: 'one' }))
    await herald.hook(event('SubagentStart', { agent_id: 'two' }))
    host.clock += 70_000
    await herald.hook(event('UserPromptSubmit', { agent_id: 'one', prompt: 'Child work' }))
    await herald.hook(event('SubagentStop', { agent_id: 'one' }))
    await herald.hook(event('Stop', { last_assistant_message: 'Still running.' }))
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
    await herald.hook(event('SubagentStop', { agent_id: 'two' }))
    await herald.hook(event('Stop', { last_assistant_message: 'Finished.' }))
    const job = host.started.find(([mode]) => mode === 'summarize')
    expect(job).toBeDefined()
    await herald.summarize(job[1])
    expect(host.sent.at(-1).text).toBe('The parser now handles empty files.')
  })

  test('a returned spawn remains pending before its delayed SubagentStart', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('PreToolUse', { tool_name: 'collaborationspawn_agent', tool_use_id: 'spawn-one' }))
    await herald.hook(event('PostToolUse', { tool_name: 'collaborationspawn_agent', tool_use_id: 'spawn-one', tool_response: '{"task_name":"/root/worker"}' }))
    host.clock += 70_000
    await herald.hook(event('Stop', { last_assistant_message: 'Worker is starting.' }))
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
    await herald.hook(event('SubagentStart', { agent_id: 'child' }))
    await herald.hook(event('SubagentStop', { agent_id: 'child' }))
    await herald.hook(event('Stop', { last_assistant_message: 'Finished.' }))
    await herald.summarize(host.started.find(([mode]) => mode === 'summarize')[1])
    expect(host.sent.at(-1).text).toBe('The parser now handles empty files.')
  })

  test('a failed parallel spawn does not release the successful child', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    for (const id of ['success', 'failure']) await herald.hook(event('PreToolUse', { tool_name: 'spawn_agent', tool_use_id: id }))
    await herald.hook(event('SubagentStart', { agent_id: 'child' }))
    await herald.hook(event('PostToolUse', { tool_name: 'spawn_agent', tool_use_id: 'failure', tool_response: '{"error":"Agent limit reached"}' }))
    await herald.hook(event('PostToolUse', { tool_name: 'spawn_agent', tool_use_id: 'success', tool_response: '{"agent_id":"child"}' }))
    host.clock += 70_000
    await herald.hook(event('Stop', { last_assistant_message: 'Child is running.' }))
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
    await herald.hook(event('SubagentStop', { agent_id: 'child' }))
    await herald.hook(event('Stop', { last_assistant_message: 'Finished.' }))
    await herald.summarize(host.started.find(([mode]) => mode === 'summarize')[1])
    expect(host.sent.at(-1).text).toBe('The parser now handles empty files.')
  })

  test('background work starting during a fork cancels that summary and retains the timer', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await runTask(host, herald, 70)
    const oldJob = host.started.find(([mode]) => mode === 'summarize')[1]
    host.onFork = () => { herald.hook(event('SubagentStart', { agent_id: 'new' })) }
    await herald.summarize(oldJob)
    expect(host.sent.filter(command => command.type === 'notify')).toEqual([])
    host.onFork = undefined
    await herald.hook(event('SubagentStop', { agent_id: 'new' }))
    await herald.hook(event('Stop', { turn_id: 'final', last_assistant_message: 'Finished.' }))
    const jobs = host.started.filter(([mode]) => mode === 'summarize')
    expect(jobs).toHaveLength(2)
    await herald.summarize(jobs[1][1])
    expect(host.sent.filter(command => command.type === 'notify').map(command => command.id)).toEqual(['codex:s1:final'])
  })

  test('a new main request cannot hide surviving shell work', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'old-shell' }))
    await herald.hook(event('Interrupt'))
    await herald.hook(event('UserPromptSubmit', { prompt: 'Continue.' }))
    host.clock += 70_000
    await herald.hook(event('Stop', { last_assistant_message: 'Still running.' }))
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
    await herald.hook(event('PostToolUse', { tool_name: 'Bash', tool_use_id: 'old-shell' }))
    await herald.hook(event('Stop', { last_assistant_message: 'Finished.' }))
    await herald.summarize(host.started.find(([mode]) => mode === 'summarize')[1])
    expect(host.sent.at(-1).text).toBe('The parser now handles empty files.')
  })

  test('a final main stop recovers a completed execution whose PostToolUse was lost during interruption', async () => {
    const host = fakeHost()
    const scans = []
    host.completedExecutions = async (input, tracked) => {
      scans.push({ input, tracked: [...tracked] })
      return new Set(['exec-old'])
    }
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'exec-old' }))
    await herald.hook(event('Interrupt'))
    expect(host.read('s1').work).toEqual({ 'tool:exec-old': null })
    await herald.hook(event('UserPromptSubmit', { prompt: 'Continue.' }))
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'exec-new' }))
    host.clock += 65_000
    await herald.hook(event('PostToolUse', { tool_name: 'Bash', tool_use_id: 'exec-new' }))
    const before = host.read('s1')
    const stop = event('Stop', { last_assistant_message: 'Finished.', transcript_path: '/rollout.jsonl' })
    await herald.hook(stop)
    expect(host.started.filter(([mode]) => mode === 'summarize')).toHaveLength(1)
    expect(scans).toEqual([{ input: stop, tracked: ['exec-old'] }])
    expect(host.read('s1')).toEqual({ ...before, work: {}, pending: before.token })
    await herald.hook(stop)
    expect(host.started.filter(([mode]) => mode === 'summarize')).toHaveLength(1)
    await herald.summarize(host.started.find(([mode]) => mode === 'summarize')[1])
    expect(host.sent.filter(command => command.type === 'notify')).toHaveLength(1)
  })

  test('recovery removes only tracked terminal executions and preserves unknown work and lifecycle state', async () => {
    const host = fakeHost()
    const scans = []
    host.completedExecutions = async (_input, tracked) => {
      scans.push([...tracked])
      return new Set(['exec-finished', 'exec-untracked', 'shell', 'child'])
    }
    const before = {
      started: host.clock - 65_000, token: 'keep-token', keeper: 99, notified: 123,
      work: { 'tool:exec-finished': null, 'tool:exec-live': 'child', 'tool:shell': null, 'agent:child': 'child' },
      agents: { child: true }, spawns: { 'exec-untracked': true },
    }
    host.write('s1', before)
    await createHerald(host).hook(event('Stop', { last_assistant_message: 'Still running.' }))
    expect(host.read('s1')).toEqual({ ...before, work: { 'tool:exec-live': 'child', 'tool:shell': null, 'agent:child': 'child' } })
    expect(scans).toEqual([['exec-finished', 'exec-live']])
    expect(host.started).toEqual([])
  })

  test('interrupts and subagent stops never reconcile executions', async () => {
    const host = fakeHost()
    let scans = 0
    host.completedExecutions = async () => { scans += 1; return new Set(['exec-old']) }
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'exec-old' }))
    await herald.hook(event('SubagentStart', { agent_id: 'child' }))
    await herald.hook(event('SubagentStop', { agent_id: 'child' }))
    await herald.hook(event('Stop', { agent_id: 'child', last_assistant_message: 'Child finished.' }))
    await herald.hook(event('Interrupt'))
    expect(scans).toBe(0)
    expect(host.read('s1').work).toEqual({ 'tool:exec-old': null })
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
  })

  test('an interrupted execution without terminal evidence still blocks the next qualifying task', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'exec-unknown' }))
    await herald.hook(event('Interrupt'))
    await herald.hook(event('UserPromptSubmit'))
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'exec-new' }))
    host.clock += 65_000
    await herald.hook(event('PostToolUse', { tool_name: 'Bash', tool_use_id: 'exec-new' }))
    await herald.hook(event('Stop', { last_assistant_message: 'Finished the new task.' }))
    expect(host.read('s1').work).toEqual({ 'tool:exec-unknown': null })
    expect(host.read('s1').started).toBe(1_000_000)
    expect(host.read('s1').pending).toBeUndefined()
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
  })

  test('main stops without tracked execution IDs do not scan transcripts', async () => {
    const host = fakeHost()
    host.completedExecutions = async () => { throw new Error('Unexpected transcript scan') }
    const herald = createHerald(host)
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'shell' }))
    await runTask(host, herald, 65)
    expect(host.read('s1').work).toEqual({ 'tool:shell': null })
    expect(host.started.filter(([mode]) => mode === 'summarize')).toEqual([])
  })

  test('a new prompt discards the queued announcement and cancels the summary in flight', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await runTask(host, herald, 120)
    host.onFork = () => { herald.hook(event('UserPromptSubmit', { prompt: 'Next' })) }
    await herald.summarize(host.started[1][1])
    expect(host.sent.map(command => command.type)).toEqual(['boot', 'discard', 'read-announcement-profile', 'boot', 'discard'])
    expect(host.sent.at(-1)).toEqual({ type: 'discard', sessionID: 'codex:s1', at: 1_120_000 })
  })

  test('a stop without an assistant message stays silent', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit', { prompt: 'Fix the parser' }))
    host.clock += 90_000
    await herald.hook(event('Stop'))
    expect(host.started.map(([mode]) => mode)).toEqual(['presence'])
    expect(host.read('s1').started).toBeUndefined()
  })

  test('prompts boot the announcer and reuse the presence keeper', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit'))
    host.live.add(99)
    await herald.hook(event('UserPromptSubmit'))
    expect(host.sent.slice(0, 2)).toEqual([{ type: 'boot' }, { type: 'discard', sessionID: 'codex:s1', at: 1_000_000 }])
    expect(host.started).toEqual([['presence', { sessionID: 's1', pid: 4242 }]])
  })

  test('presence ends after a short task even while Codex stays open', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    host.write('s1', { keeper: 7, started: host.clock })
    host.onSleep = () => herald.hook(event('Stop', { last_assistant_message: 'Done.' }))
    await herald.presence({ sessionID: 's1', pid: 4242 })
    expect(host.sent.filter(command => command.type === 'presence').map(command => command.sessionIDs)).toEqual([['codex:s1'], []])
    expect(host.live.has(4242)).toBe(true)
    expect(host.read('s1').keeper).toBeUndefined()
  })

  test('presence lasts through summary generation and the announcement grace period', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    host.write('s1', { keeper: 7, pending: 'token-1' })
    host.onSleep = () => {
      if (host.clock === 1_004_000) {
        host.write('s1', { ...host.read('s1'), pending: undefined, notified: host.clock })
      }
    }
    await herald.presence({ sessionID: 's1', pid: 4242 })
    const presence = host.sent.filter(command => command.type === 'presence')
    expect(presence.at(-2).sessionIDs).toEqual(['codex:s1'])
    expect(presence.at(-1).sessionIDs).toEqual([])
    expect(presence.at(-1).at - 1_004_000).toBe(ANNOUNCEMENT_GRACE)
    expect(host.read('s1').keeper).toBeUndefined()
  })

  test('presence ends when Codex exits during a task', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    host.write('s1', { keeper: 7, started: host.clock })
    host.onSleep = () => host.live.delete(4242)
    await herald.presence({ sessionID: 's1', pid: 4242 })
    expect(host.sent.filter(command => command.type === 'presence').map(command => command.sessionIDs)).toEqual([['codex:s1'], []])
    expect(host.read('s1').keeper).toBeUndefined()
  })

  test('keeper watches the chosen Codex owner after the wrapper shell exits', async () => {
    const owner = codexAncestor(50, processes([
      { pid: 9001, parentPID: 1, name: 'Codex.exe' },
      { pid: 4242, parentPID: 9001, name: 'powershell.exe' },
      { pid: 50, parentPID: 4242, name: 'node.exe' },
    ]))
    const host = fakeHost()
    let parentReads = 0
    Object.defineProperty(host, 'parent', { get() { parentReads += 1; return owner } })
    host.live.add(owner)
    const herald = createHerald(host)
    await herald.hook(event('PreToolUse', { tool_name: 'Bash', tool_use_id: 'shell' }))
    expect(parentReads).toBe(0)
    await herald.hook(event('UserPromptSubmit'))
    expect(parentReads).toBe(1)
    expect(host.started).toEqual([['presence', { sessionID: 's1', pid: owner }]])
    host.live.add(99)
    await herald.hook(event('UserPromptSubmit'))
    expect(parentReads).toBe(1)
    host.write('s1', { keeper: 7, started: host.clock })
    let tick = 0
    host.onSleep = () => {
      tick += 1
      if (tick === 1) host.live.delete(4242)
      if (tick === 2) host.live.delete(owner)
    }
    await herald.presence({ sessionID: 's1', pid: owner })
    expect(host.sent.filter(command => command.type === 'presence').map(command => command.sessionIDs)).toEqual([['codex:s1'], ['codex:s1'], []])
    expect(host.read('s1').keeper).toBeUndefined()
  })

  test('a missing Codex owner does not start a keeper for the wrapper shell', async () => {
    const host = fakeHost()
    Object.defineProperty(host, 'parent', { get() { throw new Error('Could not find the Codex process from pid 50') } })
    const herald = createHerald(host)
    await expect(herald.hook(event('UserPromptSubmit'))).rejects.toThrow('Could not find the Codex process from pid 50')
    expect(host.started).toEqual([])
    expect(host.read('s1').keeper).toBeUndefined()
  })
})

describe('completed Codex executions', () => {
  const sessionID = '019e7ab0-1234-7890-abcd-1234567890ab'
  const header = (id = sessionID) => ({ type: 'session_meta', payload: { id } })
  const terminal = (id, status = 'completed', exit_code = 0) => ({ type: 'event_msg', payload: { type: 'item_completed', item: { type: 'CommandExecution', id, status, exit_code } } })
  const fixture = async action => {
    const directory = mkdtempSync(fileURLToPath(new URL('../../temp/codex-rollout-', import.meta.url)))
    const sessions = join(directory, 'sessions')
    const write = (name, rows, tail = '') => {
      const path = join(directory, name)
      mkdirSync(dirname(path), { recursive: true })
      writeFileSync(path, rows.map(row => typeof row === 'string' ? row : JSON.stringify(row)).join('\n') + '\n' + tail)
      return path
    }
    try { await action({ directory, sessions, write }) } finally { rmSync(directory, { recursive: true, force: true }) }
  }

  test('trusts exact completed and failed CommandExecution events in the supplied transcript first', async () => {
    await fixture(async ({ sessions, write }) => {
      const transcript_path = write('current.jsonl', [
        header(), terminal('exec-success'), terminal('exec-failed', 'failed', 17), terminal('exec-untracked'),
        terminal('exec-running', 'in_progress'),
        { type: 'event_msg', payload: { type: 'item_completed', item: { type: 'CommandExecution', id: 'exec-exit-only', exit_code: 0 } } },
        { type: 'event_msg', payload: { type: 'item_started', item: { type: 'CommandExecution', id: 'exec-started', status: 'completed', exit_code: 0 } } },
        { type: 'item_completed', item: { type: 'CommandExecution', id: 'exec-other-row', status: 'completed', exit_code: 0 } },
        { type: 'event_msg', payload: { type: 'item_completed', item: { type: 'Other', id: 'exec-other-type', status: 'completed', exit_code: 0 } } },
      ])
      write(`sessions/2026/10/18/rollout-${sessionID}.jsonl`, [header(), terminal('exec-success', 'completed'), terminal('exec-running')])
      const tracked = new Set(['exec-success', 'exec-failed', 'exec-exit-only', 'exec-started', 'exec-other-row', 'exec-other-type'])
      expect(await completedExecutions({ session_id: sessionID, transcript_path }, tracked, sessions)).toEqual(new Set(['exec-success', 'exec-failed']))
      expect(await completedExecutions({ session_id: sessionID, transcript_path }, new Set(['exec-success']), sessions)).toEqual(new Set(['exec-success']))
    })
  })

  test('finds unresolved executions across resumed chunks with dates and filename suffixes', async () => {
    await fixture(async ({ sessions, write }) => {
      const transcript_path = write(`sessions/2026/10/18/rollout-${sessionID}-latest.jsonl`, [header(), terminal('exec-latest')])
      write(`sessions/2026/10/17/rollout-${sessionID}-resumed-1.jsonl`, [header(), terminal('exec-old')])
      write(`sessions/2026/09/30/rollout-${sessionID}-resumed-2.jsonl`, [header(), terminal('exec-failed', 'failed', 1)])
      write(`sessions/2026/10/17/rollout-${sessionID}-wrong.jsonl`, [header('another-session'), terminal('exec-wrong-session')])
      write('sessions/2026/10/17/unrelated.jsonl', [header(), terminal('exec-wrong-filename')])
      write(`sessions/2026/10/17/rollout-${sessionID}.jsonl.backup`, [header(), terminal('exec-wrong-extension')])
      const tracked = new Set(['exec-latest', 'exec-old', 'exec-failed', 'exec-wrong-session', 'exec-wrong-filename', 'exec-wrong-extension'])
      expect(await completedExecutions({ session_id: sessionID, transcript_path }, tracked, sessions)).toEqual(new Set(['exec-latest', 'exec-old', 'exec-failed']))
    })
  })

  test('missing transcripts and malformed or missing session headers provide no evidence', async () => {
    await fixture(async ({ directory, sessions, write }) => {
      const tracked = new Set(['exec-old'])
      for (const transcript_path of [
        join(directory, 'missing.jsonl'),
        write('wrong.jsonl', [header('another-session'), terminal('exec-old')]),
        write('no-header.jsonl', [terminal('exec-old')]),
        write('malformed-header.jsonl', ['{bad', terminal('exec-old')]),
        write('partial-header.jsonl', [], '{"type":"session_meta"'),
      ]) {
        expect(await completedExecutions({ session_id: sessionID, transcript_path }, tracked, sessions)).toEqual(new Set())
      }
      expect(await completedExecutions({ session_id: sessionID }, tracked, sessions)).toEqual(new Set())
    })
  })

  test('falls back to matching chunks when the supplied transcript is missing or belongs to another session', async () => {
    await fixture(async ({ directory, sessions, write }) => {
      write(`sessions/2026/10/17/rollout-${sessionID}-resumed.jsonl`, [header(), terminal('exec-old')])
      for (const transcript_path of [
        join(directory, 'missing.jsonl'),
        write('wrong-session.jsonl', [header('another-session'), terminal('exec-unrelated')]),
        undefined,
      ]) {
        expect(await completedExecutions({ session_id: sessionID, transcript_path }, new Set(['exec-old', 'exec-unrelated']), sessions)).toEqual(new Set(['exec-old']))
      }
    })
  })

  test('malformed, partial, and in-progress rows do not release live work', async () => {
    await fixture(async ({ sessions, write }) => {
      const transcript_path = write('current.jsonl', [
        header(), '{bad', terminal('exec-finished'), terminal('exec-live', 'in_progress'),
        { type: 'response_item', payload: { type: 'function_call_output', call_id: 'exec-output', output: 'Process exited with code 0' } },
      ], JSON.stringify(terminal('exec-partial')))
      const host = fakeHost()
      const tracked = new Set(['exec-finished', 'exec-live', 'exec-output', 'exec-partial'])
      expect(await completedExecutions({ session_id: sessionID, transcript_path }, tracked, sessions)).toEqual(new Set(['exec-finished']))
      host.completedExecutions = (input, ids) => completedExecutions(input, ids, sessions)
      const before = { started: host.clock - 65_000, token: 'unchanged', work: Object.fromEntries([...tracked].map(id => ['tool:' + id, null])) }
      host.write(sessionID, before)
      await createHerald(host).hook(event('Stop', { session_id: sessionID, transcript_path, last_assistant_message: 'Live work remains.' }))
      expect(host.read(sessionID)).toEqual({ ...before, work: { 'tool:exec-live': null, 'tool:exec-output': null, 'tool:exec-partial': null } })
      expect(host.started).toEqual([])
    })
  })

  test('the Node hook adapter persists only proven removals against isolated rollout files', async () => {
    await fixture(async ({ directory, write }) => {
      const transcript_path = write('current.jsonl', [header(), terminal('exec-finished')])
      const before = {
        started: Date.now() - 65_000, token: 'unchanged', keeper: 123, notified: 456,
        work: { 'tool:exec-finished': null, 'tool:exec-live': null }, agents: {}, spawns: {},
      }
      const statePath = write(`data/sessions/${sessionID}.json`, [before])
      const result = spawnSync('node', [fileURLToPath(new URL('../hooks/herald.mjs', import.meta.url))], {
        env: { ...process.env, PLUGIN_ROOT: fileURLToPath(new URL('..', import.meta.url)), PLUGIN_DATA: join(directory, 'data'), CODEX_HOME: directory, HERALD_BINARY: process.execPath },
        input: JSON.stringify(event('Stop', { session_id: sessionID, transcript_path, last_assistant_message: 'Background work remains.' })),
        encoding: 'utf8', timeout: 10_000, windowsHide: true,
      })
      expect({ status: result.status, stderr: withoutSQLiteWarning(result.stderr), error: result.error }).toEqual({ status: 0, stderr: '', error: undefined })
      expect(JSON.parse(readFileSync(statePath, 'utf8'))).toEqual({ ...before, work: { 'tool:exec-live': null } })
    })
  })
})

describe('codex ancestor', () => {
  test('selects a direct native owner', () => {
    expect(codexAncestor(10, processes([{ pid: 10, parentPID: 1, name: 'codex' }]))).toBe(10)
  })

  test('selects Codex above a wrapper shell and observer node', () => {
    expect(codexAncestor(30, processes([
      { pid: 10, parentPID: 1, name: 'codex.exe' },
      { pid: 20, parentPID: 10, name: 'powershell.exe' },
      { pid: 30, parentPID: 20, name: 'node.exe' },
    ]))).toBe(10)
  })

  test('selects the nearest Codex when one Codex hosts another', () => {
    expect(codexAncestor(40, processes([
      { pid: 10, parentPID: 1, name: 'codex' },
      { pid: 20, parentPID: 10, name: 'codex.exe' },
      { pid: 30, parentPID: 20, name: 'powershell.exe' },
      { pid: 40, parentPID: 30, name: 'node.exe' },
    ]))).toBe(20)
  })

  test('does not select a similarly named executable', () => {
    expect(codexAncestor(30, processes([
      { pid: 10, parentPID: 1, name: 'codex' },
      { pid: 15, parentPID: 10, name: 'codex.cmd' },
      { pid: 20, parentPID: 15, name: 'codex-cli' },
      { pid: 30, parentPID: 20, name: 'node.exe' },
    ]))).toBe(10)
  })

  test('rejects an unrelated ancestor chain', () => {
    expect(() => codexAncestor(30, processes([
      { pid: 1, parentPID: 0, name: 'explorer.exe' },
      { pid: 20, parentPID: 1, name: 'powershell.exe' },
      { pid: 30, parentPID: 20, name: 'node.exe' },
    ]))).toThrow('Could not find the Codex process from pid 30')
  })

  test('rejects a missing ancestor', () => {
    expect(() => codexAncestor(30, processes([
      { pid: 30, parentPID: 20, name: 'node.exe' },
    ]))).toThrow('Could not find the Codex process from pid 30')
  })

  test('rejects a cyclic ancestor chain', () => {
    expect(() => codexAncestor(30, processes([
      { pid: 20, parentPID: 30, name: 'powershell.exe' },
      { pid: 30, parentPID: 20, name: 'node.exe' },
    ]))).toThrow('Could not find the Codex process from pid 30')
  })

  test('matches Codex.exe regardless of case', () => {
    expect(codexAncestor(20, processes([
      { pid: 10, parentPID: 1, name: 'Codex.EXE' },
      { pid: 20, parentPID: 10, name: 'powershell.exe' },
    ]))).toBe(10)
  })

  test('matches Codex when the record name is a path', () => {
    expect(codexAncestor(20, processes([
      { pid: 10, parentPID: 1, name: '/usr/local/bin/codex' },
      { pid: 20, parentPID: 10, name: '/opt/codex/node' },
    ]))).toBe(10)
    expect(codexAncestor(20, processes([
      { pid: 10, parentPID: 1, name: 'C:\\Program Files\\Codex\\Codex.exe' },
      { pid: 20, parentPID: 10, name: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe' },
    ]))).toBe(10)
  })
})
