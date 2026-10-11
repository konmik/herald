import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'bun:test'
import { createHerald } from '../herald.mjs'

const epoch = 1700000000000
const assistant = { role: 'assistant', stopReason: 'stop', content: [{ type: 'text', text: 'The work is finished.' }] }
const summary = { role: 'assistant', stopReason: 'stop', content: [{ type: 'text', text: '  Fixed\n the issue.  ' }] }

function deferred() {
  let resolve
  let reject
  const promise = new Promise((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

async function flush() {
  for (let index = 0; index < 50; index++) await Promise.resolve()
}

function fakeClock() {
  let time = 0
  let next = 0
  const pending = new Map()
  function schedule(callback, delay, repeat) {
    const timer = { id: ++next, unref() { timer.unrefed = true } }
    pending.set(timer, { callback, at: time + delay, repeat })
    return timer
  }
  return {
    clock: () => time,
    now: () => epoch + time,
    pending,
    timers: {
      setTimeout: (callback, delay) => schedule(callback, delay, 0),
      clearTimeout: timer => pending.delete(timer),
      setInterval: (callback, delay) => schedule(callback, delay, delay),
      clearInterval: timer => pending.delete(timer),
    },
    async advance(duration) {
      const target = time + duration
      while (true) {
        const due = [...pending].filter(([, entry]) => entry.at <= target).sort((a, b) => a[1].at - b[1].at)[0]
        if (!due) break
        const [timer, entry] = due
        time = entry.at
        if (entry.repeat) entry.at += entry.repeat
        else pending.delete(timer)
        entry.callback()
        await flush()
      }
      time = target
      await flush()
    },
  }
}

function fixture({ mode = 'rpc', env = {}, bridgeSend, complete, parentSession } = {}) {
  const clock = fakeClock()
  let serial = 0
  const commands = []
  const requests = []
  const clones = []
  const logs = []
  let leaf = 'a'
  const header = { type: 'session', version: 3, id: 'main', cwd: '/project/work', timestamp: '2026-10-12T00:00:00Z', ...(parentSession ? { parentSession } : {}) }
  const branch = [
    { type: 'message', id: 's', parentId: null, timestamp: header.timestamp, message: { role: 'system', content: 'Remember the complete context.', sections: { policy: 'Do not alter source.' }, timestamp: epoch } },
    { type: 'message', id: 'u', parentId: 's', timestamp: header.timestamp, message: { role: 'user', content: [{ type: 'text', text: 'Fix the issue.' }, { type: 'image', data: 'YWJj', mimeType: 'image/png' }], timestamp: epoch } },
    { type: 'message', id: 'a', parentId: 'u', timestamp: header.timestamp, message: structuredClone(assistant) },
  ]
  const source = {
    getHeader: () => header,
    getSessionId: () => header.id,
    getSessionFile: () => '/sessions/main.jsonl',
    getSessionName: () => 'Main work',
    getCwd: () => header.cwd,
    getLeafId: () => leaf,
    getBranch: () => branch,
    getEntries: () => { throw new Error('Do not read abandoned branches') },
  }
  const ctx = {
    mode, cwd: header.cwd, sessionManager: source,
    isIdle: () => true, hasPendingMessages: () => false,
    model: { api: 'openai-responses', id: 'actual-model', provider: 'actual-provider' },
    modelRegistry: {
      complete(model, context, options) {
        requests.push({ model, context, options })
        return complete ? complete(model, context, options, requests.length) : Promise.resolve(summary)
      },
    },
    getSystemPrompt: () => { throw new Error('Do not duplicate the system prompt') },
  }
  const runtime = createHerald({
    env, ...clock, id: () => `id-${++serial}`, log: message => logs.push(message),
    bridge: { send: async command => {
      commands.push(structuredClone(command))
      if (bridgeSend) return bridgeSend(command)
      return command.type === 'read-announcement-profile' ? { prompt: 'Announce this full conversation.', characterID: 'wizard' } : undefined
    } },
    inMemory(cwd, options, entries) {
      clones.push({ cwd, options, entries: structuredClone(entries) })
      return {
        getLeafId: () => entries.at(-1).id,
        appendMessage: message => entries.push({ type: 'message', id: 'summary-user', message }),
        buildSessionContext: () => ({ messages: entries.filter(entry => entry.type === 'message').map(entry => entry.message) }),
      }
    },
    convertToLlm: messages => messages,
  })
  return {
    runtime, ctx, source, header, branch, commands, requests, clones, logs, clock,
    setLeaf: value => { leaf = value },
    notifications: () => commands.filter(command => command.type === 'notify'),
    async open() { await runtime.open(ctx); await flush() },
    input(source = 'rpc') { runtime.input({ source }, ctx) },
    outcome(outcome = 'completed', message = assistant) {
      runtime.beforeSettle({ outcome, context: { contextMessages: [message] } }, ctx)
    },
    async finish(outcome = 'completed', message = assistant) {
      this.outcome(outcome, message)
      runtime.settled(ctx)
      await clock.advance(0)
    },
  }
}

function expectedNotification({ id = 'pi:main:id-3', completed = epoch + 60000, title = 'Main work' } = {}) {
  return {
    type: 'notify', id, sessionID: 'pi:main', presenceSessionID: 'pi:main', completed,
    title, character: 'pi', emotion: 'neutral', characterID: 'wizard', text: 'Fixed the issue.',
  }
}

test('the factory owns no process or timer before eligible session_start', async () => {
  const f = fixture()
  assert.deepEqual(f.commands, [])
  assert.equal(f.clock.pending.size, 0)
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.commands, [])
  await f.open()
  assert.deepEqual(f.commands.slice(0, 2), [
    { type: 'boot' },
    { type: 'presence', clientID: 'pi:id-1', sequence: 1, sessionIDs: ['pi:main'], at: epoch + 60000 },
  ])
})

test('59999 milliseconds is silent, while 60000 produces the exact Pi notification once', async () => {
  const short = fixture()
  await short.open()
  short.input()
  await short.clock.advance(59999)
  await short.finish()
  assert.deepEqual(short.notifications(), [])
  assert.deepEqual(short.requests, [])
  const long = fixture()
  await long.open()
  long.input()
  await long.clock.advance(60000)
  await long.finish()
  assert.deepEqual(long.notifications(), [expectedNotification()])
  assert.deepEqual(long.commands.find(command => command.type === 'get-ready'), {
    type: 'get-ready', id: 'pi:main:id-3', sessionID: 'pi:main', presenceSessionID: 'pi:main', completed: epoch + 60000,
    title: 'Main work', character: 'pi', emotion: 'neutral', characterID: 'wizard',
  })
  long.runtime.settled(long.ctx)
  await long.clock.advance(0)
  assert.deepEqual(long.notifications(), [expectedNotification()])
})

test('settlement returns before any fork request starts and snapshots the full active branch synchronously', async () => {
  const f = fixture()
  await f.open()
  f.input()
  await f.clock.advance(60000)
  const before = structuredClone(f.branch)
  f.outcome()
  assert.equal(f.runtime.settled(f.ctx), undefined)
  assert.deepEqual(f.requests, [])
  f.branch[0].message.content = 'Late mutation must not alter the snapshot.'
  await f.clock.advance(0)
  assert.deepEqual(f.clones, [{
    cwd: '/project/work', options: undefined,
    entries: [{ ...f.header, id: 'id-2', timestamp: new Date(epoch + 60000).toISOString(), parentSession: '/sessions/main.jsonl' }, ...before],
  }])
  const { model, context, options } = f.requests[0]
  assert.equal(model, f.ctx.model)
  assert.deepEqual(context, { messages: [...before.map(entry => entry.message), { role: 'user', content: 'Announce this full conversation.', timestamp: epoch + 60000 }] })
  assert.deepEqual(Object.keys(options), ['signal'])
  assert.equal(options.signal.aborted, false)
  assert.deepEqual(f.notifications(), [expectedNotification()])
})

test('retry and intermediate agent activity preserve timing and only final settled announces', async () => {
  const f = fixture()
  await f.open()
  f.input()
  f.runtime.beforeStart(f.ctx)
  await f.clock.advance(40000)
  f.outcome('error')
  f.runtime.agentStart(f.ctx)
  f.runtime.beforeStart(f.ctx)
  await f.clock.advance(20000)
  assert.doesNotMatch(readFileSync(new URL('../index.ts', import.meta.url), 'utf8'), /pi\.on\(['"]agent_end['"]/)
  assert.deepEqual(f.notifications(), [])
  f.ctx.hasPendingMessages = () => true
  f.outcome()
  f.runtime.settled(f.ctx)
  await f.clock.advance(0)
  assert.deepEqual(f.requests, [])
  f.ctx.hasPendingMessages = () => false
  f.ctx.isIdle = () => false
  f.runtime.settled(f.ctx)
  await f.clock.advance(0)
  assert.deepEqual(f.requests, [])
  f.ctx.isIdle = () => true
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification()])
})

test('an aborted run before queued delivery does not retime the already-submitted next task', async () => {
  const f = fixture()
  await f.open()
  f.input()
  await f.clock.advance(50000)
  f.input()
  await f.clock.advance(10000)
  f.outcome('aborted')
  f.ctx.hasPendingMessages = () => true
  f.runtime.settled(f.ctx)
  await f.clock.advance(20000)
  assert.deepEqual(f.notifications(), [])
  f.runtime.beforeStart(f.ctx)
  f.runtime.agentStart(f.ctx)
  f.runtime.messageStart({ message: { role: 'user' } }, f.ctx)
  await f.clock.advance(30000)
  f.ctx.hasPendingMessages = () => false
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ completed: epoch + 110000 })])
})

