import { closeSync, fstatSync, openSync, readSync } from 'node:fs'

export function sessionTitle(path, fallback = '') {
  if (!path) return fallback || 'Untitled session'
  let file
  try {
    file = openSync(path, 'r')
    const size = fstatSync(file).size
    const bytes = Buffer.alloc(Math.min(size, 262144))
    readSync(file, bytes, 0, bytes.length, size - bytes.length)
    let title = fallback
    for (const line of bytes.toString('utf8').split('\n')) {
      try {
        const entry = JSON.parse(line)
        if (entry.type === 'custom-title' && typeof entry.customTitle === 'string') title = entry.customTitle
        if (entry.type === 'summary' && typeof entry.summary === 'string' && !title) title = entry.summary
      } catch {}
    }
    return title || 'Untitled session'
  } catch {
    return fallback || 'Untitled session'
  } finally {
    if (file !== undefined) closeSync(file)
  }
}
