import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { test } from 'node:test'
import { installedExtension, parseArguments, resolvePi, toolAborted } from '../host-verification/pi.mjs'

test('Pi verification validates scenarios and options without starting hosts', () => {
  assert.deepEqual(parseArguments(['Background', 'proof']), { scenario: 'Background', evidence: resolve('proof') })
  assert.deepEqual(parseArguments(['CancelRestart', 'proof', '--cli', 'cli.js', '--extension', 'index.ts']), { scenario: 'CancelRestart', evidence: resolve('proof'), cli: resolve('cli.js'), extension: resolve('index.ts') })
  for (const args of [[], ['Background'], ['Background', '--cli'], ['Background', 'proof', '--cli'], ['Background', 'proof', '--unknown', 'value'], ['Background', 'proof', '--extension', 'a', '--extension', 'b']]) assert.throws(() => parseArguments(args))
  assert.deepEqual(parseArguments(['Subagent', 'proof']), { scenario: 'Subagent', evidence: resolve('proof') })
  assert.throws(() => parseArguments(['Unknown', 'proof']), /Scenario must be/)
})

test('Pi cancellation proof requires an aborted result for the actual foreground tool', () => {
  const event = { type: 'tool_execution_end', toolCallId: 'foreground', isError: true, result: { content: [{ type: 'text', text: 'Command aborted' }] } }
  assert.equal(toolAborted(event, 'foreground'), true)
  for (const invalid of [{ ...event, toolCallId: 'other' }, { ...event, isError: false }, { ...event, result: undefined }, { ...event, type: 'message_end' }, { ...event, result: { content: [{ type: 'text', text: 'Command failed' }] } }]) {
    assert.equal(toolAborted(invalid, 'foreground'), false)
  }
})

test('Pi verification resolves explicit JavaScript and installed PATH shims', t => {
  const directory = mkdtempSync(join(tmpdir(), 'herald-pi-cli-'))
  t.after(() => rmSync(directory, { recursive: true, force: true }))
  const cli = join(directory, 'cli.js')
  writeFileSync(cli, '')
  const expected = { command: process.execPath, args: [cli], cli }
  assert.deepEqual(resolvePi(cli), expected)
  assert.deepEqual(resolvePi(undefined, { PI_BINARY: cli }), expected)
  if (process.platform === 'win32') {
    writeFileSync(join(directory, 'pi.cmd'), '@"node" "%dp0%\\cli.js" %*')
    assert.deepEqual(resolvePi(undefined, { PATH: directory }), expected)
    writeFileSync(join(directory, 'pi.cmd'), '@echo invalid')
    assert.throws(() => resolvePi(undefined, { PATH: directory }), /Cannot resolve the Pi shim/)
  }
  assert.throws(() => resolvePi(undefined, { PATH: '' }), /Set PI_BINARY/)
  assert.throws(() => resolvePi(directory), /Set PI_BINARY/)
})

test('Pi verification discovers installed settings registrations without changing them', t => {
  const directory = mkdtempSync(join(tmpdir(), 'herald-pi-settings-'))
  t.after(() => rmSync(directory, { recursive: true, force: true }))
  const extension = join(directory, 'pi-plugin/index.ts')
  mkdirSync(join(directory, 'pi-plugin'))
  writeFileSync(extension, '')
  const settingsPath = join(directory, 'settings.json')
  writeFileSync(settingsPath, JSON.stringify({ extensions: ['./pi-plugin/index.ts'] }))
  assert.deepEqual(installedExtension(directory), { settingsPath, source: './pi-plugin/index.ts', extension })
  writeFileSync(settingsPath, JSON.stringify({ packages: [{ source: directory }] }))
  assert.equal(installedExtension(directory).extension, extension)
  writeFileSync(settingsPath, JSON.stringify({ extensions: ['-./pi-plugin/index.ts'] }))
  assert.throws(() => installedExtension(directory), /No installed Herald Pi extension/)
})