test('queued human input resets once at submission, not at before_agent_start or user delivery', async () => {
  const f = fixture()
  await f.open()
  f.input('interactive')
  await f.clock.advance(50000)
  f.input('rpc')
  await f.clock.advance(30000)
  f.runtime.beforeStart(f.ctx)
  f.runtime.agentStart(f.ctx)
  f.runtime.messageStart({ message: { role: 'user', content: 'Queued follow up.' } }, f.ctx)
  await f.clock.advance(30000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ completed: epoch + 110000 })])
  const short = fixture()
  await short.open()
  short.input()
  await short.clock.advance(60000)
  short.input()
  await short.clock.advance(59999)
  await short.finish()
  assert.deepEqual(short.notifications(), [])
})

test('extension continuations do not reset an existing task clock', async () => {
  const f = fixture()
  await f.open()
  f.runtime.beforeStart(f.ctx)
  await f.clock.advance(45000)
  f.input('extension')
  f.runtime.messageStart({ message: { role: 'user' } }, f.ctx)
  f.runtime.beforeStart(f.ctx)
  await f.clock.advance(15000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification()])
})

test('abort is silent even with an earlier tool-call assistant, then a fresh human task can announce', async () => {
  const f = fixture()
  await f.open()
  f.input()
  await f.clock.advance(60000)
  f.outcome('completed', { ...assistant, stopReason: 'toolUse', content: [{ type: 'toolCall', id: 'call', name: 'bash', arguments: {} }] })
  await f.finish('aborted', assistant)
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.requests, [])
  assert.deepEqual(f.commands.at(-1), { type: 'discard', sessionID: 'pi:main', at: epoch + 60000 })
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ completed: epoch + 120000 })])
})

