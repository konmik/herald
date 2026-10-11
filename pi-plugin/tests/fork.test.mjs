import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { test } from 'bun:test'
import { announcementText, forkMessages, snapshotBranch } from '../fork.mjs'

function sdkEntry() {
  try {
    return createRequire(import.meta.url).resolve('@earendil-works/pi-coding-agent')
  } catch {
    const directory = process.env.HERALD_PI_SDK_DIRECTORY ?? (process.env.APPDATA && join(process.env.APPDATA, 'npm/node_modules/@earendil-works/pi-coding-agent'))
    const entry = directory && join(directory, 'dist/index.js')
    return entry && existsSync(entry) ? entry : undefined
  }
}

const entry = sdkEntry()
const sdk = entry ? await import(pathToFileURL(entry).href) : undefined
const sdkTest = sdk ? test : test.skip
const tempRoot = process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir()
const completed = 1700000060000
const image = { type: 'image', data: 'aGVyYWxk', mimeType: 'image/png' }
const usage = { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }
const assistant = {
  role: 'assistant', content: [{ type: 'text', text: 'The selected branch is complete.' }],
  api: 'openai-responses', provider: 'openai', model: 'actual-model', usage, stopReason: 'stop', timestamp: 123,
}

test('snapshot requires a real selected branch and clones header plus getBranch, never all entries', () => {
  const header = { type: 'session', id: 'source', timestamp: 'original', cwd: '/work' }
  const branch = [{ type: 'message', id: 'leaf', parentId: null, message: assistant }]
  const source = {
    getHeader: () => header, getBranch: () => branch, getEntries: () => { throw new Error('Forbidden') },
    getLeafId: () => 'leaf', getSessionFile: () => '/source.jsonl', getCwd: () => '/work',
  }
  const snapshot = snapshotBranch(source, completed, () => 'unique-fork')
  assert.deepEqual(snapshot, {
    cwd: '/work', leaf: 'leaf', entries: [
      { type: 'session', id: 'unique-fork', timestamp: new Date(completed).toISOString(), cwd: '/work', parentSession: '/source.jsonl' },
      ...branch,
    ],
  })
  snapshot.entries[1].message.content[0].text = 'Fork-only change'
  assert.equal(branch[0].message.content[0].text, 'The selected branch is complete.')
  assert.deepEqual(header, { type: 'session', id: 'source', timestamp: 'original', cwd: '/work' })
  assert.throws(() => snapshotBranch({ ...source, getHeader: () => null }, completed), /no header/)
  assert.throws(() => snapshotBranch({ ...source, getLeafId: () => 'abandoned' }, completed), /matching leaf/)
  assert.throws(() => snapshotBranch({ ...source, getLeafId: () => null }, completed), /matching leaf/)
})

test('announcement output collapses whitespace but rejects any tool, error, aborted, or empty reply', () => {
  assert.equal(announcementText({ ...assistant, content: [{ type: 'text', text: ' First\n sentence. ' }, { type: 'text', text: '\tSecond sentence.\r\n' }] }), 'First sentence. Second sentence.')
  for (const reply of [
    undefined, { role: 'user', content: 'Not an assistant' },
    { ...assistant, stopReason: 'toolUse' }, { ...assistant, stopReason: 'error' }, { ...assistant, stopReason: 'aborted' },
    { ...assistant, content: [] }, { ...assistant, content: [{ type: 'text', text: ' \n ' }] },
    { ...assistant, content: [{ type: 'toolCall', name: 'write', arguments: {} }, { type: 'text', text: 'Should not speak' }] },
  ]) assert.throws(() => announcementText(reply), /Pi announcement/)
  assert.equal(announcementText(assistant), 'The selected branch is complete.')
})

