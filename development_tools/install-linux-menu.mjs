import { mkdir, readFile, writeFile, access } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

function execArgument(value) {
  return '"' + value.replaceAll('%', '%%').replaceAll('\\', '\\\\\\\\').replace(/["`$]/g, '\\\\$&') + '"'
}

export async function installLinuxMenu(root, { env = process.env, refresh = true } = {}) {
  const opener = join(root, 'development_tools', 'open-linux-settings.sh')
  await access(opener)
  const applications = join(env.XDG_DATA_HOME || join(env.HOME || homedir(), '.local/share'), 'applications')
  const path = join(applications, 'herald-settings.desktop')
  const content = `[Desktop Entry]\nType=Application\nName=Herald Settings\nComment=Edit Herald announcement settings\nExec=sh ${execArgument(opener)}\nIcon=preferences-system\nTerminal=false\nCategories=Settings;\n`
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
