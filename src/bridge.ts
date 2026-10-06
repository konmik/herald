import { mkdir, rename, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { randomUUID } from "node:crypto"
import { homedir } from "node:os"

export const dataDirectory = process.env.CIVILIZED_AGENT_DATA ?? (process.platform === "win32"
  ? join(process.env.LOCALAPPDATA!, "CivilizedAgent")
  : process.platform === "darwin"
    ? join(homedir(), "Library", "Application Support", "CivilizedAgent")
    : join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share"), "CivilizedAgent"))
export const inbox = join(dataDirectory, "inbox")

export async function send(command: object) {
  await mkdir(inbox, { recursive: true })
  const name = join(inbox, `${Date.now()}-${randomUUID()}`)
  await writeFile(`${name}.tmp`, JSON.stringify(command), { mode: 0o600 })
  await rename(`${name}.tmp`, `${name}.json`)
}
