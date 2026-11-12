import { expect, test as engineTest } from 'claude-code/testing'

engineTest('managed classic hook policy does not bypass Herald hooks', {
  plugins: [{
    name: 'cc-plugin-sec-default',
    tier: 'prepend',
    register(on) {
      on('classic.*', async ($, e, next) => {
        const result = await next.to(e, 'append')
        await $.process.run(['trace', JSON.stringify(next.trace.map((link) => ({ plugin: link.plugin, event: link.event, reason: link.reason })))])
        return result
      })
    },
  }],
}, async ($, on) => {
  const bypassed: Array<{ plugin: string; event: string; reason?: string }> = []
  let calls = 0
  on('classic.SessionStart', () => ({}))
  on('classic.UserPromptSubmit', () => ({}))
  on('classic.Stop', () => ({}))
  on('process.run', (_, e) => {
    calls++
    const trace = JSON.parse(e.argv[1] ?? '[]') as Array<{ plugin: string; event: string; reason?: string }>
    bypassed.push(...trace.filter((link) => link.plugin === 'herald' && link.reason != null))
    return { value: { exitCode: 0, stdout: '', stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  await $.classic.SessionStart({ source: 'startup' })
  await $.classic.UserPromptSubmit({ prompt: 'Test it.' })
  await $.classic.Stop({ stop_hook_active: false })
  expect(calls).toBe(3)
  expect(bypassed).toEqual([])
})
