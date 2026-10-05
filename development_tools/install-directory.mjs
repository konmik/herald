import { randomUUID } from 'node:crypto'
import { rename, rm } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'

export async function installDirectory(staged, destination) {
  const previous = `${destination}.previous-${randomUUID()}`
  let replaced = false
  try {
    await rename(destination, previous)
    replaced = true
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
  try {
    await rename(staged, destination)
  } catch (error) {
    if (replaced) {
      try {
        await rename(previous, destination)
      } catch (rollback) {
        throw new AggregateError([error, rollback], `Installation failed; previous assets remain at ${previous}`)
      }
    }
    throw error
  }
  if (replaced) await rm(previous, { recursive: true, force: true })
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await installDirectory(process.argv[2], process.argv[3])
}
