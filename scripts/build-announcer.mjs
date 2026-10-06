import { spawn, spawnSync } from 'node:child_process'
import { chmod, copyFile, mkdir } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'

const root = fileURLToPath(new URL('..', import.meta.url))
const release = process.argv.includes('--release')
const args = ['build', '--locked', '-j', '6', '--manifest-path', join(root, 'announcer', 'Cargo.toml')]
if (release) args.push('--release')
const child = spawn('cargo', args, { stdio: 'inherit', detached: process.platform !== 'win32' })
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
const directory = join(root, 'claude', 'bin')
await mkdir(directory, { recursive: true })
const suffix = process.platform === 'win32' ? '.exe' : ''
const binary = join(directory, `civilized-announcer-${process.platform}-${process.arch}${suffix}`)
await copyFile(join(root, 'announcer', 'target', release ? 'release' : 'debug', `civilized-announcer${suffix}`), binary)
await chmod(binary, 0o755)
console.log(binary)
