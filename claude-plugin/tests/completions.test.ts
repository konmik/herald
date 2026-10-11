import type { AgentInfo } from 'claude-code'
import { expect, mock, test as engineTest, type TestBody } from 'claude-code/testing'

const test = (name: string, body: TestBody) => engineTest(name, async ($, on) => {
  on('fs.exists', () => ({ value: true }))
  on('fs.list', () => ({ value: [{ name: 'herald-win32-x64.exe', kind: 'file', size: 1, mtimeMs: 0, isLink: false }] }))
  on('env.get', () => ({ value: undefined }))
  await body($, on)
})

const usage = { model: 'claude-opus-5', input_tokens: 100, output_tokens: 10, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
const processResult = { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
type BridgeCommand = { type: string; sessionID?: string; text?: string; sessionIDs?: string[]; title?: string; characterID?: string }
const bridgeCommand = (stdin = '') => JSON.parse(stdin || '{}') as BridgeCommand
const defaultPrompt = 'Report the outcome of the task you just finished in one explicit, concise spoken sentence. State what was done and any important failure or remaining blocker. Use plain English, no Markdown. Do not run tools. Output only that sentence.'
const taskNotification = '<task-notification>completed</task-notification>'
const taskNotificationFor = (taskID: string, status = 'completed') => `<task-notification>\n<task-id>${taskID}</task-id>\n<status>${status}</status>\n<summary>Background work finished.</summary>\n</task-notification>`
const backgroundResult = (taskID: string, endsWithFinalResponse = false) => ({
  stdout: '',
  stderr: '',
  interrupted: false,
  backgroundTaskId: taskID,
  ...(endsWithFinalResponse ? { backgroundEndsWithFinalResponse: true as const } : {}),
})
const runResult = (stdin?: string, prompt = defaultPrompt, characterID = 'herald') => {
  const command = bridgeCommand(stdin)
  return command.type === 'read-announcement-profile'
    ? { ...processResult, stdout: JSON.stringify({ prompt, characterID }) }
    : processResult
}

for (const development of [true, false]) {
  engineTest(`bridge uses ${development ? 'the portable development binary' : 'the packaged native binary'}`, async ($, on) => {
    const clock = mock.clock(on)
    let argv: readonly string[] = []
    on('session.id', () => ({ value: 'main' }))
    on('fs.exists', (_, e) => ({ value: development || (!e.path.endsWith('/.claude-plugin/development.json') && !e.path.endsWith('\\.claude-plugin\\development.json')) }))
    on('fs.list', () => ({ value: [{ name: 'herald-win32-x64.exe', kind: 'file', size: 1, mtimeMs: 0, isLink: false }] }))
    on('env.get', () => ({ value: undefined }))
    on('turn.complete', () => ({ text: '' }))
    on('agent.list', () => ({ value: [] }))
    on('model.fork', () => ({ value: { isAnswered: true, text: 'Done.', usage } }))
    on('process.run', (_, e) => { argv = e.argv; return { value: runResult(e.init?.stdin) } })
    await $.turn.complete({ turnId: 'launcher', answer: 'Done.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(argv[0]).toContain('/native-announcer/bin/herald-win32-x64.exe')
    expect(argv.slice(1, 3)).toEqual(['--bridge', '--assets'])
    expect(argv[3]).toContain('/native-announcer/resources')
  })
}

engineTest('development discovery uses the explicit binary while keeping repository assets', async ($, on) => {
  const clock = mock.clock(on)
  let argv: readonly string[] = []
  on('session.id', () => ({ value: 'override' }))
  on('fs.exists', () => ({ value: true }))
  on('env.get', (_, e) => ({ value: e.name === 'HERALD_BINARY' ? 'C:/native/target/debug/herald.exe' : undefined }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'Done.', usage } }))
  on('process.run', (_, e) => { argv = e.argv; return { value: runResult(e.init?.stdin) } })
  await $.turn.complete({ turnId: 'override', answer: 'Done.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(argv[0]).toBe('C:/native/target/debug/herald.exe')
  expect(argv.slice(1, 3)).toEqual(['--bridge', '--assets'])
  expect(argv[3]).toContain('/native-announcer/resources')
})

engineTest('installed discovery does not read the development executable override', async ($, on) => {
  const clock = mock.clock(on)
  let argv: readonly string[] = []
  on('session.id', () => ({ value: 'installed' }))
  on('fs.exists', (_, e) => ({ value: !e.path.endsWith('/.claude-plugin/development.json') && !e.path.endsWith('\\.claude-plugin\\development.json') }))
  on('fs.list', () => ({ value: [{ name: 'herald-win32-arm64.exe', kind: 'file', size: 1, mtimeMs: 0, isLink: false }] }))
  on('env.get', () => { throw new Error('Installed discovery read a development override') })
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'Done.', usage } }))
  on('process.run', (_, e) => { argv = e.argv; return { value: runResult(e.init?.stdin) } })
  await $.turn.complete({ turnId: 'installed', answer: 'Done.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(argv[0]).toContain('/native-announcer/bin/herald-win32-arm64.exe')
})

test('long main tasks fork the current conversation once', async ($, on) => {
  const clock = mock.clock(on)
  const commands: Array<{ type: string; sessionID?: string; text?: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => {
    forks++
    expect(commands).toMatchObject([{ type: 'get-ready', sessionID: 'claude:main' }])
    return { value: { isAnswered: true, text: 'The tests passed.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  const event = { turnId: 'long', answer: 'All tests passed.', durationMs: 60000, isAborted: false, reason: 'answer' as const, usage }
  await $.turn.complete(event)
  await clock.settle()
  await $.turn.complete(event)
  await clock.settle()
  expect(forks).toBe(1)
  expect(commands).toMatchObject([{ type: 'get-ready', id: 'claude:main:long' }, { type: 'notify', id: 'claude:main:long', sessionID: 'claude:main', text: 'The tests passed.', characterID: 'herald' }])
})

test('custom Claude prompts are sent unchanged and reread without task data', async ($, on) => {
  const clock = mock.clock(on)
  const prompts: string[] = []
  const commands: BridgeCommand[] = []
  let savedPrompt = 'Línea Ω 😀\nReport the outcome clearly and briefly.'
  let savedCharacter = 'herald'
  on('session.id', () => ({ value: 'custom-prompt' }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', (_, e) => {
    prompts.push(e.prompt)
    return { value: { isAnswered: true, text: 'Done.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-announcement-profile') expect(command).toEqual({ type: 'read-announcement-profile' })
    if (command.type === 'notify') commands.push(command)
    return { value: runResult(e.init?.stdin, savedPrompt, savedCharacter) }
  })
  await $.turn.complete({ turnId: 'custom-complete', answer: 'The task finished.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  savedPrompt = 'Describe the result in one sentence.'
  savedCharacter = 'robot'
  await $.turn.complete({ turnId: 'custom-failed', answer: '结果 😀', durationMs: 70000, isAborted: false, reason: 'error', usage })
  await clock.settle()
  expect(prompts).toEqual([
    'Línea Ω 😀\nReport the outcome clearly and briefly.',
    'Describe the result in one sentence.',
  ])
  expect(commands).toHaveLength(2)
  expect(commands.map(command => command.characterID)).toEqual(['herald', 'robot'])
})

test('short tasks and interrupted tasks make no summary request', async ($, on) => {
  const clock = mock.clock(on)
  let requests = 0
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => {
    requests++
    return { value: { isAnswered: true, text: 'Done.', usage } }
  })
  await $.turn.complete({ turnId: 'short', answer: 'Done.', durationMs: 59999, isAborted: false, reason: 'answer', usage })
  await $.turn.complete({ turnId: 'aborted', answer: '', durationMs: 90000, isAborted: true, reason: 'aborted' })
  await clock.settle()
  expect(requests).toBe(0)
})

test('subagents stay silent until the main task finishes', async ($, on) => {
  const clock = mock.clock(on)
  let forks = 0
  let completions = 0
  let prompt = ''
  const commands: Array<{ type: string; sessionID?: string; text?: string }> = []
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.complete', () => {
    completions++
    return { value: { isAnswered: true, text: 'Unexpected standalone completion.', usage } }
  })
  on('model.fork', (_, e) => {
    forks++
    prompt = e.prompt
    return { value: { isAnswered: true, text: 'The reviews and tests passed.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  const event = { turnId: 'review', agentId: 'child', answer: 'Reviewed the implementation.', durationMs: 70000, isAborted: false, reason: 'answer' as const }
  await $.turn.complete(event)
  await clock.settle()
  await $.turn.complete(event)
  await clock.settle()
  await $.turn.complete({ ...event, turnId: 'second-review', agentId: 'second-child' })
  await clock.settle()
  expect(forks).toBe(0)
  expect(commands).toEqual([])
  await $.turn.complete({ turnId: 'reviews-final', answer: 'The reviews and tests passed.', durationMs: 90000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(1)
  expect(completions).toBe(0)
  expect(prompt).toBe(defaultPrompt)
  expect(commands).toMatchObject([{ type: 'get-ready', sessionID: 'claude:main' }, { type: 'notify', sessionID: 'claude:main', text: 'The reviews and tests passed.' }])
})

test('a new user turn cancels a queued summary', async ($, on) => {
  const clock = mock.clock(on)
  const commands: Array<{ type: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'main' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.complete', () => ({ text: '' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => {
    forks++
    return { value: { isAnswered: true, text: 'Done.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.turn.complete({ turnId: 'old', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await $.prompt.submit({ text: 'Continue.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'new', text: 'Continue.' })
  await clock.settle()
  expect(forks).toBe(0)
  expect(commands.some((command) => command.type === 'discard')).toBe(true)
})

test('a new turn during prompt loading cancels the old fork and allows the next summary', async ($, on) => {
  const clock = mock.clock(on)
  const prompts: string[] = []
  const notifications: BridgeCommand[] = []
  let cancelRead = true
  on('session.id', () => ({ value: 'prompt-race' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.complete', () => ({ text: '' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', (_, e) => {
    prompts.push(e.prompt)
    return { value: { isAnswered: true, text: 'The new work passed.', usage } }
  })
  on('process.run', async (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-announcement-profile' && cancelRead) {
      cancelRead = false
      await $.prompt.submit({ text: 'Continue.', wait: false, origin: { kind: 'composer' } })
      await $.turn.start({ turnId: 'new-race', text: 'Continue.' })
    }
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin, 'Summarize the latest result.') }
  })
  await $.turn.complete({ turnId: 'old-race', answer: 'Old work.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(prompts).toEqual([])
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'new-race', answer: 'New work.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(prompts).toEqual(['Summarize the latest result.'])
  expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:prompt-race', text: 'The new work passed.' }])
})

test('failed model calls do not invent a notification', async ($, on) => {
  const clock = mock.clock(on)
  let sent = false
  const logs: string[] = []
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: false, reason: 'nothing-to-fork' } }))
  on('ui.log', (_, e) => { logs.push(e.text); return { value: undefined } })
  on('process.run', (_, e) => {
    sent = bridgeCommand(e.init?.stdin).type === 'notify'
    return { value: runResult(e.init?.stdin) }
  })
  await $.turn.complete({ turnId: 'failure', answer: '', durationMs: 70000, isAborted: false, reason: 'error' })
  await clock.settle()
  expect(sent).toBe(false)
  expect(logs).toEqual(['Voice summary unavailable: nothing-to-fork'])
})

test('resuming a session drops its main and child announcements', async ($, on) => {
  mock.clock(on, { now: 100000 })
  const ids: string[] = []
  on('session.id', () => ({ value: 'resumed' }))
  on('agent.list', () => ({ value: [{ id: 'child', description: 'Review', type: 'general-purpose', status: 'completed' }] }))
  on('classic.SessionStart', () => ({}))
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'discard' && command.sessionID) ids.push(command.sessionID)
    return { value: runResult(e.init?.stdin) }
  })
  await $.classic.SessionStart({ source: 'resume' })
  expect(ids).toEqual(['claude:resumed', 'claude:resumed:child'])
})

test('completion includes the session title from the current prompt', async ($, on) => {
  const clock = mock.clock(on)
  let command: unknown
  on('session.id', () => ({ value: 'title-test' }))
  on('classic.UserPromptSubmit', () => ({}))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'The tests passed.', usage } }))
  on('process.run', (_, e) => { command = bridgeCommand(e.init?.stdin); return { value: runResult(e.init?.stdin) } })
  await $.classic.UserPromptSubmit({ prompt: 'Test it.', session_title: 'Native announcer' })
  await $.turn.complete({ turnId: 'titled', answer: 'Passed.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(command).toMatchObject({ type: 'notify', title: 'Native announcer' })
})

test('session presence is renewed and removed on exit', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string; sessionIDs?: string[] }> = []
  on('session.id', () => ({ value: 'presence-test' }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.end', () => ({ sessionId: 'presence-test' }))
  on('command.register', (_, e) => ({ value: { command: e.name } }))
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.session.start({ cwd: '/test', surface: 'terminal', isInteractive: true })
  expect(commands.filter((command) => command.type === 'presence')).toMatchObject([{ sessionIDs: ['claude:presence-test'] }])
  await clock.advance(2000)
  expect(commands.filter((command) => command.type === 'presence')).toHaveLength(2)
  await $.session.end({ reason: 'prompt_input_exit', sessionId: 'presence-test', resume: { id: 'presence-test' } })
  expect(commands.at(-1)).toMatchObject({ type: 'presence', sessionIDs: [] })
  const count = commands.length
  await clock.advance(10000)
  expect(commands).toHaveLength(count)
})

test('both startup events share one session presence timer', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: BridgeCommand[] = []
  on('session.id', () => ({ value: 'startup' }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('classic.SessionStart', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('command.register', (_, e) => ({ value: { command: e.name } }))
  on('process.run', (_, e) => { commands.push(bridgeCommand(e.init?.stdin)); return { value: runResult(e.init?.stdin) } })
  await $.session.start({ cwd: '/test', surface: 'terminal', isInteractive: true })
  await clock.settle()
  await $.classic.SessionStart({ source: 'startup' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'presence')).toHaveLength(1)
  await clock.advance(2000)
  expect(commands.filter((command) => command.type === 'presence')).toHaveLength(2)
})

test('clearing a conversation renews presence for the replacement session', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  let sessionID = 'before-clear'
  const commands: Array<{ type: string; sessionIDs?: string[] }> = []
  on('session.id', () => ({ value: sessionID }))
  on('session.end', (_, e) => ({ sessionId: e.sessionId }))
  on('classic.SessionStart', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.classic.SessionStart({ source: 'startup' })
  await $.session.end({ reason: 'clear', sessionId: sessionID, resume: { id: sessionID } })
  sessionID = 'after-clear'
  await $.classic.SessionStart({ source: 'clear' })
  await clock.advance(2000)
  expect(commands.filter((command) => command.type === 'presence').map((command) => command.sessionIDs)).toEqual([
    ['claude:before-clear'], [], ['claude:after-clear'], ['claude:after-clear'],
  ])
})

for (const replaced of [false, true]) {
  test(`presence failures ${replaced ? 'from a replaced session stay silent' : 'in the current session remain visible'}`, async ($, on) => {
    const clock = mock.clock(on, { now: 100000 })
    const commands: BridgeCommand[] = []
    const logs: string[] = []
    let sessionID = 'original'
    let release = () => {}
    let started = () => {}
    const inFlight = new Promise<void>((resolve) => { started = resolve })
    const stopped = new Promise<void>((resolve) => { release = resolve })
    on('session.id', () => ({ value: sessionID }))
    on('classic.SessionStart', () => ({}))
    on('agent.list', () => ({ value: [] }))
    on('ui.log', (_, e) => { logs.push(e.text); return { value: undefined } })
    on('process.run', async (_, e) => {
      const command = bridgeCommand(e.init?.stdin)
      if (command.type === 'presence') {
        commands.push(command)
        if (commands.length === 2) {
          started()
          await stopped
          throw new Error('presence process aborted')
        }
      }
      return { value: runResult(e.init?.stdin) }
    })
    await $.classic.SessionStart({ source: 'startup' })
    const tick = clock.advance(2000)
    await inFlight
    let replacement: Promise<object> | undefined
    try {
      if (replaced) {
        sessionID = 'replacement'
        replacement = $.classic.SessionStart({ source: 'resume' })
        await clock.settle()
      }
    } finally {
      release()
    }
    await tick
    await replacement
    await clock.settle()
    expect(logs).toHaveLength(replaced ? 0 : 1)
    await clock.advance(2000)
    expect(commands.at(-1)).toMatchObject({ sessionIDs: ['claude:' + sessionID] })
  })
}

test('a new user turn cancels pending child summaries', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string; sessionID?: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'child-reset' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.complete', () => ({ text: '' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('agent.list', () => ({ value: [{ id: 'child', description: 'Review', type: 'general-purpose', status: 'completed' }] }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'Old result.', usage } } })
  on('process.run', (_, e) => { const command = bridgeCommand(e.init?.stdin); if (command.type !== 'read-announcement-profile') commands.push(command); return { value: runResult(e.init?.stdin) } })
  await $.turn.complete({ turnId: 'old-child', agentId: 'child', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await $.prompt.submit({ text: 'Continue.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'new-user', text: 'Continue.' })
  await clock.settle()
  expect(forks).toBe(0)
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  expect(commands.some((command) => command.type === 'discard' && command.sessionID === 'claude:child-reset')).toBe(true)
})

test('a composer prompt resets elapsed time before the next turn', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  let forks = 0
  const notifications: BridgeCommand[] = []
  on('session.id', () => ({ value: 'prompt-reset' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'New result.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'old', text: 'Start.' })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'old', answer: 'Old result.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await $.prompt.submit({ text: 'Continue.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'new', text: 'Continue.' })
  await clock.advance(40000)
  await $.turn.complete({ turnId: 'new', answer: 'New result.', durationMs: 40000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  expect(notifications).toEqual([])
  await $.turn.start({ turnId: 'final', text: '' })
  await clock.advance(60000)
  await $.turn.complete({ turnId: 'final', answer: 'New result.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:prompt-reset', text: 'New result.' }])
})

test('an agent becoming active while loading the profile prevents notification', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  let agents: AgentInfo[] = []
  let forks = 0
  let activateAgent = true
  const notifications: BridgeCommand[] = []
  on('session.id', () => ({ value: 'profile-agent-race' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: agents }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'Done.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-announcement-profile' && activateAgent) {
      agents = [{ id: 'review', description: 'Review', type: 'general-purpose', status: 'running' }]
    }
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'profile-race', text: 'Start.' })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'profile-race', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  expect(notifications).toEqual([])
  activateAgent = false
  agents = []
  await $.turn.start({ turnId: 'profile-final', text: '' })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'profile-final', answer: 'Done.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:profile-agent-race', text: 'Done.' }])
})

test('an agent becoming active during the summary fork prevents notification', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  let agents: AgentInfo[] = []
  let forks = 0
  const notifications: BridgeCommand[] = []
  on('session.id', () => ({ value: 'fork-agent-race' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: agents }))
  on('model.fork', () => {
    forks++
    if (forks === 1) agents = [{ id: 'review', description: 'Review', type: 'general-purpose', status: 'running' }]
    return { value: { isAnswered: true, text: 'Done.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'fork-race', text: 'Start.' })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'fork-race', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(1)
  expect(notifications).toEqual([])
  agents = []
  await $.turn.start({ turnId: 'fork-final', text: '' })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'fork-final', answer: 'Done.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:fork-agent-race', text: 'Done.' }])
})

test('a queued task notification cancels a summary before the final turn starts', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const notifications: BridgeCommand[] = []
  let deliverNotice = true
  let forks = 0
  on('session.id', () => ({ value: 'queued-task-notice' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'The final result.', usage } } })
  on('process.run', async (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-announcement-profile' && deliverNotice) {
      deliverNotice = false
      await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
    }
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.turn.complete({ turnId: 'old', answer: 'An intermediate result.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  expect(notifications).toEqual([])
  await $.turn.start({ turnId: 'final', text: taskNotification })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'final', answer: 'The final result.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:queued-task-notice', text: 'The final result.' }])
})

for (const origin of ['sdk', 'task-notification'] as const) {
  test(`a ${origin} turn cancels an older completion's initial agent query`, async ($, on) => {
    const clock = mock.clock(on, { now: 100000 })
    const notifications: BridgeCommand[] = []
    let agents: AgentInfo[] = []
    let deferQuery = true
    let releaseQuery = () => {}
    let queryStarted = () => {}
    const queryGate = new Promise<void>((resolve) => { releaseQuery = resolve })
    const queryStart = new Promise<void>((resolve) => { queryStarted = resolve })
    on('session.id', () => ({ value: 'initial-query-' + origin }))
    on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
    on('turn.start', (_, e) => ({ turnId: e.turnId }))
    on('turn.complete', () => ({ text: '' }))
    on('agent.list', async () => {
      if (deferQuery) {
        deferQuery = false
        queryStarted()
        await queryGate
        return { value: [] }
      }
      return { value: agents }
    })
    on('model.fork', () => ({ value: { isAnswered: true, text: 'The new result.', usage } }))
    on('process.run', (_, e) => {
      const command = bridgeCommand(e.init?.stdin)
      if (command.type === 'notify') notifications.push(command)
      return { value: runResult(e.init?.stdin) }
    })
    await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'sdk' } })
    await $.turn.start({ turnId: 'old', text: 'Start.' })
    await clock.advance(70000)
    const oldCompletion = $.turn.complete({ turnId: 'old', answer: 'Old result.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
    await queryStart
    await $.prompt.submit({ text: 'Continue.', wait: false, origin: { kind: origin } })
    await $.turn.start({ turnId: 'new', text: 'Continue.' })
    agents = [{ id: 'review', description: 'Review', type: 'general-purpose', status: 'running' }]
    await clock.advance(61000)
    await $.turn.complete({ turnId: 'new', answer: 'Still running.', durationMs: 61000, isAborted: false, reason: 'answer', usage })
    agents = []
    releaseQuery()
    await oldCompletion
    await clock.settle()
    expect(notifications).toEqual([])
    await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
    await $.turn.start({ turnId: 'final', text: taskNotification })
    await clock.advance(1000)
    await $.turn.complete({ turnId: 'final', answer: 'New result.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:initial-query-' + origin, text: 'The new result.' }])
  })
}

test('a stale agent query cannot delete a newer pending summary', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const notifications: BridgeCommand[] = []
  let deferAgentList = false
  let releaseAgentList = () => {}
  let resolveAgentListStarted = () => {}
  const agentListGate = new Promise<void>((resolve) => { releaseAgentList = resolve })
  const agentListStarted = new Promise<void>((resolve) => { resolveAgentListStarted = resolve })
  on('session.id', () => ({ value: 'stale-agent-query' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', async () => {
    if (deferAgentList) {
      resolveAgentListStarted()
      await agentListGate
    }
    return { value: [] }
  })
  on('model.fork', () => ({ value: { isAnswered: true, text: 'The new result.', usage } }))
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-announcement-profile') deferAgentList = true
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'old', text: 'Start.' })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'old', answer: 'Old result.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  const oldAnnouncement = clock.settle()
  await agentListStarted
  deferAgentList = false
  await $.prompt.submit({ text: 'Continue.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'new', text: 'Continue.' })
  await clock.advance(60000)
  await $.turn.complete({ turnId: 'new', answer: 'New result.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
  releaseAgentList()
  await oldAnnouncement
  await clock.settle()
  expect(notifications).toHaveLength(1)
  expect(notifications[0]).toMatchObject({ type: 'notify', sessionID: 'claude:stale-agent-query', text: 'The new result.' })
})

test('an unavailable agent list defers the summary until a later main result', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const notifications: BridgeCommand[] = []
  const logs: string[] = []
  let unavailable = true
  on('session.id', () => ({ value: 'agent-list-error' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => {
    if (unavailable) throw new Error('Agent list unavailable')
    return { value: [] }
  })
  on('ui.log', (_, e) => { logs.push(e.text); return { value: undefined } })
  on('model.fork', () => ({ value: { isAnswered: true, text: 'The final result.', usage } }))
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'notify') notifications.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.turn.complete({ turnId: 'unavailable', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(notifications).toEqual([])
  expect(logs[0]).toContain('Voice summary failed: HooksError: no implementation for agent.list')
  unavailable = false
  await $.turn.start({ turnId: 'available', text: '' })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'available', answer: 'Done.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(notifications).toMatchObject([{ type: 'notify', sessionID: 'claude:agent-list-error', text: 'The final result.' }])
})

for (const initialDuration of [30000, 70000]) {
  test(`task notifications preserve elapsed time after a ${initialDuration}ms first turn`, async ($, on) => {
    const clock = mock.clock(on, { now: 100000 })
    const notifications: Array<{ sessionID?: string; text?: string }> = []
    let forks = 0
    let agents: AgentInfo[] = [
      { id: 'review', description: 'Review', type: 'general-purpose', status: 'pending' },
    ]
    on('session.id', () => ({ value: 'live-background-agents' }))
    on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
    on('turn.start', (_, e) => ({ turnId: e.turnId }))
    on('turn.complete', () => ({ text: '' }))
    on('agent.list', () => ({ value: agents }))
    on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'The final main result.', usage } } })
    on('process.run', (_, e) => {
      const command = bridgeCommand(e.init?.stdin)
      if (command.type === 'notify') notifications.push({ sessionID: command.sessionID, text: command.text })
      return { value: runResult(e.init?.stdin) }
    })
    await $.prompt.submit({ text: 'Start the review.', wait: false, origin: { kind: 'composer' } })
    await $.turn.start({ turnId: 'launch', text: 'Start the review.' })
    await clock.advance(initialDuration)
    await $.turn.complete({ turnId: 'launch', answer: 'The review is running.', durationMs: initialDuration, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(forks).toBe(0)
    expect(notifications).toEqual([])
    if (initialDuration < 60000) await clock.advance(31000)
    agents = [
      { id: 'review', description: 'Review', type: 'general-purpose', status: 'running' },
    ]
    await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
    await $.turn.start({ turnId: 'running', text: taskNotification })
    await $.turn.complete({ turnId: 'running', answer: 'The review is still running.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(forks).toBe(0)
    expect(notifications).toEqual([])
    agents = [
      { id: 'review', description: 'Review', type: 'general-purpose', status: 'waiting' },
    ]
    await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
    await $.turn.start({ turnId: 'waiting', text: taskNotification })
    await $.turn.complete({ turnId: 'waiting', answer: 'The review is waiting.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(forks).toBe(0)
    expect(notifications).toEqual([])
    await clock.advance(65000)
    agents = [
      { id: 'reviewer@team', description: 'Teammate', type: 'teammate', teammateId: 'reviewer@team', status: 'idle' },
      { id: 'terminal', description: 'Terminal', type: 'terminal', teammateId: 'builder@team', status: 'idle' },
    ]
    await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
    await $.turn.start({ turnId: 'final', text: taskNotification })
    await clock.advance(1000)
    await $.turn.complete({ turnId: 'final', answer: 'The review finished.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(forks).toBe(1)
    expect(notifications).toEqual([{ sessionID: 'claude:live-background-agents', text: 'The final main result.' }])
  })
}

test('background shell work defers announcement and counts the whole task', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  on('session.id', () => ({ value: 'background-shell' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'The background work finished.', usage } }))
  on('process.run', (_, e) => { const command = bridgeCommand(e.init?.stdin); if (command.type !== 'read-announcement-profile') commands.push(command); return { value: runResult(e.init?.stdin) } })
  await $.prompt.submit({ text: 'Run it.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'launch', text: 'Run it.' })
  await clock.advance(70000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [{ id: 'shell', type: 'shell', status: 'running', description: 'Build' }] })
  await $.turn.complete({ turnId: 'launch', answer: 'Still running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  await clock.advance(30000)
  await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'result', text: taskNotification })
  await clock.advance(1000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [] })
  await $.turn.complete({ turnId: 'result', answer: 'Finished.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

for (const tool of ['Bash', 'PowerShell'] as const) {
  for (const initialDuration of [30000, 70000]) {
    test(`${tool} background results defer a ${initialDuration}ms main completion until its terminal notice`, async ($, on) => {
      const clock = mock.clock(on, { now: 100000 })
      const taskID = `${tool.toLowerCase()}-${initialDuration}`
      const commands: Array<{ type: string; text?: string }> = []
      let forks = 0
      on('session.id', () => ({ value: `${tool}-${initialDuration}` }))
      on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
      on('turn.start', (_, e) => ({ turnId: e.turnId }))
      on('turn.complete', () => ({ text: '' }))
      on('agent.list', () => ({ value: [] }))
      on('tool.call', { tool: 'Bash' }, () => ({ result: backgroundResult(taskID) }))
      on('tool.call', { tool: 'PowerShell' }, () => ({ result: backgroundResult(taskID) }))
      on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'The final main result.', usage } } })
      on('process.run', (_, e) => {
        const command = bridgeCommand(e.init?.stdin)
        if (command.type !== 'read-announcement-profile') commands.push(command)
        return { value: runResult(e.init?.stdin) }
      })
      await $.prompt.submit({ text: 'Start the background work.', wait: false, origin: { kind: 'composer' } })
      await $.turn.start({ turnId: 'launch', text: 'Start the background work.' })
      if (tool === 'Bash') {
        await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
      } else {
        await $.tool.call({ tool: 'PowerShell', command: 'Start-Sleep', run_in_background: true })
      }
      await clock.advance(initialDuration)
      await $.turn.complete({ turnId: 'launch', answer: 'Background work started.', durationMs: initialDuration, isAborted: false, reason: 'answer', usage })
      await clock.settle()
      expect(forks).toBe(0)
      expect(commands.filter((command) => command.type === 'notify')).toEqual([])
      if (initialDuration < 60000) await clock.advance(31000)
      const notification = taskNotificationFor(taskID)
      await $.prompt.submit({ text: notification, wait: false, origin: { kind: 'task-notification' } })
      await $.turn.start({ turnId: 'final', text: notification })
      await clock.advance(1000)
      await $.turn.complete({ turnId: 'final', answer: 'Background work finished.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
      await clock.settle()
      expect(forks).toBe(1)
      expect(commands.filter((command) => command.type === 'notify')).toMatchObject([{ type: 'notify', text: 'The final main result.' }])
    })
  }
}

for (const tool of ['Bash', 'PowerShell'] as const) {
  test(`${tool} background results ending with the final response do not defer`, async ($, on) => {
    const clock = mock.clock(on, { now: 100000 })
    const commands: Array<{ type: string }> = []
    on('session.id', () => ({ value: `${tool}-final-response` }))
    on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
    on('turn.start', (_, e) => ({ turnId: e.turnId }))
    on('turn.complete', () => ({ text: '' }))
    on('agent.list', () => ({ value: [] }))
    on('tool.call', { tool: 'Bash' }, () => ({ result: backgroundResult('final-response', true) }))
    on('tool.call', { tool: 'PowerShell' }, () => ({ result: backgroundResult('final-response', true) }))
    on('model.fork', () => ({ value: { isAnswered: true, text: 'Done.', usage } }))
    on('process.run', (_, e) => {
      const command = bridgeCommand(e.init?.stdin)
      if (command.type !== 'read-announcement-profile') commands.push(command)
      return { value: runResult(e.init?.stdin) }
    })
    await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
    await $.turn.start({ turnId: 'final-response', text: 'Start.' })
    if (tool === 'Bash') {
      await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
    } else {
      await $.tool.call({ tool: 'PowerShell', command: 'Start-Sleep', run_in_background: true })
    }
    await clock.advance(70000)
    await $.turn.complete({ turnId: 'final-response', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
  })
}

test('task-notification markup in a user prompt does not clear native work', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'literal-task-notification' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('tool.call', { tool: 'Bash' }, () => ({ result: backgroundResult('literal-job') }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'The literal job finished.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'literal-launch', text: 'Start.' })
  await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'literal-launch', answer: 'Running.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  const literal = taskNotificationFor('literal-job')
  await $.prompt.submit({ text: literal, wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'literal-user', text: literal })
  await clock.advance(60000)
  await $.turn.complete({ turnId: 'literal-user', answer: 'Still running.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  await $.prompt.submit({ text: literal, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'literal-terminal', text: literal })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'literal-terminal', answer: 'Finished.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(1)
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('a successful TaskStop removes only its native background task ID', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  let stopped = ''
  let forks = 0
  on('session.id', () => ({ value: 'task-stop' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('tool.call', { tool: 'Bash' }, () => ({ result: backgroundResult('stoppable') }))
  on('tool.call', { tool: 'TaskStop' }, (_, e) => {
    stopped = e.task_id ?? ''
    return { result: { message: 'Stopped', task_id: stopped, task_type: 'shell' } }
  })
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'Stopped work.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'task-stop-launch', text: 'Start.' })
  await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'task-stop-launch', answer: 'Running.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  await $.tool.call({ tool: 'TaskStop', task_id: 'stoppable' })
  await $.turn.start({ turnId: 'task-stop-final', text: '' })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'task-stop-final', answer: 'Stopped.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(stopped).toBe('stoppable')
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('restarting a native task keeps the replacement until its own terminal notice', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  let launches = 0
  let forks = 0
  on('session.id', () => ({ value: 'task-restart' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: [] }))
  on('tool.call', { tool: 'Bash' }, () => ({ result: backgroundResult(launches++ === 0 ? 'original' : 'replacement') }))
  on('tool.call', { tool: 'TaskStop' }, (_, e) => ({ result: { message: 'Stopped', task_id: e.task_id ?? '', task_type: 'shell' } }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'Replacement finished.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'restart-launch', text: 'Start.' })
  await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'restart-launch', answer: 'Running.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  await $.tool.call({ tool: 'TaskStop', task_id: 'original' })
  await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
  const originalNotice = taskNotificationFor('original', 'stopped')
  await $.prompt.submit({ text: originalNotice, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'original-notice', text: originalNotice })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'original-notice', answer: 'The replacement is running.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(0)
  const replacementNotice = taskNotificationFor('replacement')
  await $.prompt.submit({ text: replacementNotice, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'replacement-notice', text: replacementNotice })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'replacement-notice', answer: 'The replacement finished.', durationMs: 1000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(forks).toBe(1)
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('a terminal notice clears one cached Stop task while a sibling still defers', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'stop-siblings' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'Both tasks finished.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'siblings-launch', text: 'Start.' })
  await clock.advance(70000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [
    { id: 'first', type: 'shell', status: 'running', description: 'First task' },
    { id: 'second', type: 'shell', status: 'pending', description: 'Second task' },
  ] })
  await $.turn.complete({ turnId: 'siblings-launch', answer: 'Running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(0)
  const firstNotice = taskNotificationFor('first')
  await $.prompt.submit({ text: firstNotice, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'first-notice', text: firstNotice })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'first-notice', answer: 'One task finished.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(0)
  const secondNotice = taskNotificationFor('second')
  await $.prompt.submit({ text: secondNotice, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'second-notice', text: secondNotice })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'second-notice', answer: 'Both tasks finished.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(1)
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('known terminal task statuses clear their matching native IDs', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  const statuses = ['completed', 'failed', 'killed', 'stopped', 'cancelled'] as const
  let forks = 0
  on('session.id', () => ({ value: 'terminal-statuses' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'All tasks finished.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'statuses-launch', text: 'Start.' })
  await clock.advance(70000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: statuses.map((status) => ({ id: status, type: 'shell', status: 'running', description: status })) })
  await $.turn.complete({ turnId: 'statuses-launch', answer: 'Running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(0)
  for (const [index, status] of statuses.entries()) {
    const notification = taskNotificationFor(status, status)
    await $.prompt.submit({ text: notification, wait: false, origin: { kind: 'task-notification' } })
    await $.turn.start({ turnId: 'status-' + status, text: notification })
    await clock.advance(1000)
    await $.turn.complete({ turnId: 'status-' + status, answer: status, durationMs: 1000, isAborted: false, reason: 'answer' })
    await clock.settle()
    expect(forks).toBe(index === statuses.length - 1 ? 1 : 0)
  }
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('an empty Stop snapshot does not erase a native job result that has not notified yet', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'stop-empty-race' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('tool.call', { tool: 'Bash' }, () => ({ result: backgroundResult('late-job') }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'The late job finished.', usage } } })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.prompt.submit({ text: 'Start.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'stop-empty-launch', text: 'Start.' })
  await $.tool.call({ tool: 'Bash', command: 'sleep', run_in_background: true })
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [] })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'stop-empty-launch', answer: 'Running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(0)
  const notice = taskNotificationFor('late-job')
  await $.prompt.submit({ text: notice, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'stop-empty-final', text: notice })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'stop-empty-final', answer: 'The late job finished.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(forks).toBe(1)
})

test('monitor Stop tasks do not defer the main announcement', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  on('session.id', () => ({ value: 'monitor-stop' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'Done.', usage } }))
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-announcement-profile') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.turn.start({ turnId: 'monitor', text: 'Start.' })
  await clock.advance(70000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [{ id: 'monitor', type: 'monitor', status: 'running', description: 'Watch' }] })
  await $.turn.complete({ turnId: 'monitor', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('the main task waits for every background agent and the final reply', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string; sessionID?: string; text?: string }> = []
  let agents: AgentInfo[] = [
    { id: 'first', description: 'First review', type: 'general-purpose', status: 'running' },
    { id: 'second', description: 'Second review', type: 'general-purpose', status: 'running' },
  ]
  on('session.id', () => ({ value: 'background-agents' }))
  on('prompt.submit', (_, e) => ({ text: e.text, origin: e.origin }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('agent.list', () => ({ value: agents }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'Both reviews passed.', usage } }))
  on('process.run', (_, e) => { const command = bridgeCommand(e.init?.stdin); if (command.type !== 'read-announcement-profile') commands.push(command); return { value: runResult(e.init?.stdin) } })
  await $.prompt.submit({ text: 'Run both reviews.', wait: false, origin: { kind: 'composer' } })
  await $.turn.start({ turnId: 'launch', text: 'Run both reviews.' })
  await clock.advance(70000)
  await $.turn.complete({ turnId: 'launch', answer: 'Reviews are running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  agents = [
    { id: 'second', description: 'Second review', type: 'general-purpose', status: 'running' },
  ]
  await $.turn.complete({ turnId: 'first-result', agentId: 'first', answer: 'First review passed.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'intermediate', text: taskNotification })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'intermediate', answer: 'One review remains.', durationMs: 1000, isAborted: false, reason: 'answer' })
  agents = []
  await $.turn.complete({ turnId: 'second-result', agentId: 'second', answer: 'Second review passed.', durationMs: 80000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  await $.prompt.submit({ text: taskNotification, wait: false, origin: { kind: 'task-notification' } })
  await $.turn.start({ turnId: 'agents-final', text: taskNotification })
  await clock.advance(1000)
  await $.turn.complete({ turnId: 'agents-final', answer: 'Both reviews passed.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toMatchObject([
    { sessionID: 'claude:background-agents', text: 'Both reviews passed.' },
  ])
})
