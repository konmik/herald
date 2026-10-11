import assert from 'node:assert/strict'
import { resolve, dirname } from 'node:path'
import { test } from 'node:test'
import { inspectTask, parseArguments, sessionRecords, terminalArguments } from '../host-verification/pi-terminal.mjs'

test('Pi terminal driver accepts only its TUI and bounded smoke scenarios', () => {
  assert.deepEqual(parseArguments(['Background', 'proof']), { scenario: 'Background', evidence: resolve('proof') })
  assert.deepEqual(parseArguments(['Smoke', 'proof', '--cli', 'cli.js']), { scenario: 'Smoke', evidence: resolve('proof'), cli: resolve('cli.js') })
  assert.deepEqual(parseArguments(['Background', 'proof', '--extension', 'index.ts']), { scenario: 'Background', evidence: resolve('proof'), extension: resolve('index.ts') })
  for (const args of [[], ['CancelRestart', 'proof'], ['Subagent', 'proof'], ['Background'], ['Background', 'proof', '--unknown', 'a']]) assert.throws(() => parseArguments(args))
})

test('Pi terminal driver runs the actual CLI interactively under winpty', () => {
  const session = resolve('scratch/main.jsonl')
  const pi = { command: process.execPath, args: [resolve('pi/cli.js')] }
  const extension = resolve('pi-plugin/index.ts')
  const prompt = 'Original marker. Run one awaited command.'
  const args = terminalArguments(pi, session, extension, prompt)
  assert.deepEqual(args.slice(0, 5), ['-Xallow-non-tty', '-Xplain', '--', pi.command, pi.args[0]])
  assert.equal(args.at(-1), prompt)
  assert.equal(args[args.indexOf('--session') + 1], session)
  assert.equal(args[args.indexOf('--session-dir') + 1], dirname(session))
  assert.equal(args[args.indexOf('--tools') + 1], 'bash')
  assert.equal(args[args.indexOf('--extension') + 1], extension)
  for (const flag of ['--no-skills', '--no-prompt-templates', '--no-context-files', '--no-extensions']) assert.ok(args.includes(flag))
  for (const flag of ['--mode', '--print', '--no-session']) assert.ok(!args.includes(flag))
  const smoke = terminalArguments(pi, session)
  assert.ok(smoke.includes('--no-extensions') && !smoke.includes('--extension'))
})

const options = { marker: 'original-context-token', completion: 'tool-completed-token', command: 'pwsh -NoProfile -Command "Start-Sleep -Seconds 65"' }
const fixture = () => [
  { type: 'session', id: 'main-session', timestamp: '2026-10-12T12:00:00.000Z' },
  { type: 'message', timestamp: '2026-10-12T12:00:01.000Z', message: { role: 'user', content: [{ type: 'text', text: options.marker }] } },
  { type: 'message', timestamp: '2026-10-12T12:00:02.000Z', message: { role: 'assistant', content: [{ type: 'toolCall', id: 'call-1', name: 'bash', arguments: { command: options.command, timeout: 120 } }], stopReason: 'toolUse' } },
  { type: 'message', timestamp: '2026-10-12T12:01:07.000Z', message: { role: 'toolResult', toolName: 'bash', toolCallId: 'call-1', content: [{ type: 'text', text: options.completion }], isError: false } },
  { type: 'message', timestamp: '2026-10-12T12:01:08.000Z', message: { role: 'assistant', content: [{ type: 'text', text: 'Foreground work finished.' }], stopReason: 'stop' } },
]

test('Pi terminal proof reads authoritative persisted user, tool and assistant records', () => {
  const records = fixture()
  const bytes = Buffer.from(records.map(row => JSON.stringify(row)).join('\n') + '\n')
  assert.deepEqual(sessionRecords(bytes), records)
  assert.deepEqual(sessionRecords(Buffer.concat([bytes, Buffer.from('{"partial":')])), records)
  const task = inspectTask(records, options)
  assert.equal(task.sessionID, 'main-session')
  assert.equal(task.milliseconds, 65000)
  assert.equal(task.finalTimestamp, records.at(-1).timestamp)
  assert.equal(task.records.length, 4)
})

test('Pi terminal proof rejects false completion, failed tools and lost original context', () => {
  const mutations = [
    records => { records[0].id = '' },
    records => { records[1].message.content[0].text = 'not the marker' },
    records => { records[2].message.content.push(structuredClone(records[2].message.content[0])) },
    records => { records[2].message.content[0].name = 'read' },
    records => { records[2].message.content[0].arguments.command = 'echo simulated' },
    records => { records[2].message.content[0].arguments.timeout = 10 },
    records => { records[3].message.toolCallId = 'different-call' },
    records => { records[3].message.isError = true },
    records => { records[3].message.content[0].text = 'missing tool result token' },
    records => { records[3].timestamp = '2026-10-12T12:00:03.000Z' },
    records => { records[4].message.stopReason = 'aborted' },
    records => { records[4].message.content[0].text = 'Not finished.' },
    records => { records.push(structuredClone(records[1])) },
  ]
  for (const mutate of mutations) {
    const records = fixture()
    mutate(records)
    assert.throws(() => inspectTask(records, options))
  }
})
