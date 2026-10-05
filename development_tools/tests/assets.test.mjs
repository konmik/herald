import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { installDirectory } from '../install-directory.mjs'

async function fixture(t) {
  const root = await mkdtemp(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'asset-test-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  return root
}

test('installs a complete asset directory on a fresh installation', async t => {
  const root = await fixture(t)
  const staged = join(root, 'staged')
  const destination = join(root, 'installed')
  await mkdir(staged)
  await writeFile(join(staged, 'model.onnx'), 'complete')
  await installDirectory(staged, destination)
  assert.equal(await readFile(join(destination, 'model.onnx'), 'utf8'), 'complete')
  assert.deepEqual(await readdir(root), ['installed'])
})

test('repairs a nonempty incomplete installation with the verified directory', async t => {
  const root = await fixture(t)
  const staged = join(root, 'staged')
  const destination = join(root, 'installed')
  await mkdir(staged)
  await mkdir(destination)
  await writeFile(join(destination, 'model.onnx'), 'incomplete')
  await writeFile(join(staged, 'model.onnx'), 'complete')
  await writeFile(join(staged, 'LICENSE'), 'license')
  await installDirectory(staged, destination)
  assert.equal(await readFile(join(destination, 'model.onnx'), 'utf8'), 'complete')
  assert.equal(await readFile(join(destination, 'LICENSE'), 'utf8'), 'license')
  assert.deepEqual(await readdir(root), ['installed'])
})

test('retains the previous installation when publishing fails', async t => {
  const root = await fixture(t)
  const destination = join(root, 'installed')
  await mkdir(destination)
  await writeFile(join(destination, 'model.onnx'), 'previous')
  await assert.rejects(installDirectory(join(root, 'missing'), destination))
  assert.equal(await readFile(join(destination, 'model.onnx'), 'utf8'), 'previous')
  assert.deepEqual(await readdir(root), ['installed'])
})

test('Windows builds prepare missing GPU assets and stop if preparation fails', { skip: process.platform !== 'win32' || process.arch !== 'x64' }, async t => {
  const root = await fixture(t)
  const tools = join(root, 'development_tools')
  await mkdir(tools)
  await copyFile(new URL('../build-announcer.mjs', import.meta.url), join(tools, 'build-announcer.mjs'))
  await writeFile(join(tools, 'prepare-tts.mjs'), 'export {}\n')
  await writeFile(join(tools, 'prepare-gpu-tts.ps1'), "Set-Content -LiteralPath (Join-Path $PSScriptRoot 'prepared') -Value 'gpu'\nexit 73\n")
  const result = spawnSync(process.execPath, [join(tools, 'build-announcer.mjs')], { encoding: 'utf8', timeout: 15000 })
  assert.ifError(result.error)
  assert.equal(await readFile(join(tools, 'prepared'), 'utf8').catch(() => null), 'gpu\r\n')
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, /GPU runtime preparation failed/)
})
