import { describe, expect, test } from 'bun:test'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { ANNOUNCEMENT_GRACE, createHerald } from '../hooks/herald.mjs'

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

async function runTask(host, herald, seconds) {
  await herald.hook(event('UserPromptSubmit', { prompt: 'Fix the parser' }))
  host.clock += seconds * 1000
  await herald.hook(event('Stop', { last_assistant_message: 'Done.' }))
}

describe('codex hooks', () => {
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
})
