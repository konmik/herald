import { spawn, spawnSync } from 'node:child_process'
import { access, chmod, copyFile, mkdir } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { join, resolve } from 'node:path'

const root = fileURLToPath(new URL('..', import.meta.url))
const release = process.argv.includes('--release')
if (process.platform === 'win32') await import('./prepare-tts.mjs')
const args = ['build', '--locked', '-j', '6', '--manifest-path', join(root, 'native-announcer', 'Cargo.toml')]
if (release) args.push('--release')
const child = spawn('cargo', args, { cwd: root, stdio: 'inherit', detached: process.platform !== 'win32' })
let timedOut = false
const timeout = setTimeout(() => {
  timedOut = true
  if (process.platform === 'win32') spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true, timeout: 10000 })
  else process.kill(-child.pid, 'SIGTERM')
}, 120000)
const code = await new Promise((resolve, reject) => {
  child.once('error', reject)
  child.once('exit', resolve)
}).finally(() => clearTimeout(timeout))
if (timedOut) throw new Error('Native build exceeded two minutes; its process tree was stopped.')
if (code !== 0) process.exit(code ?? 1)
const directory = join(root, 'native-announcer', 'bin')
await mkdir(directory, { recursive: true })
const suffix = process.platform === 'win32' ? '.exe' : ''
const binary = join(directory, `civilized-announcer-${process.platform}-${process.arch}${suffix}`)
const target = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'native-announcer', 'target')
await copyFile(join(target, release ? 'release' : 'debug', `civilized-announcer${suffix}`), binary)
if (process.platform === 'win32') {
  for (const name of ['sherpa-onnx-c-api.dll', 'onnxruntime.dll']) {
    await copyFile(join(target, release ? 'release' : 'debug', name), join(directory, name))
  }
  const gpu = join(directory, 'gpu')
  if (await access(join(gpu, 'onnxruntime.dll')).then(() => true, () => false)) {
    await copyFile(binary, join(gpu, 'civilized-announcer.exe'))
  }
}
await chmod(binary, 0o755)
console.log(binary)
