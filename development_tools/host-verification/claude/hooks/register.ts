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
  on('turn.complete', async ($, e, next) => {
    await record($, { type: 'turn.complete', event: e })
    return next(e)
  })
  on('classic.Stop', async ($, e, next) => {
    await record($, { type: 'stop', background: e.background_tasks ?? [] })
    return next(e)
  })
  on('model.fork', async ($, e, next) => {
    const result = await next(e)
    await record($, { type: 'fork', prompt: e.prompt, result: result.value, denied: result.deny })
    return result
  })
  on('model.complete', async ($, e, next) => {
    await record($, { type: 'standalone', prompt: e.prompt })
    return next(e)
  })
  on('process.run', async ($, e, next) => {
    const result = await next(e)
    if (e.argv.some((argument) => argument.endsWith('/scripts/bridge.mjs')) && e.init?.stdin) {
      await record($, { type: 'bridge', command: JSON.parse(e.init.stdin), exitCode: result.value?.exitCode, denied: result.deny })
    }
    return result
  })
}
