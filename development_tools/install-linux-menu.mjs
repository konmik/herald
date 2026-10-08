import { mkdir, readFile, writeFile, access } from 'node:fs/promises'
import { constants } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

function execArgument(value) {
  return '"' + value.replaceAll('%', '%%').replaceAll('\\', '\\\\\\\\').replace(/["`$]/g, '\\\\$&') + '"'
}

export async function installLinuxMenu(root, { env = process.env, refresh = true } = {}) {
  if (/[\r\n]/.test(root)) throw new Error('The installation path must not contain carriage returns or newlines.')
  const binary = join(root, 'native-announcer', 'bin', `herald-linux-${process.arch}`)
  const assets = join(root, 'native-announcer', 'resources')
  await access(binary, constants.X_OK)
  const applications = join(env.XDG_DATA_HOME || join(env.HOME || homedir(), '.local/share'), 'applications')
  const path = join(applications, 'herald-settings.desktop')
  const icon = join(root, 'native-announcer', 'resources', 'portraits', 'flamboyant-herald.png')
  await access(icon)
  const content = `[Desktop Entry]\nType=Application\nName=Herald Settings\nComment=Configure Herald announcements\nExec=/usr/bin/env -- ${execArgument(binary)} --settings --assets ${execArgument(assets)}\nIcon=${icon.replaceAll('\\', '\\\\')}\nTerminal=false\nCategories=Settings;\n`
  await mkdir(applications, { recursive: true })
  const previous = await readFile(path, 'utf8').catch(error => {
    if (error.code === 'ENOENT') return undefined
    throw error
  })
  if (previous !== content) await writeFile(path, content)
  if (refresh) {
    const result = spawnSync('update-desktop-database', [applications], { env, stdio: 'inherit' })
    if (result.error && result.error.code !== 'ENOENT') throw result.error
    if (!result.error && result.status !== 0) throw new Error('Could not refresh the application launcher database')
  }
  return path
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(await installLinuxMenu(fileURLToPath(new URL('..', import.meta.url))))
}
