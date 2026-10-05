import { expect, mock, test } from 'claude-code/testing'

const usage = { model: 'claude-opus-5', input_tokens: 100, output_tokens: 10, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 }
const processResult = { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false }

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
    commands.push(JSON.parse(e.init?.stdin ?? '{}'))
    return { value: processResult }
  })
  const event = { turnId: 'long', answer: 'All tests passed.', durationMs: 60000, isAborted: false, reason: 'answer' as const, usage }
  await $.turn.complete(event)
  await clock.settle()
  await $.turn.complete(event)
  await clock.settle()
  expect(forks).toBe(1)
  expect(commands).toMatchObject([{ type: 'notify', sessionID: 'claude:main', text: 'The tests passed.' }])
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

test('subagents summarize their own report with their own model', async ($, on) => {
  const clock = mock.clock(on)
  let model = ''
  let prompt = ''
  let command: unknown
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('model.complete', (_, e) => {
    model = e.model
    prompt = e.prompt
    return { value: { isAnswered: true, text: 'The review is complete.', usage } }
  })
  on('process.run', (_, e) => {
    command = JSON.parse(e.init?.stdin ?? '{}')
    return { value: processResult }
  })
  await $.turn.complete({ turnId: 'review', agentId: 'child', answer: 'Reviewed the implementation.', durationMs: 70000, isAborted: false, reason: 'answer', usage: { ...usage, model: 'claude-sonnet-4-6' } })
  await clock.settle()
  expect(model).toBe('claude-sonnet-4-6')
  expect(prompt).toContain('Reviewed the implementation.')
  expect(command).toMatchObject({ type: 'notify', sessionID: 'claude:main:child' })
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
    commands.push(JSON.parse(e.init?.stdin ?? '{}'))
    return { value: processResult }
  })
  await $.turn.complete({ turnId: 'old', answer: 'Done.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await $.turn.start({ turnId: 'new', text: 'Continue.' })
  await clock.settle()
  expect(forks).toBe(0)
  expect(commands).toMatchObject([{ type: 'discard' }])
})

test('failed model calls do not invent a notification', async ($, on) => {
  const clock = mock.clock(on)
  let sent = false
  const logs: string[] = []
  on('session.id', () => ({ value: 'main' }))
  on('turn.complete', () => ({ text: '' }))
  on('model.fork', () => ({ value: { isAnswered: false, reason: 'nothing-to-fork' } }))
  on('ui.log', (_, e) => { logs.push(e.text); return { value: undefined } })
  on('process.run', () => { sent = true; return { value: processResult } })
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
    ids.push(JSON.parse(e.init?.stdin ?? '{}').sessionID)
    return { value: processResult }
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
  on('process.run', (_, e) => { command = JSON.parse(e.init?.stdin ?? '{}'); return { value: processResult } })
  await $.classic.UserPromptSubmit({ prompt: 'Test it.', session_title: 'Native announcer' })
  await $.turn.complete({ turnId: 'titled', answer: 'Passed.', durationMs: 70000, isAborted: false, reason: 'answer', usage })
  await clock.settle()
  expect(command).toMatchObject({ type: 'notify', title: 'Native announcer' })
})
