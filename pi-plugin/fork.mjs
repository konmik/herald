import { randomUUID } from 'node:crypto'

export function snapshotBranch(source, completed, id = randomUUID) {
  const header = source.getHeader()
  if (!header) throw new Error('Pi session has no header')
  const leaf = source.getLeafId()
  const entries = structuredClone([header, ...source.getBranch()])
  if (!leaf || entries.at(-1)?.id !== leaf) throw new Error('Pi active branch has no matching leaf')
  entries[0].id = id()
  entries[0].timestamp = new Date(completed).toISOString()
  const sourceFile = source.getSessionFile()
  if (sourceFile) entries[0].parentSession = sourceFile
  return { entries, leaf, cwd: source.getCwd() }
}

export function forkMessages(snapshot, prompt, completed, inMemory, convertToLlm) {
  if (typeof prompt !== 'string' || !prompt.trim()) throw new Error('Herald announcement profile has no prompt')
  const fork = inMemory(snapshot.cwd, undefined, snapshot.entries)
  if (fork.getLeafId() !== snapshot.leaf) throw new Error('Pi fork changed the selected branch')
  fork.appendMessage({ role: 'user', content: prompt, timestamp: completed })
  return convertToLlm(fork.buildSessionContext().messages)
}

export function announcementText(reply) {
  if (!reply || reply.role !== 'assistant' || ['error', 'aborted', 'toolUse'].includes(reply.stopReason)) {
    throw new Error('Pi announcement did not finish with assistant text')
  }
  if (!Array.isArray(reply.content) || reply.content.some(part => part.type === 'toolCall')) {
    throw new Error('Pi announcement requested a tool or returned invalid content')
  }
  const text = reply.content.filter(part => part.type === 'text').map(part => part.text).join(' ').replace(/\s+/g, ' ').trim()
  if (!text) throw new Error('Pi announcement returned empty text')
  return text
}
