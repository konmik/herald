import { spawn } from 'node:child_process'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { existsSync } from 'node:fs'

export const dataDirectory = process.env.CIVILIZED_AGENT_DATA ?? (process.platform === 'win32'
  ? join(process.env.LOCALAPPDATA, 'CivilizedAgent')
  : process.platform === 'darwin'
    ? join(homedir(), 'Library', 'Application Support', 'CivilizedAgent')
    : join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'CivilizedAgent'))

export function boot() {
  if (process.env.CIVILIZED_AGENT_EXTERNAL_COMPANION === '1') {
    if (!process.env.CIVILIZED_AGENT_DATA) throw new Error('An external companion requires CIVILIZED_AGENT_DATA')
    return
  }
  const suffix = process.platform === 'win32' ? '.exe' : ''
  const packaged = new URL('../native-announcer/', import.meta.url)
  const announcer = existsSync(fileURLToPath(packaged)) ? packaged : new URL('../../native-announcer/', import.meta.url)
  const binary = process.env.CIVILIZED_AGENT_BINARY ?? fileURLToPath(new URL(`bin/civilized-announcer-${process.platform}-${process.arch}${suffix}`, announcer))
  if (!existsSync(binary)) throw new Error('Civilized Agent native announcer is missing. Run npm run build:announcer on this platform.')
  const assets = fileURLToPath(new URL('resources', announcer))
  const child = spawn(binary, ['--assets', assets], { detached: true, stdio: 'ignore', windowsHide: true })
  child.on('error', console.error)
  child.unref()
}
