import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, utimes, writeFile } from 'node:fs/promises'
import { homedir, tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { test } from 'node:test'
import { piAgentDirectory, registerPi, updatePiRegistration } from '../bundle/register-pi.mjs'

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'Herald Ω pi-registration-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const installed = join(directory, 'installed')
  const old = join(directory, 'old')
  for (const root of [installed, old]) {
    await mkdir(join(root, 'pi-plugin'), { recursive: true })
    await writeFile(join(root, 'package.json'), JSON.stringify({ name: 'herald', exports: { '.': './opencode-plugin/index.ts' }, pi: { extensions: ['./pi-plugin/index.ts'] } }))
    await writeFile(join(root, 'pi-plugin/index.ts'), 'import "./bridge.mjs"\n')
    await writeFile(join(root, 'pi-plugin/bridge.mjs'), 'export {}\n')
  }
  return { directory, installed, old, agent: join(directory, 'agent'), entry: join(installed, 'pi-plugin/index.ts').replaceAll('\\', '/') }
}

test('Pi registration migrates only owned extensions and preserves other settings', async t => {
  const { directory, installed, old, entry } = await fixture(t)
  const foreign = join(directory, 'foreign/pi-plugin/index.ts')
  const settings = { model: 'keep', nested: { keep: true }, extensions: ['builtin:mcp', '-builtin:llama.cpp', 'npm:other', foreign, join(old, 'pi-plugin/index.ts'), join(old, 'pi-plugin')] }
  const result = updatePiRegistration(JSON.stringify(settings), directory, installed)
  assert.deepEqual(JSON.parse(result), { ...settings, extensions: [...settings.extensions.slice(0, 4), entry] })
  assert.equal(updatePiRegistration(result, directory, installed), result)
})

test('Pi registration preserves unknown Herald-like paths and unrelated text', async t => {
  const { directory, installed, old, entry } = await fixture(t)
  await writeFile(join(old, 'package.json'), '{"name":"other"}')
  const text = '\ufeff{\r\n  "model": "keep",\r\n  "extensions": ' + JSON.stringify(['./old/pi-plugin/index.ts']) + '\r\n}\r\n'
  const result = updatePiRegistration(text, directory, installed)
  assert.deepEqual(JSON.parse(result.slice(1)).extensions, ['./old/pi-plugin/index.ts', entry])
  assert.ok(result.startsWith('\ufeff'))
  assert.ok(result.includes('"model": "keep"'))
  assert.ok(result.includes('\r\n'))
})

test('Pi registration rejects invalid settings and unowned payloads', async t => {
  const { directory, installed } = await fixture(t)
  for (const text of ['[]', '{invalid}', '{"extensions":null}', '{"extensions":{}}', '{"extensions":[{}]}', '{"extensions":[],"extensions":[]}', '{"nested":{"a":1,"a":2}}', '{/* comment */}', '{"extensions":[],}']) {
    assert.throws(() => updatePiRegistration(text, directory, installed))
  }
  assert.throws(() => updatePiRegistration('{}', directory, join(directory, 'unknown')), /ownership/)
})

test('Pi dry runs leave absent profiles untouched and registration backs up exact bytes', async t => {
  const { directory, installed, agent, entry } = await fixture(t)
  assert.deepEqual(await registerPi(installed, agent, { dryRun: true }), { config: join(agent, 'settings.json'), changed: true, dryRun: true })
  assert.ok(!(await readdir(directory)).includes('agent'))
  await mkdir(agent)
  const bytes = Buffer.from('{"model":"keep","extensions":[]}\n')
  await writeFile(join(agent, 'settings.json'), bytes)
  const backupDirectory = join(directory, 'backups')
  assert.equal((await registerPi(installed, agent, { backupDirectory })).changed, true)
  assert.deepEqual(await readFile(join(backupDirectory, 'pi-settings.json')), bytes)
  assert.deepEqual(JSON.parse(await readFile(join(agent, 'settings.json'), 'utf8')), { model: 'keep', extensions: [entry] })
  assert.equal((await registerPi(installed, agent, { backupDirectory })).changed, false)
  assert.deepEqual(await readdir(agent), ['settings.json'])
})

test('Pi registration refuses links and unsafe legacy locks without changing settings', async t => {
  const { directory, installed, agent } = await fixture(t)
  await mkdir(agent)
  await writeFile(join(agent, 'settings.json'), '{}')
  await mkdir(join(agent, 'settings.json.lock'))
  await assert.rejects(registerPi(installed, agent), /Unsafe Pi registration lock/)
  assert.equal(await readFile(join(agent, 'settings.json'), 'utf8'), '{}')
  const linked = join(directory, 'linked')
  await symlink(agent, linked, process.platform === 'win32' ? 'junction' : 'dir')
  await assert.rejects(registerPi(installed, linked), /Links are not allowed/)
  assert.deepEqual(await readdir(join(agent, 'settings.json.lock')), [])
})

