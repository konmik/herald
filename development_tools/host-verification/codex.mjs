import { spawn, spawnSync } from 'node:child_process'
import { createInterface } from 'node:readline'
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { randomUUID } from 'node:crypto'
import assert from 'node:assert/strict'

const commandRecipe = seconds => `Run exactly one command pwsh -NoProfile -Command "Start-Sleep -Seconds ${seconds}" using exec_command with yield_time_ms=1000. If it returns a running session, poll it until it finishes. Then give your final reply Background work finished.`
const scenarioSpec = {
  Background: { instructions: commandRecipe(65), workStart: 'PreToolUse', workEnd: 'PostToolUse' },
  Subagent: { instructions: 'Spawn exactly one background subagent to run pwsh -NoProfile -Command "Start-Sleep -Seconds 65" and report finished. Immediately reply Background work started without waiting or closing the agent. When the agent finishes, give your final reply Background work finished.', workStart: 'SubagentStart', workEnd: 'SubagentStop' },
  CancelRestart: { instructions: commandRecipe(65), opening: commandRecipe(3), workStart: 'PreToolUse', workEnd: 'PostToolUse', delayPostToolUse: true },
}
const lifecycle = {
  commandStarted: message => message.method === 'item/started' && message.params?.item?.type === 'commandExecution',
  commandCompleted: (message, started) => message.method === 'item/completed' && message.params?.item?.type === 'commandExecution' && message.params.item.id === started.params.item.id,
  turnCompleted: (message, turnId) => message.method === 'turn/completed' && message.params?.turn?.id === turnId,
}
const lifecycleMethods = new Set(['item/started', 'item/completed', 'turn/completed'])
const [scenario, destination] = process.argv.slice(2)
assert.equal(process.platform, 'win32', 'Codex desktop verification requires Windows')
const spec = scenarioSpec[scenario]
assert.ok(Object.hasOwn(scenarioSpec, scenario), 'Scenario must be ' + Object.keys(scenarioSpec).join(', '))
assert.ok(destination)
const root = fileURLToPath(new URL('../..', import.meta.url))
const evidence = resolve(destination)
assert.ok(!existsSync(evidence), 'Use a new evidence directory')
mkdirSync(evidence, { recursive: true })
const scratch = mkdtempSync(join(process.env.LOCALAPPDATA, 'Temp/opencode/herald-codex-'))
const data = join(scratch, 'data')
mkdirSync(data)
const codexHome = join(scratch, 'codex')
mkdirSync(codexHome)
copyFileSync(join(process.env.CODEX_HOME ?? join(homedir(), '.codex'), 'auth.json'), join(codexHome, 'auth.json'))
const marker = 'herald-context-' + randomUUID()
writeFileSync(join(data, 'settings.json'), JSON.stringify({ quietMode: true, scheduleEnabled: false, summaryPrompt: 'Summarize the final outcome in one sentence. Include the unique herald-context marker from the original conversation. Do not run tools.' }))
const binary = join(root, 'native-announcer/target/debug/herald.exe')
const assets = join(root, 'native-announcer/resources')
const proof = join(evidence, 'hooks.jsonl')
const env = { ...process.env, CODEX_HOME: codexHome, HERALD_DATA: data, HERALD_BINARY: binary, HERALD_EXTERNAL_COMPANION: '1', PLUGIN_ROOT: join(root, 'codex-plugin'), PLUGIN_DATA: join(scratch, 'plugin'), HERALD_HOST_PROOF: proof }
if (spec.delayPostToolUse) env.HERALD_DELAY_POST_TOOL_USE_MS = '3000'
else delete env.HERALD_DELAY_POST_TOOL_USE_MS
const sleep = ms => new Promise(done => setTimeout(done, ms))
const rows = path => existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line)) : []
const native = spawn(binary, ['--isolated', '--assets', assets, '--test-seconds', '160', '--report', join(evidence, 'report.json'), '--snapshot', join(evidence, 'render.png')], { env, windowsHide: true, stdio: 'ignore' })
let host
let hostExited = false
let nativeExited = false
let sessionID
native.on('exit', () => { nativeExited = true })
writeFileSync(join(evidence, 'instance.json'), JSON.stringify({ nativePID: native.pid, binary, scratch, driverPID: process.pid }))
try {
  const command = `node "${join(root, 'development_tools/host-verification/codex-hook.mjs').replaceAll('\\', '/')}"`
  const args = ['app-server', '-c', 'features.hooks=true']
  const hooks = {}
  for (const name of ['UserPromptSubmit', 'SubagentStart', 'SubagentStop', 'PreToolUse', 'PostToolUse', 'Interrupt', 'Stop']) {
    const matcher = ['PreToolUse', 'PostToolUse'].includes(name) ? { matcher: 'Bash|Agent|spawn_agent|collaborationspawn_agent' } : {}
    hooks[name] = [{ ...matcher, hooks: [{ type: 'command', command, timeout: name === 'Interrupt' ? 3 : 15 }] }]
  }
  writeFileSync(join(codexHome, 'hooks.json'), JSON.stringify({ hooks }))
  assert.ok(process.env.CODEX_BINARY, 'Set CODEX_BINARY to the native Codex executable on Windows')
  host = spawn(process.env.CODEX_BINARY, args, { env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] })
  host.on('exit', () => { hostExited = true })
  let errors = ''
  host.stderr.on('data', chunk => { errors += chunk; writeFileSync(join(evidence, 'errors.txt'), errors) })
  let sequence = 0
  const pending = new Map()
  const observed = []
  const observers = []
  const failWaiters = error => {
    for (const { fail } of pending.values()) fail(error)
    pending.clear()
    for (const entry of observers.splice(0)) entry.fail(error)
  }
  const observe = (match, label) => {
    const found = observed.find(match)
    if (found) return Promise.resolve(found)
    return new Promise((done, fail) => {
      const entry = { match, done: message => { clearTimeout(timer); done(message) }, fail: error => { clearTimeout(timer); fail(error) } }
      const timer = setTimeout(() => {
        const index = observers.indexOf(entry)
        if (index >= 0) observers.splice(index, 1)
        entry.fail(new Error('Timed out waiting for ' + label))
      }, 60000)
      observers.push(entry)
    })
  }
  host.on('exit', () => failWaiters(new Error('Codex exited during a request')))
  createInterface({ input: host.stdout }).on('line', line => {
    const message = JSON.parse(line)
    writeFileSync(join(evidence, 'transcript.jsonl'), line + '\n', { flag: 'a' })
    if (message.id != null && pending.has(message.id)) {
      const { done, fail } = pending.get(message.id)
      pending.delete(message.id)
      if (message.error) fail(new Error(JSON.stringify(message.error)))
      else done(message.result)
    }
    if (!lifecycleMethods.has(message.method)) return
    observed.push(message)
    for (let index = observers.length - 1; index >= 0; index--) {
      if (!observers[index].match(message)) continue
      observers.splice(index, 1)[0].done(message)
    }
  })
  const request = (method, params) => new Promise((done, fail) => {
    const id = ++sequence
    const timer = setTimeout(() => { pending.delete(id); fail(new Error('Codex request timed out: ' + method)) }, 30000)
    pending.set(id, { done: result => { clearTimeout(timer); done(result) }, fail: error => { clearTimeout(timer); fail(error) } })
    host.stdin.write(JSON.stringify({ id, method, params }) + '\n')
  })
  await request('initialize', { clientInfo: { name: 'herald-verification', version: '1' }, capabilities: { experimentalApi: true } })
  host.stdin.write(JSON.stringify({ method: 'initialized', params: {} }) + '\n')
  const listed = await request('hooks/list', { cwds: [scratch] })
  writeFileSync(join(evidence, 'hook-list.json'), JSON.stringify(listed))
  const configured = listed.data.flatMap(entry => entry.hooks)
  assert.equal(configured.length, 7, 'All verification hooks must be registered')
  writeFileSync(join(codexHome, 'config.toml'), '[features]\nhooks = true\n' + configured.map(hook => `\n[hooks.state.${JSON.stringify(hook.key)}]\ntrusted_hash = ${JSON.stringify(hook.currentHash)}\n`).join(''))
  const created = await request('thread/start', { cwd: scratch, approvalPolicy: 'never', sandbox: 'danger-full-access' })
  sessionID = created.thread.id
  const turnInput = text => ({ threadId: sessionID, input: [{ type: 'text', text, text_elements: [] }] })
  let restartAt = 0
  let cancelled
  await request('turn/start', turnInput(`This is an isolated lifecycle test. Do not read or edit files. Original conversation marker ${marker}. ${spec.opening ?? spec.instructions} No other work.`))
  if (spec.opening) {
    const started = await observe(lifecycle.commandStarted, 'commandExecution start')
    const completed = await observe(message => lifecycle.commandCompleted(message, started), 'commandExecution completion')
    assert.equal(completed.params.item.status, 'completed')
    assert.equal(completed.params.item.exitCode, 0)
    assert.equal(typeof started.params.turnId, 'string')
    await request('turn/interrupt', { threadId: sessionID, turnId: started.params.turnId })
    const interrupted = await observe(message => lifecycle.turnCompleted(message, started.params.turnId), 'interrupted turn completion')
    assert.equal(interrupted.params.turn.status, 'interrupted')
    cancelled = completed
    writeFileSync(join(evidence, 'cancelled-command.json'), JSON.stringify(completed))
    const statePath = join(scratch, 'plugin/sessions', sessionID + '.json')
    const state = JSON.parse(readFileSync(statePath, 'utf8'))
    assert.ok(Object.hasOwn(state.work ?? {}, 'tool:' + completed.params.item.id), 'Interruption must leave the completed command tracked without its completion hook')
    copyFileSync(statePath, join(evidence, 'state-after-interrupt.json'))
    restartAt = Date.now()
    await request('turn/start', turnInput(`Continue the isolated lifecycle test. ${spec.instructions} Do not read or edit files. No other work.`))
  }
  const deadline = Date.now() + 240000
  let renderedID
  let seenActive = false
  let completed = false
  while (Date.now() < deadline) {
    assert.ok(!hostExited, 'Codex exited before verification')
    assert.ok(!nativeExited, 'Herald exited before verification')
    const history = rows(join(data, 'history.jsonl'))
    const hooks = rows(proof).filter(event => event.at >= restartAt)
    const workStart = hooks.find(event => event.hook_event_name === spec.workStart)
    const workEnd = hooks.find(event => event.hook_event_name === spec.workEnd)
    if (history.length) {
      assert.equal(history.length, 1)
      assert.ok(workStart && workEnd, 'Announcement requires real background work completion')
      assert.ok(workEnd.at - workStart.at >= 60000, 'Background work must last at least a minute')
      const finalStop = hooks.findLast(event => event.hook_event_name === 'Stop' && !event.agent_id)
      assert.ok(finalStop && finalStop.at >= workEnd.at, 'Announcement requires the final main Stop')
      assert.ok(Date.parse(history[0].shownAt) > workEnd.at)
      assert.equal(history[0].id, 'codex:' + sessionID + ':' + finalStop.turn_id)
      assert.ok(history[0].text.includes(marker), 'Fork must retain original conversation context')
      renderedID = history[0].id
      const queue = JSON.parse(readFileSync(join(data, 'queue.json'), 'utf8'))
      if (queue.some(entry => entry.id === renderedID)) seenActive = true
      if (seenActive && queue.length === 0) { completed = true; break }
    }
    await sleep(1000)
  }
  assert.ok(completed, 'Timed out waiting for one final rendered announcement')
  for (let i = 0; i < 290 && !nativeExited; i++) await sleep(1000)
  assert.ok(nativeExited)
  const report = JSON.parse(readFileSync(join(evidence, 'report.json'), 'utf8'))
  assert.equal(report.shown, 1)
  assert.equal(report.finished, 1)
  assert.equal(report.passiveWindow, true)
  assert.equal(report.focusChecked, true)
  assert.equal(report.focusUnchanged, true)
  assert.ok(report.decodedVideoFrames > 0)
  await request('thread/archive', { threadId: sessionID })
  const result = { passed: true, scenario, sessionID, announcementID: renderedID, originalContext: true }
  if (cancelled) result.cancellation = { commandID: cancelled.params.item.id, exitCode: cancelled.params.item.exitCode, turnID: cancelled.params.turnId }
  writeFileSync(join(evidence, 'result.json'), JSON.stringify(result))
  console.log(`PASS: Codex ${scenario}; ${cancelled ? 'cancelled completed command, then ' : ''}one final main announcement with original context`)
} finally {
  for (const name of ['history.jsonl', 'queue.json']) if (existsSync(join(data, name))) writeFileSync(join(evidence, name), readFileSync(join(data, name)))
  if (sessionID) {
    const statePath = join(scratch, 'plugin/sessions', sessionID + '.json')
    if (existsSync(statePath)) copyFileSync(statePath, join(evidence, 'state-final.json'))
  }
  const helpers = join(scratch, 'plugin/helpers.log')
  if (existsSync(helpers)) copyFileSync(helpers, join(evidence, 'helpers.log'))
  if (host && !hostExited) {
    spawnSync('taskkill', ['/PID', String(host.pid), '/T', '/F'], { windowsHide: true })
    await sleep(500)
  }
  if (!nativeExited) { native.kill(); await sleep(500) }
  rmSync(scratch, { recursive: true, force: true })
  writeFileSync(join(evidence, 'cleanup.json'), JSON.stringify({ scratchRemoved: !existsSync(scratch), nativeExited, hostExited }))
}