test('missing final outcome, assistant, text, or completed tool turn stays silent; meaningful error can announce', async () => {
  for (const message of [undefined, { role: 'user', content: 'User text' }, { ...assistant, content: [] }, { ...assistant, stopReason: 'toolUse' }]) {
    const f = fixture()
    await f.open()
    f.input()
    await f.clock.advance(60000)
    f.runtime.beforeSettle({ outcome: 'completed', context: { contextMessages: message ? [message] : [] } }, f.ctx)
    f.runtime.settled(f.ctx)
    await f.clock.advance(0)
    assert.deepEqual(f.notifications(), [])
  }
  const noOutcome = fixture()
  await noOutcome.open()
  noOutcome.input()
  await noOutcome.clock.advance(60000)
  noOutcome.runtime.settled(noOutcome.ctx)
  await noOutcome.clock.advance(0)
  assert.deepEqual(noOutcome.requests, [])
  const error = fixture()
  await error.open()
  error.input()
  await error.clock.advance(60000)
  await error.finish('error', { ...assistant, content: [{ type: 'text', text: 'The build failed with a useful error.' }] })
  assert.deepEqual(error.notifications(), [expectedNotification()])
})

test('new input during profile lookup invalidates the old snapshot without losing a newer task', async () => {
  const profile = deferred()
  let reads = 0
  const f = fixture({ bridgeSend: command => command.type === 'read-announcement-profile'
    ? ++reads === 1 ? profile.promise : { prompt: 'Announce this full conversation.', characterID: 'wizard' }
    : undefined })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  f.input()
  await flush()
  profile.resolve({ prompt: 'Obsolete profile', characterID: 'obsolete' })
  await flush()
  assert.deepEqual(f.requests, [])
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  assert.deepEqual(f.logs, [])
})

