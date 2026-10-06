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

test('Windows builds prepare CPU speech without GPU assets', async () => {
  const source = await readFile(new URL('../build-announcer.mjs', import.meta.url), 'utf8')
  assert.match(source, /import\('\.\/prepare-tts\.mjs'\)/)
  assert.doesNotMatch(source, /gpu|cuda/i)
})

test('bundled characters cover every library video with usable voice design inputs', async () => {
  const resources = new URL('../../native-announcer/resources/', import.meta.url)
  const catalog = JSON.parse(await readFile(new URL('characters.json', resources), 'utf8'))
  const videos = (await readdir(new URL('videos/', resources))).filter(name => name.endsWith('.mp4')).sort()
  const paths = Object.values(catalog).map(character => character.animationPath).sort()
  assert.deepEqual(paths, videos.map(name => `videos/${name}`))
  assert.equal(new Set(Object.values(catalog).map(character => character.name)).size, videos.length)
  for (const [id, character] of Object.entries(catalog)) {
    assert.match(id, /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/)
    assert.equal(typeof character.name, 'string')
    assert.equal(character.name.trim().length > 0 && character.name.length <= 160, true)
    assert.equal(character.voiceDescription.trim().length >= 20 && character.voiceDescription.length <= 1000, true)
    assert.equal(Object.hasOwn(character, 'sampleText'), false)
    assert.equal(Object.hasOwn(character, 'voice'), false)
    assert.equal((await readFile(new URL(character.animationPath, resources))).length > 0, true)
  }
})
