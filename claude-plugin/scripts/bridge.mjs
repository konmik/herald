import { chmod, mkdir, rename, writeFile } from 'node:fs/promises'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { randomUUID } from 'node:crypto'
import { boot, dataDirectory } from './runtime.mjs'
import { sessionTitle } from './session-title.mjs'
import { readSummaryPrompt } from './summary-prompt.mjs'

const command = JSON.parse(readFileSync(0, 'utf8').replace(/^\uFEFF/, ''))
if (command.type === 'read-summary-prompt') {
  process.stdout.write(JSON.stringify(readSummaryPrompt()))
} else if (command.type === 'boot') {
  boot()
} else {
  if (command.type === 'notify') {
    command.title = sessionTitle(command.transcriptPath, command.title)
    delete command.transcriptPath
  }
  const inbox = join(dataDirectory, 'inbox')
  await mkdir(inbox, { recursive: true, mode: 0o700 })
  if (process.platform !== 'win32') {
    await chmod(dataDirectory, 0o700)
    await chmod(inbox, 0o700)
  }
  const path = join(inbox, `${Date.now()}-${randomUUID()}`)
  await writeFile(`${path}.tmp`, JSON.stringify(command), { mode: 0o600 })
  await rename(`${path}.tmp`, `${path}.json`)
}
