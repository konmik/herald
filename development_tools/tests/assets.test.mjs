import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
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

test('builds do not download or bundle the optional voice engine', async () => {
  for (const name of ['build-announcer.mjs', 'build-bundle.ps1']) {
    const source = await readFile(new URL(`../${name}`, import.meta.url), 'utf8')
    assert.doesNotMatch(source, /prepare-tts|onnxruntime\.dll|sherpa-onnx-c-api\.dll|tts\/kitten|gpu|cuda/i)
  }
})
