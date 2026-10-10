import { describe, expect, test } from 'bun:test'
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
    expect(host.started).toEqual([['summarize', { sessionID: 's1', turnID: 't1', token: 'token-1', at: 1_061_000, model: 'gpt-6.1-sol', cwd: '/work' }]])
    await herald.summarize(host.started[0][1])
    expect(host.forks).toEqual([{ sessionID: 's1', prompt: 'Summarize.' }])
    expect(host.sent.at(-1)).toEqual({ type: 'notify', id: 'codex:s1:t1', sessionID: 'codex:s1', presenceSessionID: 'codex:s1', completed: 1_061_000, text: 'The parser now handles empty files.', title: 'Fix the parser', character: 'codex', characterID: 'royal', emotion: 'neutral' })
  })

  test('stays silent for tasks shorter than a minute', async () => {
    const host = fakeHost()
    await runTask(host, createHerald(host), 59)
    expect(host.started).toEqual([])
  })

  test('a new prompt discards the queued announcement and cancels the summary in flight', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await runTask(host, herald, 120)
    host.onFork = () => { herald.hook(event('UserPromptSubmit', { prompt: 'Next' })) }
    await herald.summarize(host.started[0][1])
    expect(host.sent.map(command => command.type)).toEqual(['discard', 'read-announcement-profile', 'discard'])
    expect(host.sent.at(-1)).toEqual({ type: 'discard', sessionID: 'codex:s1', at: 1_120_000 })
  })

  test('an interrupted turn is not announced', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('UserPromptSubmit', { prompt: 'Fix the parser' }))
    host.clock += 90_000
    await herald.hook(event('Interrupt'))
    await herald.hook(event('Stop', { last_assistant_message: 'Partial.' }))
    expect(host.started).toEqual([])
  })

  test('session start boots the announcer and starts one presence keeper', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    await herald.hook(event('SessionStart', { source: 'startup' }))
    host.live.add(99)
    await herald.hook(event('SessionStart', { source: 'resume' }))
    expect(host.sent.slice(0, 2)).toEqual([{ type: 'boot' }, { type: 'discard', sessionID: 'codex:s1', at: 1_000_000 }])
    expect(host.started).toEqual([['presence', { sessionID: 's1', pid: 4242 }]])
  })

  test('presence lasts while Codex runs and through the announcement grace period', async () => {
    const host = fakeHost()
    const herald = createHerald(host)
    host.write('s1', { keeper: 7 })
    host.onSleep = () => {
      if (host.clock === 1_004_000) {
        host.live.delete(4242)
        host.write('s1', { ...host.read('s1'), notified: host.clock })
      }
    }
    await herald.presence({ sessionID: 's1', pid: 4242 })
    const presence = host.sent.filter(command => command.type === 'presence')
    expect(presence.at(-2).sessionIDs).toEqual(['codex:s1'])
    expect(presence.at(-1).sessionIDs).toEqual([])
    expect(presence.at(-1).at - 1_004_000).toBe(ANNOUNCEMENT_GRACE)
    expect(host.read('s1').keeper).toBeUndefined()
  })
})