test('a cancelled model result cannot clear or notify for a newer running task', async () => {
  const model = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? model.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  const signal = f.requests[0].options.signal
  f.input()
  await flush()
  assert.equal(signal.aborted, true)
  await f.clock.advance(30000)
  model.resolve({ ...summary, content: [{ type: 'text', text: 'Obsolete result' }] })
  await flush()
  assert.deepEqual(f.notifications(), [])
  await f.clock.advance(30000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  assert.deepEqual(f.logs, [])
})

test('cancellation discard follows a notify whose native write already started', async () => {
  const notify = deferred()
  const f = fixture({ bridgeSend: command => {
    if (command.type === 'notify') return notify.promise
    if (command.type === 'read-announcement-profile') return { prompt: 'Announce this full conversation.', characterID: 'wizard' }
  } })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification()])
  const position = f.commands.length
  f.input()
  await flush()
  assert.equal(f.commands.length, position)
  notify.resolve()
  await flush()
  assert.deepEqual(f.commands.slice(position), [{ type: 'discard', sessionID: 'pi:main', at: epoch + 60000 }])
  await f.clock.advance(60000)
  await f.finish()
  notify.resolve()
  await flush()
  assert.deepEqual(f.notifications(), [expectedNotification(), expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
})

test('queued notify checks its token again before starting the native write', async () => {
  const model = deferred()
  const presence = deferred()
  let held = false
  const f = fixture({
    complete: (_model, _context, _options, count) => count === 1 ? model.promise : Promise.resolve(summary),
    bridgeSend: command => {
      if (command.type === 'read-announcement-profile') return { prompt: 'Announce this full conversation.', characterID: 'wizard' }
      if (command.type === 'presence' && command.at === epoch + 62000 && !held) {
        held = true
        return presence.promise
      }
    },
  })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  await f.clock.advance(2000)
  model.resolve(summary)
  await flush()
  f.input()
  await flush()
  presence.resolve()
  await flush()
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.commands.at(-1), { type: 'discard', sessionID: 'pi:main', at: epoch + 62000 })
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 122000 })])
})

test('automatic activity cancels an old summary and resumes its original clock', async () => {
  const model = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? model.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  await f.clock.advance(1000)
  f.runtime.agentStart(f.ctx)
  await flush()
  model.reject(new Error('Old cancelled provider error'))
  await flush()
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 61000 })])
  assert.deepEqual(f.logs, [])
})

test('a late user message invalidates an old summary without a duplicate settlement', async () => {
  const gate = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  f.runtime.settled(f.ctx)
  await f.clock.advance(0)
  assert.equal(f.requests.length, 1)
  f.runtime.messageStart({ message: { role: 'user' } }, f.ctx)
  gate.resolve(summary)
  await flush()
  assert.deepEqual(f.notifications(), [])
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
})

test('branch navigation and silent leaf changes drop old work and a fresh branch task still announces', async () => {
  for (const explicit of [true, false]) {
    const gate = deferred()
    const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
    await f.open()
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    f.setLeaf('different-branch')
    if (explicit) f.runtime.navigate(f.ctx)
    gate.resolve(summary)
    await flush()
    assert.deepEqual(f.notifications(), [])
    assert.deepEqual(f.logs, [])
    f.setLeaf('a')
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  }
})

test('source leaf is checked during profile and get-ready boundaries before requesting the model', async () => {
  for (const boundary of ['read-announcement-profile', 'get-ready']) {
    const gate = deferred()
    let held = false
    const f = fixture({ bridgeSend: command => {
      if (command.type === boundary && !held) {
        held = true
        return gate.promise
      }
      if (command.type === 'read-announcement-profile') return { prompt: 'Announce this full conversation.', characterID: 'wizard' }
    } })
    await f.open()
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    f.setLeaf('other')
    gate.resolve(boundary === 'read-announcement-profile' ? { prompt: 'Obsolete profile' } : undefined)
    await flush()
    assert.deepEqual(f.requests, [])
    assert.deepEqual(f.notifications(), [])
    f.setLeaf('a')
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  }
})

