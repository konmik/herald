import { execFile } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export function resolveRuntime(root, env = process.env, platform = process.platform, exists = existsSync, arch = process.arch) {
  if (env.HERALD_EXTERNAL_COMPANION === '1' && !env.HERALD_DATA) {
    throw new Error('An external companion requires HERALD_DATA')
  }
  const installed = exists(join(root, 'bundle-manifest.json'))
  const binary = installed || !env.HERALD_BINARY
    ? join(root, 'native-announcer', 'bin', installed ? `herald-${platform}-${arch}${platform === 'win32' ? '.exe' : ''}` : platform === 'win32' ? 'herald.exe' : 'herald')
    : resolve(env.HERALD_BINARY)
  const assets = installed || !env.HERALD_ASSETS
    ? join(root, 'native-announcer', 'resources')
    : resolve(env.HERALD_ASSETS)
  if (!exists(binary)) throw new Error(`Herald binary not found: ${binary}`)
  if (!exists(assets)) throw new Error(`Herald assets not found: ${assets}`)
  return { binary, assets }
}

export function createBridge({
  root = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  env = process.env,
  run = execFile,
} = {}) {
  return {
    send(command) {
      return new Promise((resolveResult, reject) => {
        const { binary, assets } = resolveRuntime(root, env)
        const child = run(binary, ['--bridge', '--assets', assets], {
          env,
          encoding: 'utf8',
          windowsHide: true,
          timeout: 10000,
          maxBuffer: 1048576,
        }, (error, stdout, stderr) => {
          if (error) {
            reject(new Error(`Herald bridge ${command.type}: ${stderr?.trim() || error.message}`))
            return
          }
          try {
            resolveResult(stdout.trim() ? JSON.parse(stdout) : undefined)
          } catch (error) {
            reject(new Error(`Invalid Herald bridge response: ${error.message}`))
          }
        })
        child.stdin.on('error', reject)
        child.stdin.end(JSON.stringify(command))
      })
    },
  }
}
