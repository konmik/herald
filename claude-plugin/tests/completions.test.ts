import { expect, mock, test as engineTest, type TestBody } from 'claude-code/testing'

const test = (name: string, body: TestBody) => engineTest(name, async ($, on) => {
  on('fs.exists', () => ({ value: true }))
  await body($, on)
})

const usage = { model: 'claude-opus-5', input_tokens: 100, output_tokens: 10, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
const processResult = { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
type BridgeCommand = { type: string; sessionID?: string; text?: string; sessionIDs?: string[]; title?: string }
const bridgeCommand = (stdin = '') => JSON.parse(stdin || '{}') as BridgeCommand
const defaultPrompt = 'Report the outcome of the task you just finished in one explicit, concise spoken sentence. State what was done and any important failure or remaining blocker. Use plain English, no Markdown. Do not run tools. Output only that sentence.'
const runResult = (stdin?: string, prompt = defaultPrompt) => {
  const command = bridgeCommand(stdin)
  return command.type === 'read-summary-prompt'
    ? { ...processResult, stdout: JSON.stringify(prompt) }
    : processResult
}

for (const development of [true, false]) {
  engineTest(`bridge uses ${development ? 'PATH Node in development' : 'bundled Node when installed'}`, async ($, on) => {
    const clock = mock.clock(on)
    let executable = ''
    let bridgePath = ''
    on('session.id', () => ({ value: 'main' }))
    on('fs.exists', () => ({ value: development }))
    on('turn.complete', () => ({ text: '' }))
    on('model.fork', () => ({ value: { isAnswered: true, text: 'Done.', usage } }))
    on('process.run', (_, e) => { executable = e.argv[0] ?? ''; bridgePath = e.argv[1] ?? ''; return { value: runResult(e.init?.stdin) } })
    await $.turn.complete({ turnId: 'launcher', answer: 'Done.', durationMs: 60000, isAborted: false, reason: 'answer', usage })
    await clock.settle()
    expect(executable).toBe(development ? 'node' : bridgePath.replace('/scripts/bridge.mjs', '/native-announcer/bin/node.exe'))
  })
}

test('long main tasks fork the current conversation once', async ($, on) => {
  const clock = mock.clock(on)
  const commands: Array<{ type: string; sessionID?: string; text?: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('model.fork', () => {
    forks++
    return { value: { isAnswered: true, text: 'The tests passed.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-summary-prompt') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  const event = { turnId: 'long', answer: 'All tests passed.', durationMs: 60000, isAborted: false, reason: 'answer' as const, usage }
  await $.turn.complete(event)
  await clock.settle()
  await $.turn.complete(event)
  await clock.settle()
  expect(forks).toBe(1)
  expect(commands).toMatchObject([{ type: 'notify', sessionID: 'claude:main', text: 'The tests passed.' }])
})

test('custom Claude prompts are sent unchanged and reread without task data', async ($, on) => {
  const clock = mock.clock(on)
  const prompts: string[] = []
  const commands: BridgeCommand[] = []
  let savedPrompt = 'Línea Ω 😀\nReport the outcome clearly and briefly.'
  on('session.id', () => ({ value: 'custom-prompt' }))
  on('turn.complete', () => ({ text: '' }))
  on('model.fork', (_, e) => {
    prompts.push(e.prompt)
    return { value: { isAnswered: true, text: 'Done.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-summary-prompt') expect(command).toEqual({ type: 'read-summary-prompt' })
    if (command.type === 'notify') commands.push(command)
    return { value: runResult(e.init?.stdin, savedPrompt) }
  })
  await $.turn.complete({ turnId: 'custom-complete', answer: 'The task finished.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  savedPrompt = 'Describe the result in one sentence.'
  await $.turn.complete({ turnId: 'custom-failed', answer: '结果 😀', durationMs: 70000, isAborted: false, reason: 'error', usage })
  await clock.settle()
  expect(prompts).toEqual([
    'Línea Ω 😀\nReport the outcome clearly and briefly.',
    'Describe the result in one sentence.',
  ])
  expect(commands).toHaveLength(2)
})

test('short tasks and interrupted tasks make no summary request', async ($, on) => {
  const clock = mock.clock(on)
  let requests = 0
  on('turn.complete', () => ({ text: '' }))
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
    if (command.type !== 'read-summary-prompt') commands.push(command)
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
  expect(commands).toMatchObject([{ type: 'notify', sessionID: 'claude:main', text: 'The reviews and tests passed.' }])
})

test('a new user turn cancels a queued summary', async ($, on) => {
  const clock = mock.clock(on)
  const commands: Array<{ type: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('model.fork', () => {
    forks++
    return { value: { isAnswered: true, text: 'Done.', usage } }
  })
  on('process.run', (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type !== 'read-summary-prompt') commands.push(command)
    return { value: runResult(e.init?.stdin) }
  })
  await $.turn.complete({ turnId: 'old', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await $.turn.start({ turnId: 'new', text: 'Continue.' })
  await clock.settle()
  expect(forks).toBe(0)
  expect(commands).toMatchObject([{ type: 'discard' }])
})

test('a new turn during prompt loading cancels the old fork and allows the next summary', async ($, on) => {
  const clock = mock.clock(on)
  const prompts: string[] = []
  const notifications: BridgeCommand[] = []
  let cancelRead = true
  on('session.id', () => ({ value: 'prompt-race' }))
  on('turn.complete', () => ({ text: '' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('model.fork', (_, e) => {
    prompts.push(e.prompt)
    return { value: { isAnswered: true, text: 'The new work passed.', usage } }
  })
  on('process.run', async (_, e) => {
    const command = bridgeCommand(e.init?.stdin)
    if (command.type === 'read-summary-prompt' && cancelRead) {
      cancelRead = false
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
    if (command.type !== 'read-summary-prompt') commands.push(command)
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
    if (command.type !== 'read-summary-prompt') commands.push(command)
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

test('a new user turn cancels pending child summaries', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string; sessionID?: string }> = []
  let forks = 0
  on('session.id', () => ({ value: 'child-reset' }))
  on('turn.complete', () => ({ text: '' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('agent.list', () => ({ value: [{ id: 'child', description: 'Review', type: 'general-purpose', status: 'completed' }] }))
  on('model.fork', () => { forks++; return { value: { isAnswered: true, text: 'Old result.', usage } } })
  on('process.run', (_, e) => { const command = bridgeCommand(e.init?.stdin); if (command.type !== 'read-summary-prompt') commands.push(command); return { value: runResult(e.init?.stdin) } })
  await $.turn.complete({ turnId: 'old-child', agentId: 'child', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await $.turn.start({ turnId: 'new-user', text: 'Continue.' })
  await clock.settle()
  expect(forks).toBe(0)
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  expect(commands.some((command) => command.type === 'discard' && command.sessionID === 'claude:child-reset')).toBe(true)
})

test('background shell work defers announcement and counts the whole task', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string }> = []
  on('session.id', () => ({ value: 'background-shell' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('agent.list', () => ({ value: [] }))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'The background work finished.', usage } }))
  on('process.run', (_, e) => { const command = bridgeCommand(e.init?.stdin); if (command.type !== 'read-summary-prompt') commands.push(command); return { value: runResult(e.init?.stdin) } })
  await $.turn.start({ turnId: 'launch', text: 'Run it.' })
  await clock.advance(70000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [{ id: 'shell', type: 'shell', status: 'running', description: 'Build' }] })
  await $.turn.complete({ turnId: 'launch', answer: 'Still running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  await clock.advance(30000)
  await $.turn.start({ turnId: 'result', text: '' })
  await clock.advance(1000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [] })
  await $.turn.complete({ turnId: 'result', answer: 'Finished.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toHaveLength(1)
})

test('the main task waits for every background agent and the final reply', async ($, on) => {
  const clock = mock.clock(on, { now: 100000 })
  const commands: Array<{ type: string; sessionID?: string; text?: string }> = []
  on('session.id', () => ({ value: 'background-agents' }))
  on('turn.start', (_, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))
  on('classic.Stop', () => ({}))
  on('model.fork', () => ({ value: { isAnswered: true, text: 'Both reviews passed.', usage } }))
  on('process.run', (_, e) => { const command = bridgeCommand(e.init?.stdin); if (command.type !== 'read-summary-prompt') commands.push(command); return { value: runResult(e.init?.stdin) } })
  await $.turn.start({ turnId: 'launch', text: 'Run both reviews.' })
  await clock.advance(70000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [
    { id: 'first', type: 'agent', status: 'running', description: 'First review' },
    { id: 'second', type: 'agent', status: 'running', description: 'Second review' },
  ] })
  await $.turn.complete({ turnId: 'launch', answer: 'Reviews are running.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await $.turn.complete({ turnId: 'first-result', agentId: 'first', answer: 'First review passed.', durationMs: 70000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  await $.turn.start({ turnId: 'intermediate', text: '' })
  await clock.advance(1000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [
    { id: 'second', type: 'agent', status: 'running', description: 'Second review' },
  ] })
  await $.turn.complete({ turnId: 'intermediate', answer: 'One review remains.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await $.turn.complete({ turnId: 'second-result', agentId: 'second', answer: 'Second review passed.', durationMs: 80000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toEqual([])
  await $.turn.start({ turnId: 'agents-final', text: '' })
  await clock.advance(1000)
  await $.classic.Stop({ stop_hook_active: false, background_tasks: [] })
  await $.turn.complete({ turnId: 'agents-final', answer: 'Both reviews passed.', durationMs: 1000, isAborted: false, reason: 'answer' })
  await clock.settle()
  expect(commands.filter((command) => command.type === 'notify')).toMatchObject([
    { sessionID: 'claude:background-agents', text: 'Both reviews passed.' },
  ])
})
