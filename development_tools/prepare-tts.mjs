import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { access, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { installDirectory } from './install-directory.mjs'

const name = 'kitten-nano-en-v0_8-int8'
const resources = fileURLToPath(new URL('../native-announcer/resources/tts/', import.meta.url))
const destination = join(resources, name)
const required = ['model.int8.onnx', 'voices.bin', 'tokens.txt', 'espeak-ng-data/en_dict', 'LICENSE']
try {
  await Promise.all(required.map(file => access(join(destination, file))))
  console.log(`Kitten Nano 0.8 INT8 ready: ${destination}`)
} catch {
  await mkdir(resources, { recursive: true })
  const stage = await mkdtemp(join(resources, '.kitten-install-'))
  try {
    const archive = join(stage, `${name}.tar.bz2`)
    const response = await fetch(`https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/${name}.tar.bz2`, { signal: AbortSignal.timeout(120000) })
    if (!response.ok) throw new Error(`Kitten model download failed: ${response.status}`)
    const bytes = Buffer.from(await response.arrayBuffer())
    if (createHash('sha256').update(bytes).digest('hex') !== '6fa5be852612ce761094ba74ee6123b4fc4acfefa79bf64dc63acae4a83af2fd') throw new Error('Kitten model checksum mismatch')
    await writeFile(archive, bytes)
    const extraction = spawnSync('tar', ['-xjf', archive, '-C', stage], { stdio: 'inherit', timeout: 60000 })
    if (extraction.error) throw extraction.error
    if (extraction.status !== 0) throw new Error('Kitten model extraction failed')
    await Promise.all(required.map(file => access(join(stage, name, file))))
    await installDirectory(join(stage, name), destination)
    console.log(`Installed Kitten Nano 0.8 INT8: ${destination}`)
  } finally {
    await rm(stage, { recursive: true, force: true })
  }
}
