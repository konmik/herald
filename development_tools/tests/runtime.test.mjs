import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { mkdtempSync, mkdirSync, copyFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'

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

test('a packaged Claude runtime never falls back to a sibling checkout or an executable override', () => {
  const directory = mkdtempSync(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'packaged-runtime-'))
  try {
    const scripts = join(directory, 'claude-plugin', 'scripts')
    mkdirSync(scripts, { recursive: true })
    mkdirSync(join(directory, 'native-announcer', 'bin'), { recursive: true })
    copyFileSync(fileURLToPath(new URL(runtime)), join(scripts, 'runtime.mjs'))
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `import { boot } from ${JSON.stringify(pathToFileURL(join(scripts, 'runtime.mjs')).href)}; try { boot() } catch (error) { console.error(error.message); process.exitCode = 1 }`], {
      encoding: 'utf8',
      env: { ...process.env, CIVILIZED_AGENT_EXTERNAL_COMPANION: '', CIVILIZED_AGENT_BINARY: process.execPath },
    })
    assert.equal(result.status, 1)
    assert.match(result.stderr, /Reinstall the application bundle/)
    assert.doesNotMatch(result.stderr, /npm run build:announcer/)
  } finally { rmSync(directory, { recursive: true, force: true }) }
})