sdkTest('installed public SDK preserves all system sections, tool deltas, images, branch edits, and source bytes', () => {
  const directory = mkdtempSync(join(tempRoot, 'herald-pi-fork-'))
  try {
    const main = sdk.SessionManager.create(directory, directory)
    const tool = { name: 'bash', description: 'Run a foreground command', parameters: { type: 'object', properties: { command: { type: 'string' } } } }
    const leading = { role: 'system', content: 'FULL ORIGINAL SYSTEM POLICY', sections: { identity: '<identity>Real host</identity>', rules: '<rules>Preserve conversation</rules>' }, toolsAdded: [tool], timestamp: 1 }
    main.appendMessage(leading)
    const root = main.appendMessage({ role: 'user', content: 'Original human context marker', timestamp: 2 })
    main.appendMessage({ ...assistant, content: [{ type: 'text', text: 'ABANDONED BRANCH DO NOT SUMMARIZE' }] })
    main.branchWithSummary(root, 'Returned from the abandoned branch.')
    const delta = { role: 'system', content: 'LATER SYSTEM INSTRUCTION', sections: { rules: '<rules>Updated but preserved</rules>', identity: null, extra: '<extra>Keep images</extra>' }, toolsRemoved: [{ name: 'bash' }], timestamp: 3 }
    main.appendMessage(delta)
    const user = main.appendMessage({ role: 'user', content: [{ type: 'text', text: 'Unedited context' }, image], timestamp: 4 })
    const edited = [{ type: 'text', text: 'Edited selected-branch human marker' }, image]
    main.appendContextEdit(user, { content: edited })
    main.appendMessage(assistant)
    const before = readFileSync(main.getSessionFile())
    const branchBefore = structuredClone(main.getBranch())
    const leafBefore = main.getLeafId()
    const snapshot = snapshotBranch(main, completed)
    const second = snapshotBranch(main, completed)
    assert.notEqual(snapshot.entries[0].id, main.getSessionId())
    assert.notEqual(snapshot.entries[0].id, second.entries[0].id)
    assert.equal(snapshot.entries[0].parentSession, main.getSessionFile())
    const messages = forkMessages(snapshot, 'Selected character announcement prompt', completed, sdk.SessionManager.inMemory, sdk.convertToLlm)
    const branchEntry = branchBefore.find(item => item.type === 'branch_summary')
    assert.deepEqual(messages, [
      leading,
      { role: 'user', content: 'Original human context marker', timestamp: 2 },
      { role: 'user', content: [{ type: 'text', text: 'The following is a summary of a branch that this conversation came back from:\n\n<summary>\nReturned from the abandoned branch.</summary>' }], timestamp: new Date(branchEntry.timestamp).getTime() },
      delta,
      { role: 'user', content: edited, timestamp: 4 },
      assistant,
      { role: 'user', content: 'Selected character announcement prompt', timestamp: completed },
    ])
    assert.deepEqual(main.getBranch(), branchBefore)
    assert.equal(main.getLeafId(), leafBefore)
    assert.deepEqual(readFileSync(main.getSessionFile()), before)
    assert.throws(() => forkMessages(second, '', completed, sdk.SessionManager.inMemory, sdk.convertToLlm), /profile has no prompt/)
    assert.deepEqual(readFileSync(main.getSessionFile()), before)
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})

sdkTest('installed public SDK owns compaction projection and retains checkpoint system plus edited images', () => {
  const main = sdk.SessionManager.inMemory('/compacted-work', { id: randomUUID() })
  main.appendMessage({ role: 'system', content: 'ORIGINAL FULL SYSTEM', sections: { guard: '<guard>Preserve sections</guard>' }, timestamp: 1 })
  main.appendMessage({ role: 'user', content: 'History that must be compacted', timestamp: 2 })
  main.appendMessage({ role: 'system', content: 'ADDITIONAL SYSTEM', sections: { extra: '<extra>Do not drop</extra>' }, timestamp: 3 })
  const kept = main.appendMessage({ role: 'user', content: [{ type: 'text', text: 'Kept original human turn' }, image], timestamp: 4 })
  main.appendMessage(assistant)
  main.appendContextEdit(kept, { content: [{ type: 'text', text: 'Kept edited turn' }, image] })
  const compaction = main.appendCompaction('Full-context compacted history marker', kept, 10000)
  const after = { role: 'system', content: 'AFTER COMPACTION', sections: { guard: '<guard>Updated after checkpoint</guard>' }, timestamp: 5 }
  main.appendMessage(after)
  main.appendMessage(assistant)
  const before = structuredClone(main.getBranch())
  const compacted = main.getEntry(compaction)
  const snapshot = snapshotBranch(main, completed)
  assert.deepEqual(snapshot.entries.slice(1), before)
  const messages = forkMessages(snapshot, 'Announce the compacted branch', completed, sdk.SessionManager.inMemory, sdk.convertToLlm)
  assert.deepEqual(messages, [
    { role: 'system', content: 'ORIGINAL FULL SYSTEM\n\nADDITIONAL SYSTEM', sections: { guard: '<guard>Preserve sections</guard>', extra: '<extra>Do not drop</extra>' }, timestamp: new Date(compacted.timestamp).getTime() },
    { role: 'user', content: [{ type: 'text', text: 'The conversation history before this point was compacted into the following summary:\n\n<summary>\nFull-context compacted history marker\n</summary>' }], timestamp: new Date(compacted.timestamp).getTime() },
    { role: 'user', content: [{ type: 'text', text: 'Kept edited turn' }, image], timestamp: 4 },
    assistant,
    after,
    assistant,
    { role: 'user', content: 'Announce the compacted branch', timestamp: completed },
  ])
  assert.deepEqual(main.getBranch(), before)
  assert.equal(main.getSessionFile(), undefined)
})
