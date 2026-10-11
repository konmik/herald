import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { test } from 'bun:test'
import { createBridge, resolveRuntime } from '../bridge.mjs'

const tempRoot = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir()

function directory(installed = false) {
  const root = mkdtempSync(join(tempRoot, 'herald-pi-bridge-'))
  mkdirSync(join(root, 'native-announcer/bin'), { recursive: true })
  mkdirSync(join(root, 'native-announcer/resources'), { recursive: true })
  const binary = join(root, 'native-announcer/bin', installed ? `herald-${process.platform}-${process.arch}${process.platform === 'win32' ? '.exe' : ''}` : process.platform === 'win32' ? 'herald.exe' : 'herald')
  writeFileSync(binary, '')
  return { root, binary, assets: join(root, 'native-announcer/resources') }
}

test('installed bundle uses only its portable root binary and resources despite overrides', () => {
  const f = directory(true)
  try {
    writeFileSync(join(f.root, 'bundle-manifest.json'), '{}')
    assert.deepEqual(resolveRuntime(f.root, { HERALD_BINARY: '/untrusted/herald', HERALD_ASSETS: '/untrusted/resources' }), { binary: f.binary, assets: f.assets })
    rmSync(f.binary)
    assert.throws(() => resolveRuntime(f.root, { HERALD_BINARY: process.execPath }), /Herald binary not found/)
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})

test('checkout accepts explicit verification overrides and otherwise uses the root native binary', () => {
  const f = directory()
  try {
    assert.deepEqual(resolveRuntime(f.root, {}), { binary: f.binary, assets: f.assets })
    assert.deepEqual(resolveRuntime(f.root, { HERALD_BINARY: process.execPath, HERALD_ASSETS: f.root }), { binary: resolve(process.execPath), assets: resolve(f.root) })
    assert.throws(() => resolveRuntime(f.root, { HERALD_BINARY: join(f.root, 'missing') }), /Herald binary not found/)
    assert.throws(() => resolveRuntime(f.root, { HERALD_ASSETS: join(f.root, 'missing') }), /Herald assets not found/)
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})

test('external companion always requires an explicit data directory', () => {
  const f = directory()
  try {
    for (const HERALD_DATA of [undefined, '']) assert.throws(() => resolveRuntime(f.root, { HERALD_EXTERNAL_COMPANION: '1', HERALD_DATA }), /external companion requires HERALD_DATA/)
    assert.deepEqual(resolveRuntime(f.root, { HERALD_EXTERNAL_COMPANION: '1', HERALD_DATA: '/isolated' }), { binary: f.binary, assets: f.assets })
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})

test('portable runtime names are platform-specific without searching for another host binary', () => {
  const root = resolve('portable root')
  for (const platform of ['win32', 'darwin', 'linux']) {
    for (const arch of ['x64', 'arm64']) {
      const expected = {
        binary: join(root, 'native-announcer/bin', `herald-${platform}-${arch}${platform === 'win32' ? '.exe' : ''}`),
        assets: join(root, 'native-announcer/resources'),
      }
      const paths = new Set([join(root, 'bundle-manifest.json'), expected.binary, expected.assets])
      assert.deepEqual(resolveRuntime(root, {}, platform, path => paths.has(path), arch), expected)
    }
  }
})

test('bridge writes JSON to stdin with bounded execFile and parses only native responses', async () => {
  const f = directory()
  try {
    const calls = []
    const env = { HERALD_EXTERNAL_COMPANION: '1', HERALD_DATA: '/isolated', OTHER: 'preserved' }
    const bridge = createBridge({ root: f.root, env, run(binary, args, options, callback) {
      const stdin = new EventEmitter()
      stdin.end = body => {
        calls.push({ binary, args, options, body: JSON.parse(body) })
        callback(null, '{"prompt":"Selected character prompt","characterID":"wizard"}', '')
      }
      return { stdin }
    } })
    assert.deepEqual(calls, [])
    assert.deepEqual(await bridge.send({ type: 'read-announcement-profile' }), { prompt: 'Selected character prompt', characterID: 'wizard' })
    assert.deepEqual(calls, [{
      binary: f.binary, args: ['--bridge', '--assets', f.assets],
      options: { env, encoding: 'utf8', windowsHide: true, timeout: 10000, maxBuffer: 1048576 },
      body: { type: 'read-announcement-profile' },
    }])
    assert.equal(existsSync(join(f.root, 'inbox')), false)
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})

test('bridge accepts empty native write responses and reports malformed or failed requests', async () => {
  const f = directory()
  try {
    let response = { error: null, stdout: '', stderr: '' }
    const bridge = createBridge({ root: f.root, env: {}, run(_binary, _args, _options, callback) {
      const stdin = new EventEmitter()
      stdin.end = () => callback(response.error, response.stdout, response.stderr)
      return { stdin }
    } })
    assert.equal(await bridge.send({ type: 'boot' }), undefined)
    response = { error: null, stdout: 'not JSON', stderr: '' }
    await assert.rejects(bridge.send({ type: 'read-announcement-profile' }), /Invalid Herald bridge response/)
    response = { error: new Error('timed out'), stdout: '', stderr: 'native timeout diagnostic' }
    await assert.rejects(bridge.send({ type: 'notify' }), /Herald bridge notify: native timeout diagnostic/)
    response = { error: new Error('timed out'), stdout: '', stderr: '' }
    await assert.rejects(bridge.send({ type: 'boot' }), /Herald bridge boot: timed out/)
    response = { error: null, stdout: '', stderr: '' }
    assert.equal(await bridge.send({ type: 'discard', sessionID: 'pi:main', at: 123 }), undefined)
  } finally {
    rmSync(f.root, { recursive: true, force: true })
  }
})