test('a source manager changing session ID cannot notify even when the selected leaf is unchanged', async () => {
  const gate = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  f.header.id = 'replacement'
  gate.resolve(summary)
  await flush()
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.logs, [])
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [{
    ...expectedNotification({ id: 'pi:replacement:id-5', completed: epoch + 120000 }),
    sessionID: 'pi:replacement', presenceSessionID: 'pi:replacement',
  }])
})

test('an abort arriving during detached summarization cancels and discards the current job', async () => {
  const gate = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  f.outcome('aborted')
  gate.resolve(summary)
  await flush()
  assert.equal(f.requests[0].options.signal.aborted, true)
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.commands.at(-1), { type: 'discard', sessionID: 'pi:main', at: epoch + 60000 })
  assert.deepEqual(f.logs, [])
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
})

test('shutdown before the detached job starts cancels its timer and never reads a profile', async () => {
  const f = fixture()
  await f.open()
  f.input()
  await f.clock.advance(60000)
  f.outcome()
  f.runtime.settled(f.ctx)
  await f.runtime.close()
  assert.equal(f.clock.pending.size, 0)
  await f.clock.advance(0)
  assert.deepEqual(f.requests, [])
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.commands.filter(command => command.type === 'read-announcement-profile'), [])
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
})

test('a changed leaf before the detached job starts is discarded and does not leave a stuck owner', async () => {
  const f = fixture()
  await f.open()
  f.input()
  await f.clock.advance(60000)
  f.outcome()
  f.runtime.settled(f.ctx)
  f.setLeaf('changed-before-profile')
  await f.clock.advance(0)
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.requests, [])
  assert.deepEqual(f.commands.at(-1), { type: 'discard', sessionID: 'pi:main', at: epoch + 60000 })
  f.setLeaf('a')
  f.runtime.beforeStart(f.ctx)
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
})

test('summary timeout is bounded at 60 seconds even when the provider ignores abort', async () => {
  const gate = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  await f.clock.advance(59999)
  assert.deepEqual(f.logs, [])
  await f.clock.advance(1)
  assert.equal(f.requests[0].options.signal.aborted, true)
  assert.deepEqual(f.logs, ['Pi announcement timed out after 60 seconds'])
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.commands.at(-1), { type: 'discard', sessionID: 'pi:main', at: epoch + 120000 })
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 180000 })])
})

test('empty, error, aborted, tool-call, and rejected model replies never invent a notification', async () => {
  const invalid = [
    { ...summary, content: [] },
    { ...summary, stopReason: 'error' },
    { ...summary, stopReason: 'aborted' },
    { ...summary, stopReason: 'toolUse' },
    { ...summary, content: [{ type: 'toolCall', name: 'bash', arguments: {} }, { type: 'text', text: 'Ignore this tool request' }] },
    new Error('Provider refused the request'),
  ]
  for (const reply of invalid) {
    const f = fixture({ complete: (_model, _context, _options, count) => count === 1
      ? reply instanceof Error ? Promise.reject(reply) : Promise.resolve(reply)
      : Promise.resolve(summary) })
    await f.open()
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.notifications(), [])
    assert.equal(f.logs.length, 1)
    assert.deepEqual(f.commands.at(-1), { type: 'discard', sessionID: 'pi:main', at: epoch + 60000 })
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  }
})

test('virtual and missing models fail visibly with no alternate request, physical models work', async () => {
  for (const model of [undefined, { api: 'pi-virtual' }]) {
    const f = fixture()
    await f.open()
    f.ctx.model = model
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.requests, [])
    assert.deepEqual(f.notifications(), [])
    assert.deepEqual(f.logs, [model ? 'Pi virtual models are unsupported for Herald announcements' : 'Pi announcement has no selected model'])
    f.ctx.model = { api: 'openai-responses' }
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  }
})

