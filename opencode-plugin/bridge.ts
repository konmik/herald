import { chmod, mkdir, rename, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { randomUUID } from "node:crypto"
import { homedir } from "node:os"

export const dataDirectory = process.env.HERALD_DATA ?? (process.platform === "win32"
  ? join(process.env.LOCALAPPDATA!, "herald")
  : process.platform === "darwin"
    ? join(homedir(), "Library", "Application Support", "herald")
    : join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share"), "herald"))
export const inbox = join(dataDirectory, "inbox")

export async function send(command: object) {
  await mkdir(inbox, { recursive: true, mode: 0o700 })
  if (process.platform !== "win32") {
    await chmod(dataDirectory, 0o700)
    await chmod(inbox, 0o700)
  }
  const name = join(inbox, `${Date.now()}-${randomUUID()}`)
  await writeFile(`${name}.tmp`, JSON.stringify(command), { mode: 0o600 })
  await rename(`${name}.tmp`, `${name}.json`)
}
