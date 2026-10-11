import type { EngineInterface, Register } from 'claude-code'

async function record($: EngineInterface, value: object) {
  const result = await $.process.run(['node', $.plugin.root + '/../record.mjs'], { stdin: JSON.stringify({ at: await $.clock.now(), sessionID: await $.session.id(), ...value }), timeoutMs: 10000 })
  if (result.exitCode !== 0) throw new Error(result.stderr)
}

export const register: Register = (on) => {
  on('turn.start', async ($, e, next) => {
    await record($, { type: 'turn.start', event: e })
    return next(e)
  })
  on('prompt.submit', async ($, e, next) => {
    await record($, { type: 'prompt.submit', origin: e.origin, event: e })
    return next(e)
  })
  on('turn.complete', async ($, e, next) => {
    const agents = e.agentId == null ? await $.agent.list() : undefined
    await record($, { type: 'turn.complete', event: e, ...(agents ? { agents } : {}) })
    return next(e)
  })
  on('classic.Stop', async ($, e, next) => {
    await record($, { type: 'stop', background: e.background_tasks ?? [] })
    return next(e)
  })
  on('model.fork', async ($, e, next) => {
    await record($, { type: 'fork.start', prompt: e.prompt })
    const result = await next(e)
    await record($, { type: 'fork', prompt: e.prompt, result: result.value, denied: result.deny })
    return result
  })
  on('model.complete', async ($, e, next) => {
    await record($, { type: 'standalone', prompt: e.prompt })
    return next(e)
  })
  on('process.run', async ($, e, next) => {
    const command = e.argv.includes('--bridge') && e.init?.stdin ? JSON.parse(e.init.stdin) : undefined
    if (command?.type === 'notify') await record($, { type: 'bridge.start', command })
    const result = await next(e)
    if (command) {
      await record($, { type: 'bridge', command, exitCode: result.value?.exitCode, denied: result.deny })
    }
    return result
  })
}
