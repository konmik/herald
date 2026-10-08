import type { EngineInterface, Register, Timer, TurnCompleteInput } from 'claude-code'

const pending = new Map<string, object>()
const viewed = new Map<string, number>()
const expansion = new Map<string, boolean>()
const completed = new Set<string>()
const started = new Map<string, number>()
const background = new Map<string, boolean>()
const sessions = new Set<string>()
let editTimer: Timer | undefined
let sessionTitle = ''
let transcriptPath = ''
const clientID = crypto.randomUUID()
let presenceTimer: Timer | undefined
let presenceWrites = Promise.resolve()
let presenceSequence = 0

function reportPresence($: EngineInterface, sessionIDs: string[]) {
  presenceWrites = presenceWrites.then(async () => {
    await bridge($, { type: 'presence', clientID, sessionIDs, sequence: ++presenceSequence, at: await $.clock.now() })
  }).catch(async (error) => { await $.ui.log('Voice presence failed: ' + String(error)) })
  return presenceWrites
}

async function startPresence($: EngineInterface) {
  presenceTimer?.cancel()
  const sessionID = await keyFor($)
  await reportPresence($, [sessionID])
  presenceTimer = $.clock.every(2000, async () => { await reportPresence($, [sessionID]) })
}

async function bridge($: EngineInterface, command: object) {
  const development = await $.fs.exists($.plugin.root + '/.claude-plugin/development.json')
  const executable = development ? 'node' : $.plugin.root + '/native-announcer/bin/node.exe'
  const result = await $.process.run([executable, $.plugin.root + '/scripts/bridge.mjs'], { stdin: JSON.stringify(command), timeoutMs: 10000 })
  if (result.exitCode !== 0) throw new Error('Civilized Agent bridge failed: ' + result.stderr)
  return result
}

async function announcementProfile($: EngineInterface) {
  const result = await bridge($, { type: 'read-announcement-profile' })
  const profile = JSON.parse(result.stdout) as { prompt: string; characterID?: string }
  if (typeof profile.prompt !== 'string') throw new Error('Civilized Agent bridge returned an invalid summary prompt')
  return profile
}

async function keyFor($: EngineInterface, agentId?: string) {
  const root = 'claude:' + await $.session.id()
  return agentId ? root + ':' + agentId : root
}

async function discard($: EngineInterface, agentId?: string) {
  const key = await keyFor($, agentId)
  const at = await $.clock.now()
  viewed.set(key, at)
  if (viewed.size > 2048) viewed.delete(viewed.keys().next().value!)
  pending.delete(key)
  await bridge($, { type: 'discard', sessionID: key, at })
}

async function resetTask($: EngineInterface) {
  pending.clear()
  started.clear()
  const root = await keyFor($)
  await discard($)
  for (const key of sessions) {
    if (key.startsWith(root + ':')) await discard($, key.slice(root.length + 1))
  }
}

async function announce($: EngineInterface, event: TurnCompleteInput, key: string, at: number, token: object) {
  if (pending.get(key) !== token) return
  if (background.get(key)) {
    pending.delete(key)
    return
  }
  const profile = await announcementProfile($)
  if (pending.get(key) !== token) return
  if (background.get(key)) {
    pending.delete(key)
    return
  }
  const reply = await $.model.fork({ prompt: profile.prompt })
  if (pending.get(key) !== token) return
  pending.delete(key)
  started.delete(key)
  if (!reply.isAnswered) {
    await $.ui.log('Voice summary unavailable: ' + reply.reason)
    return
  }
  const text = reply.text.replace(/\s+/g, ' ').trim()
  if (!text || (viewed.get(key) ?? -1) >= at) return
  await bridge($, { type: 'notify', id: key + ':' + event.turnId, sessionID: key, presenceSessionID: await keyFor($), completed: at, text, title: sessionTitle, transcriptPath, character: 'claude', characterID: profile.characterID, emotion: 'neutral' })
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    await bridge($, { type: 'boot' })
    await startPresence($)
    await $.command.register({ name: 'civilized-status', description: 'Check the voice adviser installation', immediate: true })
    await $.command.register({ name: 'voice-dismiss', description: 'Dismiss queued voice messages for this session or a subagent', argumentHint: '[agent-id]', immediate: true })
    return next(e)
  })

  on('classic.SessionStart', async ($, e, next) => {
    sessionTitle = e.session_title ?? ''
    transcriptPath = e.transcript_path ?? ''
    await startPresence($)
    await discard($)
    const agents = await $.agent.list()
    for (const agent of agents) await discard($, agent.id)
    return next(e)
  }).catch(async ($, e, next) => next(e))

  on('classic.UserPromptSubmit', async ($, e, next) => {
    sessionTitle = e.session_title ?? sessionTitle
    transcriptPath = e.transcript_path ?? transcriptPath
    return next(e)
  }).catch(async ($, e, next) => next(e))

  on('turn.start', async ($, e, next) => {
    const key = await keyFor($)
    if (e.text || !started.has(key)) {
      await resetTask($)
      started.set(key, await $.clock.now())
    } else {
      await discard($)
    }
    return next(e)
  })

  on('classic.Stop', async ($, e, next) => {
    background.set(await keyFor($), (e.background_tasks ?? []).some((task) => task.type !== 'monitor' && ['running', 'pending'].includes(task.status)))
    return next(e)
  }).catch(async ($, e, next) => next(e))

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    if (e.agentId != null) return result
    if (completed.has(e.turnId)) return result
    const key = await keyFor($)
    const at = await $.clock.now()
    sessions.add(key)
    if (sessions.size > 2048) sessions.delete(sessions.values().next().value!)
    if (e.isAborted) {
      started.delete(key)
      return result
    }
    if (!started.has(key)) started.set(key, at - e.durationMs)
    if (background.get(key)) return result
    if (at - started.get(key)! < 60000) {
      started.delete(key)
      return result
    }
    completed.add(e.turnId)
    if (completed.size > 2048) completed.delete(completed.values().next().value!)
    const token = {}
    pending.set(key, token)
    $.clock.after(0, async () => {
      try {
        await announce($, e, key, at, token)
      } catch (error) {
        if (pending.get(key) === token) pending.delete(key)
        await $.ui.log('Voice summary failed: ' + String(error))
      }
    })
    return result
  })

  on('prompt.edit', async ($, e, next) => {
    editTimer?.cancel()
    editTimer = $.clock.after(100, async () => { await discard($) })
    return next(e)
  })

  on('ui.render', { component: 'UserMessage' }, async ($, e, next) => {
    const previous = expansion.get(e.requestId)
    expansion.set(e.requestId, e.props.isExpanded)
    if (expansion.size > 2048) expansion.delete(expansion.keys().next().value!)
    if (previous === false && e.props.isExpanded && e.props.onScreen != null && e.props.task?.id) await discard($, e.props.task.id)
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    presenceTimer?.cancel()
    presenceTimer = undefined
    editTimer?.cancel()
    pending.clear()
    started.clear()
    background.clear()
    sessions.clear()
    await reportPresence($, [])
    if (e.reason === 'clear' || e.reason === 'resume' || e.reason === 'logout') {
      await discard($)
      const agents = await $.agent.list()
      for (const agent of agents) await discard($, agent.id)
    }
    return next(e)
  })

  on('command.run', { command: 'civilized-status' }, async () => ({ text: 'Civilized Agent is loaded; announcements start after one minute and pause during meetings.' }))
  on('command.run', { command: 'voice-dismiss' }, async ($, e) => {
    await discard($, e.args.trim() || undefined)
    return { text: 'Queued voice announcement dismissed.' }
  })
}