test('Pi registration refuses an active PID lock and leaves settings and backups untouched', async t => {
  const { directory, installed, agent } = await fixture(t)
  await mkdir(agent)
  const config = join(agent, 'settings.json')
  const bytes = Buffer.from('\ufeff{\r\n  "extensions": ["builtin:mcp"], "model": "keep"\r\n}\r\n')
  const lock = JSON.stringify({ owner: 'herald-pi', pid: process.pid, token: 'active-registrar' })
  await writeFile(config, bytes)
  await writeFile(config + '.lock', lock)
  const backupDirectory = join(directory, 'backups')
  await assert.rejects(registerPi(installed, agent, { backupDirectory }), /already active/)
  assert.deepEqual(await readFile(config), bytes)
  assert.equal(await readFile(config + '.lock', 'utf8'), lock)
  assert.ok(!(await readdir(directory)).includes('backups'))
  assert.equal((await registerPi(installed, agent, { dryRun: true })).changed, true)
  assert.equal(await readFile(config + '.lock', 'utf8'), lock)
})

test('Pi registration recovers a crashed PID owner and preserves exact backup bytes', async t => {
  const { directory, installed, agent, entry } = await fixture(t)
  await mkdir(agent)
  const config = join(agent, 'settings.json')
  const bytes = Buffer.from('{"model":"keep","extensions":["builtin:mcp","npm:other"]}\r\n')
  await writeFile(config, bytes)
  const child = spawn(process.execPath, ['-e', 'require("node:fs").writeFileSync(process.argv[1], JSON.stringify({owner:"herald-pi",pid:process.pid,token:"crashed"})); process.exit(9)', config + '.lock'])
  const [code] = await once(child, 'exit')
  assert.equal(code, 9)
  const owner = JSON.parse(await readFile(config + '.lock', 'utf8'))
  assert.throws(() => process.kill(owner.pid, 0), { code: 'ESRCH' })
  const backupDirectory = join(directory, 'backups')
  await mkdir(backupDirectory)
  await writeFile(join(backupDirectory, 'pi-settings.json'), bytes)
  assert.equal((await registerPi(installed, agent, { backupDirectory })).changed, true)
  assert.deepEqual(await readFile(join(backupDirectory, 'pi-settings.json')), bytes)
  assert.deepEqual(JSON.parse(await readFile(config, 'utf8')), { model: 'keep', extensions: ['builtin:mcp', 'npm:other', entry] })
  const registered = await readFile(config)
  assert.equal((await registerPi(installed, agent, { backupDirectory })).changed, false)
  assert.deepEqual(await readFile(config), registered)
  assert.deepEqual(await readdir(agent), ['settings.json'])
})

test('Pi registration recovers only aged empty locks and refuses unknown owners', async t => {
  const { installed, agent, entry } = await fixture(t)
  await mkdir(agent)
  const config = join(agent, 'settings.json')
  const lockPath = config + '.lock'
  await writeFile(config, '{}')
  await writeFile(lockPath, '')
  await assert.rejects(registerPi(installed, agent), /still being initialized/)
  assert.equal(await readFile(config, 'utf8'), '{}')
  const stale = new Date(Date.now() - 120_000)
  await utimes(lockPath, stale, stale)
  assert.equal((await registerPi(installed, agent)).changed, true)
  assert.deepEqual(JSON.parse(await readFile(config, 'utf8')).extensions, [entry])
  for (const lock of ['{partial', '{}', '{"pid":0}', '{"pid":"unknown"}', JSON.stringify({ pid: process.pid, token: 'foreign' }), JSON.stringify({ owner: 'herald-pi', pid: process.pid })]) {
    await writeFile(config, '{}')
    await writeFile(lockPath, lock)
    await utimes(lockPath, stale, stale)
    await assert.rejects(registerPi(installed, agent), /Unknown Pi registration lock owner/)
    assert.equal(await readFile(config, 'utf8'), '{}')
    assert.equal(await readFile(lockPath, 'utf8'), lock)
  }
})

test('Pi registration releases its lock after a backup failure without changing settings', async t => {
  const { directory, installed, agent } = await fixture(t)
  await mkdir(agent)
  const config = join(agent, 'settings.json')
  const bytes = Buffer.from('{"extensions":[]}\n')
  await writeFile(config, bytes)
  const backupDirectory = join(directory, 'backups')
  await mkdir(backupDirectory)
  await writeFile(join(backupDirectory, 'pi-settings.json'), 'keep')
  await assert.rejects(registerPi(installed, agent, { backupDirectory }), /EEXIST/)
  assert.deepEqual(await readFile(config), bytes)
  assert.equal(await readFile(join(backupDirectory, 'pi-settings.json'), 'utf8'), 'keep')
  assert.deepEqual(await readdir(agent), ['settings.json'])
  assert.equal((await registerPi(installed, agent)).changed, true)
})

test('Pi agent directory accepts an explicit profile', () => {
  assert.equal(piAgentDirectory('./pi-profile'), resolve('./pi-profile'))
  assert.equal(piAgentDirectory('~/pi-profile'), join(homedir(), 'pi-profile'))
  assert.equal(piAgentDirectory('~\\pi-profile'), join(homedir(), 'pi-profile'))
  assert.equal(piAgentDirectory('~'), homedir())
})
