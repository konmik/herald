import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const runtime = new URL('../../claude-plugin/scripts/runtime.mjs', import.meta.url).href
const root = fileURLToPath(new URL('../..', import.meta.url))

test('an externally owned Claude companion does not launch the configured executable', () => {
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `import { boot } from ${JSON.stringify(runtime)}; boot()`], {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, CIVILIZED_AGENT_EXTERNAL_COMPANION: '1', CIVILIZED_AGENT_DATA: 'isolated-proof', CIVILIZED_AGENT_BINARY: 'not-an-executable' },
  })
  assert.equal(result.status, 0, result.stderr)
})

test('an externally owned Claude companion requires separate data', () => {
  const env = { ...process.env, CIVILIZED_AGENT_EXTERNAL_COMPANION: '1' }
  delete env.CIVILIZED_AGENT_DATA
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `import { boot } from ${JSON.stringify(runtime)}; boot()`], { cwd: root, encoding: 'utf8', env })
  assert.equal(result.status, 1)
  assert.match(result.stderr, /An external companion requires CIVILIZED_AGENT_DATA/)
})