test('print, json, explicit subagents, and inherited parent IDs are silent, but ordinary root forks work', async () => {
  for (const options of [{ mode: 'print' }, { mode: 'json' }, { env: { HERALD_PI_SUBAGENT: '1' } }, { env: { PI_SESSION_ID: 'parent' } }]) {
    const f = fixture(options)
    await f.open()
    f.input()
    f.runtime.beforeStart(f.ctx)
    await f.clock.advance(60000)
    await f.finish()
    await f.runtime.close()
    assert.deepEqual(f.commands, [])
    assert.equal(f.clock.pending.size, 0)
    assert.deepEqual(f.requests, [])
  }
  for (const mode of ['tui', 'rpc']) {
    const f = fixture({ mode, parentSession: '/normal-user-fork.jsonl', env: { PI_SESSION_ID: 'main' } })
    await f.open()
    f.input()
    await f.clock.advance(60000)
    await f.finish()
    assert.deepEqual(f.notifications(), [expectedNotification()])
  }
})

test('presence uses one stable client, increasing sequence, unrefed 2000ms timer and empty withdrawal', async () => {
  const f = fixture()
  await f.open()
  await f.runtime.open(f.ctx)
  assert.equal(f.clock.pending.size, 1)
  assert.equal([...f.clock.pending.keys()][0].unrefed, true)
  await f.clock.advance(1999)
  assert.equal(f.commands.filter(command => command.type === 'presence').length, 1)
  await f.clock.advance(1)
  await f.clock.advance(2000)
  await f.runtime.close()
  await f.runtime.close()
  assert.deepEqual(f.commands.filter(command => command.type === 'presence'), [
    { type: 'presence', clientID: 'pi:id-1', sequence: 1, sessionIDs: ['pi:main'], at: epoch },
    { type: 'presence', clientID: 'pi:id-1', sequence: 2, sessionIDs: ['pi:main'], at: epoch + 2000 },
    { type: 'presence', clientID: 'pi:id-1', sequence: 3, sessionIDs: ['pi:main'], at: epoch + 4000 },
    { type: 'presence', clientID: 'pi:id-1', sequence: 4, sessionIDs: [], at: epoch + 4000 },
  ])
  assert.equal(f.clock.pending.size, 0)
  await f.clock.advance(8000)
  assert.equal(f.commands.filter(command => command.type === 'presence').length, 4)
})

test('shutdown aborts summary, withdraws presence, and reopening can announce a new task', async () => {
  const gate = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  await f.runtime.close()
  assert.equal(f.requests[0].options.signal.aborted, true)
  assert.deepEqual(f.commands.at(-1), { type: 'presence', clientID: 'pi:id-1', sequence: 32, sessionIDs: [], at: epoch + 60000 })
  gate.resolve(summary)
  await flush()
  assert.deepEqual(f.notifications(), [])
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  assert.deepEqual(f.notifications(), [expectedNotification({ id: 'pi:main:id-5', completed: epoch + 120000 })])
  assert.deepEqual(f.logs, [])
})

test('session replacement drops old work and reports only the new presence key', async () => {
  const gate = deferred()
  const f = fixture({ complete: (_model, _context, _options, count) => count === 1 ? gate.promise : Promise.resolve(summary) })
  await f.open()
  f.input()
  await f.clock.advance(60000)
  await f.finish()
  const newHeader = { ...f.header, id: 'next', parentSession: '/sessions/main.jsonl' }
  const newSource = { ...f.source, getSessionId: () => 'next', getHeader: () => newHeader, getSessionName: () => undefined }
  const next = { ...f.ctx, sessionManager: newSource }
  await f.runtime.open(next)
  gate.resolve(summary)
  await flush()
  assert.deepEqual(f.notifications(), [])
  assert.deepEqual(f.commands.filter(command => command.type === 'presence').slice(-2).map(command => command.sessionIDs), [[], ['pi:next']])
  f.runtime.input({ source: 'rpc' }, next)
  await f.clock.advance(60000)
  f.runtime.beforeSettle({ outcome: 'completed', context: { contextMessages: [assistant] } }, next)
  f.runtime.settled(next)
  await f.clock.advance(0)
  assert.deepEqual(f.notifications(), [{ ...expectedNotification({ id: 'pi:next:id-5', completed: epoch + 120000, title: 'work' }), sessionID: 'pi:next', presenceSessionID: 'pi:next' }])
})
