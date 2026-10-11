import { randomUUID } from 'node:crypto'
import { basename } from 'node:path'
import { createBridge } from './bridge.mjs'
import { announcementText, forkMessages, snapshotBranch } from './fork.mjs'

export function createHerald({
  inMemory,
  convertToLlm,
  env = process.env,
  bridge = createBridge({ env }),
  clock = () => performance.now(),
  now = Date.now,
  id = randomUUID,
  timers = { setTimeout, clearTimeout, setInterval, clearInterval },
  log = message => console.error(`[herald/pi] ${message}`),
}) {
  let task = { kind: 'idle', generation: 0 }
  let generation = 0
  let owner
  let presenceTimer
  let presencePending = false
  let sequence = 0
  const clientID = `pi:${id()}`
  let writes = Promise.resolve()

  function send(command, current = () => true) {
    const result = writes.then(() => current() ? bridge.send(command) : undefined)
    writes = result.catch(() => {})
    return result
  }

  function owns(ctx) {
    return owner?.id === ctx.sessionManager.getSessionId()
  }

  function invalidate(kind = 'idle', started = clock()) {
    if (task.kind === 'summarizing') {
      task.controller.abort()
      timers.clearTimeout(task.scheduled)
    }
    task = kind === 'running'
      ? { kind, generation: ++generation, started, outcome: undefined, assistant: undefined }
      : { kind, generation: ++generation }
    if (!owner) return
    const session = owner
    const token = task.generation
    void send({ type: 'discard', sessionID: session.key, at: now() }).catch(error => {
      if (owner === session && task.generation === token) log(error.message)
    })
  }

  function heartbeat(session) {
    if (presencePending || owner !== session) return
    presencePending = true
    void send({ type: 'presence', clientID, sequence: ++sequence, sessionIDs: [session.key], at: now() }, () => owner === session)
      .catch(error => { if (owner === session) log(error.message) })
      .finally(() => { presencePending = false })
  }

  function close() {
    if (!owner) return writes
    invalidate()
    owner = undefined
    timers.clearInterval(presenceTimer)
    presenceTimer = undefined
    return send({ type: 'presence', clientID, sequence: ++sequence, sessionIDs: [], at: now() })
      .catch(error => { if (!owner) log(error.message) })
  }

  function open(ctx) {
    if (owns(ctx)) return writes
    void close()
    const sessionID = ctx.sessionManager.getSessionId()
    if (!['tui', 'rpc'].includes(ctx.mode) || env.HERALD_PI_SUBAGENT === '1' ||
      (env.PI_SESSION_ID && env.PI_SESSION_ID !== sessionID)) return writes
    const session = { id: sessionID, key: `pi:${sessionID}`, source: ctx.sessionManager }
    owner = session
    task = { kind: 'idle', generation: ++generation }
    void send({ type: 'boot' }, () => owner === session).catch(error => { if (owner === session) log(error.message) })
    heartbeat(session)
    presenceTimer = timers.setInterval(() => heartbeat(session), 2000)
    presenceTimer.unref?.()
    return writes
  }

  function input(event, ctx) {
    if (owns(ctx) && ['interactive', 'rpc'].includes(event.source)) invalidate('running')
  }

  function beforeStart(ctx) {
    if (owns(ctx) && task.kind === 'idle') invalidate('running')
  }

  function agentStart(ctx) {
    if (!owns(ctx)) return
    if (task.kind === 'summarizing') invalidate('running', task.started)
    if (task.kind === 'running') {
      task.outcome = undefined
      task.assistant = undefined
    }
  }

  function messageStart(event, ctx) {
    if (!owns(ctx) || event.message.role !== 'user') return
    if (task.kind === 'summarizing' || task.kind === 'idle') invalidate('running')
    if (task.kind === 'running') {
      task.outcome = undefined
      task.assistant = undefined
    }
  }

  function beforeSettle(event, ctx) {
    if (!owns(ctx)) return
    if (event.outcome === 'aborted') {
      if (task.kind === 'running') {
        invalidate('running', task.started)
        task.outcome = 'aborted'
      } else {
        invalidate()
      }
      return
    }
    if (task.kind !== 'running') return
    task.outcome = event.outcome
    task.assistant = event.context.contextMessages.findLast(message => message.role === 'assistant')
  }

  function settled(ctx) {
    if (!owns(ctx) || task.kind !== 'running' || !ctx.isIdle() || ctx.hasPendingMessages()) return
    const running = task
    if (clock() - running.started < 60000 || !['completed', 'error'].includes(running.outcome) ||
      !running.assistant?.content?.some(part => part.type === 'text' && part.text.trim()) ||
      ['aborted', 'toolUse'].includes(running.assistant.stopReason)) {
      task = { kind: 'idle', generation: running.generation }
      return
    }
    const completed = now()
    let snapshot
    try {
      snapshot = snapshotBranch(ctx.sessionManager, completed, id)
    } catch (error) {
      log(error.message)
      task = { kind: 'idle', generation: running.generation }
      return
    }
    const job = {
      kind: 'summarizing', generation: running.generation, started: running.started,
      completed, snapshot, session: owner, controller: new AbortController(),
      model: ctx.model, registry: ctx.modelRegistry,
      notification: {
        id: `${owner.key}:${id()}`, sessionID: owner.key, presenceSessionID: owner.key,
        completed, title: ctx.sessionManager.getSessionName() || basename(ctx.cwd),
        character: 'pi', emotion: 'neutral',
      },
    }
    task = job
    job.scheduled = timers.setTimeout(() => {
      job.scheduled = undefined
      void summarize(job).catch(error => { if (current(job)) log(error.message) })
    }, 0)
    job.scheduled.unref?.()
  }

  function current(job) {
    return task === job && owner === job.session && job.session.source.getSessionId() === job.session.id &&
      job.session.source.getLeafId() === job.snapshot.leaf
  }

  async function summarize(job) {
    if (!current(job) || job.controller.signal.aborted) {
      if (task === job) {
        void send({ type: 'discard', sessionID: job.session.key, at: now() }).catch(() => {})
        task = { kind: 'idle', generation: job.generation }
      }
      return
    }
    const timeout = timers.setTimeout(() => job.controller.abort(new Error('Pi announcement timed out after 60 seconds')), 60000)
    timeout.unref?.()
    let delivered = false
    const valid = () => current(job) && !job.controller.signal.aborted
    const signal = job.controller.signal
    let abortListener
    const cancelled = new Promise((_, reject) => {
      abortListener = () => reject(signal.reason)
      signal.addEventListener('abort', abortListener, { once: true })
    })
    try {
      if (!job.model) throw new Error('Pi announcement has no selected model')
      if (job.model.api === 'pi-virtual') throw new Error('Pi virtual models are unsupported for Herald announcements')
      const profile = await Promise.race([send({ type: 'read-announcement-profile' }, valid), cancelled])
      if (!valid()) return
      const messages = forkMessages(job.snapshot, profile?.prompt, job.completed, inMemory, convertToLlm)
      const notification = { ...job.notification, ...(profile.characterID ? { characterID: profile.characterID } : {}) }
      await Promise.race([send({ type: 'get-ready', ...notification }, valid), cancelled])
      if (!valid()) return
      const reply = await Promise.race([job.registry.complete(job.model, { messages }, { signal }), cancelled])
      if (!valid()) return
      const text = announcementText(reply)
      await Promise.race([send({ type: 'notify', ...notification, text }, valid), cancelled])
      if (!valid()) return
      delivered = true
    } catch (error) {
      if (current(job)) log(error?.message || String(error))
    } finally {
      timers.clearTimeout(timeout)
      signal.removeEventListener('abort', abortListener)
      if (task === job) {
        if (!delivered && owner === job.session) {
          void send({ type: 'discard', sessionID: job.session.key, at: now() }).catch(error => {
            if (owner === job.session && task.generation === job.generation) log(error.message)
          })
        }
        task = { kind: 'idle', generation: job.generation }
      }
    }
  }

  function navigate(ctx) {
    if (owns(ctx)) invalidate()
  }

  return { open, close, input, beforeStart, agentStart, messageStart, beforeSettle, settled, navigate }
}
