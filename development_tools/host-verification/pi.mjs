import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export function parseArguments(args) {
  const [scenario, destination, ...flags] = args
  assert.ok(['Background', 'CancelRestart', 'Subagent'].includes(scenario), 'Scenario must be Background, CancelRestart, or Subagent')
  assert.ok(destination && !destination.startsWith('--'), 'Supply a new evidence directory')
  const options = { scenario, evidence: resolve(destination) }
  while (flags.length) {
    const flag = flags.shift()
    assert.ok(['--cli', '--extension'].includes(flag), 'Unknown option: ' + flag)
    const value = flags.shift()
    assert.ok(value && !value.startsWith('--'), 'Missing value for ' + flag)
    const key = flag.slice(2)
    assert.ok(!options[key], 'Duplicate option: ' + flag)
    options[key] = resolve(value)
  }
  return options
}

export function resolvePi(explicit, env = process.env) {
  const names = process.platform === 'win32' ? ['pi.exe', 'pi.cmd', 'pi.bat', 'pi.ps1', 'pi'] : ['pi']
  const binary = explicit ?? env.PI_BINARY ?? (env.PATH ?? env.Path ?? '').split(delimiter).flatMap(directory => names.map(name => join(directory.replaceAll('"', ''), name))).find(path => existsSync(path) && statSync(path).isFile())
  assert.ok(binary && existsSync(binary) && statSync(binary).isFile(), 'Set PI_BINARY or --cli to the installed Pi executable or JavaScript CLI')
  let cli = resolve(binary)
  if (/\.(cmd|ps1|bat)$/i.test(cli)) {
    const shim = readFileSync(cli, 'utf8')
    const target = shim.match(/(?:%dp0%|\$basedir)[\\/]([^"\r\n]+?\.js)/i)?.[1]
    assert.ok(target, 'Cannot resolve the Pi shim; use --cli with its JavaScript CLI')
    cli = resolve(dirname(cli), target.replaceAll('\\', '/'))
    assert.ok(existsSync(cli), 'Pi shim target does not exist')
  }
  return /\.[cm]?js$/i.test(cli) ? { command: process.execPath, args: [cli], cli } : { command: cli, args: [], cli }
}

export function installedExtension(agentDirectory) {
  const settingsPath = join(agentDirectory, 'settings.json')
  const settings = JSON.parse(readFileSync(settingsPath, 'utf8').replace(/^\ufeff/, ''))
  const sources = [...(settings.extensions ?? []), ...(settings.packages ?? []).map(item => typeof item === 'string' ? item : item.source)]
  for (const source of sources) {
    if (typeof source !== 'string' || /^[!-]/.test(source) || /^(npm:|git:|https?:)/.test(source)) continue
    const path = resolve(agentDirectory, source.replace(/^\+/, '').replace(/^~(?=[\\/])/, homedir()))
    const candidates = [path, join(path, 'index.ts'), join(path, 'pi-plugin/index.ts')]
    const manifest = join(path, 'package.json')
    if (existsSync(manifest)) for (const entry of JSON.parse(readFileSync(manifest, 'utf8')).pi?.extensions ?? []) candidates.push(resolve(path, entry))
    const extension = candidates.find(candidate => /[\\/]pi-plugin[\\/]index\.[cm]?[jt]s$/.test(candidate) && existsSync(candidate))
    if (extension) return { settingsPath, source, extension }
  }
  throw new Error('No installed Herald Pi extension found in user settings; use --extension for checkout verification')
}

export function toolAborted(event, toolCallId) {
  return event.type === 'tool_execution_end' && event.toolCallId === toolCallId && event.isError === true &&
    event.result?.content?.some(part => part.type === 'text' && part.text === 'Command aborted') === true
}

export async function verify(options) {
  assert.equal(process.platform, 'win32', 'Pi desktop verification requires an interactive Windows desktop')
  const root = fileURLToPath(new URL('../..', import.meta.url))
  const pi = resolvePi(options.cli)
  const integration = options.extension ? { extension: options.extension, scope: 'explicit extension' } : { ...installedExtension(process.env.PI_CODING_AGENT_DIR ?? join(homedir(), '.pi/agent')), scope: 'installed settings' }
  assert.ok(existsSync(integration.extension), 'Herald extension does not exist')
  const binary = resolve(process.env.HERALD_BINARY ?? join(root, 'native-announcer/target/debug/herald.exe'))
  const assets = resolve(process.env.HERALD_ASSETS ?? (process.env.HERALD_BINARY ? join(dirname(binary), '../resources') : join(root, 'native-announcer/resources')))
  assert.ok(existsSync(binary) && existsSync(assets), 'Herald binary and assets must exist')
  assert.ok(!existsSync(options.evidence), 'Use a new evidence directory')
  mkdirSync(options.evidence, { recursive: true })
  const scratch = mkdtempSync(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'herald-pi-'))
  const data = join(scratch, 'data')
  mkdirSync(data)
  const marker = 'herald-context-' + randomUUID()
  const summaryMarker = 'herald-summary-' + randomUUID()
  const completion = 'foreground-complete-' + randomUUID()
  const summaryPrompt = `Summarize the final completed outcome in one sentence. Include the unique herald-context marker from the original conversation and the exact token ${summaryMarker}. Do not run tools.`
  writeFileSync(join(data, 'settings.json'), JSON.stringify({ quietMode: true, scheduleEnabled: false, summaryPrompt }))
  const env = { ...process.env, HERALD_DATA: data, HERALD_BINARY: binary, HERALD_ASSETS: assets, HERALD_EXTERNAL_COMPANION: '1' }
  const events = []
  const pending = new Map()
  const processes = []
  const deadline = Date.now() + 240000
  let failure
  let sequence = 0
  let result = { passed: false, scenario: options.scenario, integration, unverified: ['Detached background work', ...(options.scenario === 'Subagent' ? [] : ['Subagent: run the Subagent scenario separately']), 'Audible output', 'Mouse dismissal'] }
  const save = (name, value) => writeFileSync(join(options.evidence, name), JSON.stringify(value, null, 2))
  const rows = name => existsSync(join(data, name)) ? readFileSync(join(data, name), 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line)) : []
  const identity = pid => {
    const query = spawnSync('pwsh', ['-NoProfile', '-Command', `$p=Get-Process -Id ${pid} -ErrorAction Stop; @{pid=$p.Id;start=$p.StartTime.ToUniversalTime().ToString('o');path=$p.Path}|ConvertTo-Json -Compress`], { encoding: 'utf8', windowsHide: true, timeout: 5000 })
    assert.equal(query.status, 0, query.stderr)
    return JSON.parse(query.stdout)
  }
  const own = (name, command, args, stdio) => {
    const child = spawn(command, args, { cwd: scratch, env, windowsHide: true, stdio })
    const record = { name, child, exited: false, command, args, started: new Date().toISOString() }
    processes.push(record)
    child.on('exit', (code, signal) => { record.exited = true; record.code = code; record.signal = signal })
    child.on('error', error => { record.exited = true; failure = error })
    assert.ok(child.pid, 'Failed to start ' + name)
    record.identity = identity(child.pid)
    record.sha256 = createHash('sha256').update(readFileSync(record.identity.path)).digest('hex')
    save('instance.json', { driverPID: process.pid, scratch, binary, assets, pi, integration, processes: processes.map(({ child: _child, ...entry }) => entry) })
    return record
  }
  const check = () => {
    if (failure) throw failure
    assert.ok(Date.now() < deadline, 'Pi verification exceeded 240 seconds')
    assert.ok(!processes.find(entry => entry.name === 'host')?.exited, 'Pi exited before verification completed')
  }
  const wait = async (predicate, label) => {
    while (true) {
      check()
      const value = predicate()
      if (value) return value
      await new Promise(done => setTimeout(done, 100))
      assert.ok(!processes.find(entry => entry.name === 'native')?.exited || label === 'native report', 'Herald exited before ' + label)
    }
  }
  const interrupted = () => { failure = new Error('Verification interrupted') }
  process.on('SIGINT', interrupted)
  process.on('SIGTERM', interrupted)
  try {
    const native = own('native', binary, ['--isolated', '--assets', assets, '--test-seconds', '210', '--report', join(options.evidence, 'report.json'), '--snapshot', join(options.evidence, 'render.png')], 'ignore')
    await wait(() => existsSync(join(data, 'inbox')), 'native inbox')
    const args = [...pi.args, '--mode', 'rpc', '--session', join(scratch, 'main.jsonl'), '--session-dir', scratch, '--no-skills', '--no-prompt-templates', '--tools', 'bash']
    if (options.extension) args.push('--no-extensions', '--extension', options.extension)
    const host = own('host', pi.command, args, ['pipe', 'pipe', 'pipe'])
    host.child.stderr.on('data', chunk => writeFileSync(join(options.evidence, 'stderr.txt'), chunk, { flag: 'a' }))
    host.child.on('exit', () => { for (const entry of pending.values()) entry.fail(new Error('Pi exited during RPC')); pending.clear() })
    let buffer = ''
    host.child.stdout.setEncoding('utf8')
    host.child.stdout.on('data', chunk => {
      buffer += chunk
      let newline
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline).replace(/\r$/, '')
        buffer = buffer.slice(newline + 1)
        try {
          const event = JSON.parse(line)
          events.push({ at: Date.now(), event })
          writeFileSync(join(options.evidence, 'transcript.jsonl'), JSON.stringify(events.at(-1)) + '\n', { flag: 'a' })
          if (event.type === 'extension_error') failure = new Error(JSON.stringify(event))
          if (event.type === 'response' && pending.has(event.id)) {
            const entry = pending.get(event.id)
            pending.delete(event.id)
            if (event.success) entry.done(event.data)
            else entry.fail(new Error(event.error))
          }
        } catch (error) { failure = error }
      }
    })
    const request = (type, fields = {}) => new Promise((done, fail) => {
      const id = String(++sequence)
      const timer = setTimeout(() => { pending.delete(id); fail(new Error('Pi RPC timed out: ' + type)) }, Math.min(30000, Math.max(1, deadline - Date.now())))
      pending.set(id, { done: value => { clearTimeout(timer); done(value) }, fail: error => { clearTimeout(timer); fail(error) } })
      const command = { id, type, ...fields }
      writeFileSync(join(options.evidence, 'requests.jsonl'), JSON.stringify({ at: Date.now(), command }) + '\n', { flag: 'a' })
      host.child.stdin.write(JSON.stringify(command) + '\n')
    })
    const state = await request('get_state')
    save('state.json', state)
    assert.equal(resolve(state.sessionFile), join(scratch, 'main.jsonl'))
    assert.ok(state.sessionId, 'Pi must have a persisted main session')
    const find = (offset, type) => events.slice(offset).find(row => row.event.type === type)
    const prompt = seconds => `This is an isolated lifecycle test. Do not read or edit files. Run exactly one foreground bash tool command pwsh -NoProfile -Command "Start-Sleep -Seconds ${seconds}; Write-Output '${completion}'" with a timeout of 120 seconds. Await the tool completely; do not detach it or spawn agents. Then reply with this exact sentence: "Foreground work finished." Do not echo any conversation markers. No other work.`
    if (options.scenario === 'CancelRestart') {
      const offset = events.length
      await request('prompt', { message: `Original conversation marker ${marker}. ${prompt(100)}` })
      const started = await wait(() => find(offset, 'tool_execution_start'), 'cancelled foreground tool')
      await wait(() => { assert.equal(rows('history.jsonl').length, 0, 'No announcement before abort'); return Date.now() - started.at >= 61000 }, 'one-minute cancellation')
      assert.ok(!find(offset, 'tool_execution_end'), 'Earlier task must still be running when aborted')
      await request('abort')
      await wait(() => find(offset, 'agent_settled'), 'aborted settlement')
      assert.ok(events.slice(offset).some(row => toolAborted(row.event, started.event.toolCallId)), 'Pi must report the foreground tool was really aborted')
      assert.equal(rows('history.jsonl').length, 0, 'Aborted work must stay silent')
      save('cancelled-messages.json', await request('get_messages'))
    }
    const offset = events.length
    let instructions = prompt(65)
    const childTranscript = join(scratch, 'child.jsonl')
    if (options.scenario === 'Subagent') {
      const quote = value => "'" + value.replaceAll("'", "''") + "'"
      const childPrompt = `This is an isolated child-agent test. ${prompt(65)}`
      const childArgs = [...pi.args, '--mode', 'json', '--no-session', '--no-skills', '--no-prompt-templates', '--tools', 'bash']
      if (options.extension) childArgs.push('--no-extensions', '--extension', options.extension)
      childArgs.push(childPrompt)
      const command = `& ${[pi.command, ...childArgs].map(quote).join(' ')} | Set-Content -LiteralPath ${quote(childTranscript)} -Encoding utf8NoBOM; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; Write-Output ${quote(completion)}`
      const encoded = Buffer.from(command, 'utf16le').toString('base64')
      instructions = `This is an isolated lifecycle test. Run exactly one bash tool command pwsh -NoProfile -EncodedCommand ${encoded} with a timeout of 150 seconds. It runs one child Pi agent and waits for its work. Do not run other commands or edit files. Await the command completely, then reply with the exact sentence "Foreground work finished." Do not echo any conversation markers.`
      save('child-invocation.json', { command: pi.command, args: childArgs, transcript: childTranscript })
    }
    await request('prompt', { message: `${options.scenario !== 'CancelRestart' ? `Original conversation marker ${marker}. ` : ''}${instructions}` })
    const settled = await wait(() => {
      const event = find(offset, 'agent_settled')
      if (!event) assert.equal(rows('history.jsonl').length, 0, 'No announcement before final settlement')
      return event
    }, 'final settlement')
    const work = events.slice(offset).filter(row => row.event.type === 'tool_execution_start')
    assert.equal(work.length, 1, 'Exactly one foreground tool must run')
    assert.equal(work[0].event.toolName, 'bash')
    assert.match(work[0].event.args.command, options.scenario === 'Subagent' ? /-EncodedCommand/ : /Start-Sleep -Seconds 65/)
    const end = events.slice(offset).find(row => row.event.type === 'tool_execution_end' && row.event.toolCallId === work[0].event.toolCallId)
    assert.ok(end && !end.event.isError && end.at - work[0].at >= 65000, 'Real foreground work must finish after at least 65 seconds')
    assert.ok(JSON.stringify(end.event.result).includes(completion), 'Foreground tool must return its completion token')
    assert.ok(settled.at >= end.at)
    if (options.scenario === 'Subagent') {
      const childRows = readFileSync(childTranscript, 'utf8').replace(/^\ufeff/, '').split('\n').filter(Boolean).map(line => JSON.parse(line))
      assert.ok(childRows.some(row => row.type === 'agent_settled'), 'The real child Pi agent must settle')
      assert.equal(childRows.filter(row => row.type === 'tool_execution_start').length, 1)
      assert.ok(childRows.some(row => row.type === 'tool_execution_start' && /Start-Sleep -Seconds 65/.test(row.args.command)))
      copyFileSync(childTranscript, join(options.evidence, 'child-transcript.jsonl'))
      result.childSettled = true
    }
    const before = await request('get_messages')
    assert.ok(JSON.stringify(before).includes(marker), 'Original context must exist in the real main conversation')
    const finalMessage = before.messages.at(-1)
    assert.equal(finalMessage.role, 'assistant')
    assert.ok(!['aborted', 'error'].includes(finalMessage.stopReason), 'Fresh task must complete successfully')
    assert.equal(finalMessage.content.filter(block => block.type === 'text').map(block => block.text).join('').trim(), 'Foreground work finished.')
    save('main-before.json', before)
    copyFileSync(state.sessionFile, join(options.evidence, 'session-before.jsonl'))
    const shown = await wait(() => rows('history.jsonl')[0], 'rendered announcement')
    assert.equal(shown.sessionID, 'pi:' + state.sessionId)
    assert.equal(shown.source, 'pi')
    assert.ok(shown.id.startsWith('pi:' + state.sessionId + ':'), 'Announcement must belong to the main Pi session')
    assert.ok(Date.parse(shown.shownAt) >= settled.at, 'Announcement must be shown after settlement')
    assert.ok(shown.text.includes(marker) && shown.text.includes(summaryMarker), 'Summary fork must retain original context and use scratch summaryPrompt')
    if (existsSync(join(data, 'queue.json'))) copyFileSync(join(data, 'queue.json'), join(options.evidence, 'queue-during.json'))
    await wait(() => existsSync(join(data, 'queue.json')) && JSON.parse(readFileSync(join(data, 'queue.json'), 'utf8')).length === 0, 'drained queue')
    const after = await request('get_messages')
    save('main-after.json', after)
    copyFileSync(state.sessionFile, join(options.evidence, 'session-after.jsonl'))
    assert.deepEqual(after, before, 'Summary generation must not change main messages')
    assert.equal(readFileSync(join(options.evidence, 'session-before.jsonl'), 'utf8'), readFileSync(state.sessionFile, 'utf8'), 'Summary fork must not append main session entries')
    assert.ok(!JSON.stringify(after).includes(summaryMarker) && !JSON.stringify(after).includes(summaryPrompt), 'Summary prompt and reply must stay out of the main transcript')
    const finalState = await request('get_state')
    assert.equal(finalState.isStreaming, false)
    assert.equal(finalState.pendingMessageCount, 0)
    await wait(() => native.exited, 'native report')
    assert.equal(native.code, 0)
    assert.equal(rows('history.jsonl').length, 1, 'Exactly one main announcement')
    assert.deepEqual(JSON.parse(readFileSync(join(data, 'queue.json'), 'utf8')), [])
    assert.ok(!existsSync(join(data, 'errors.log')), 'Native runtime must have no errors')
    const report = JSON.parse(readFileSync(join(options.evidence, 'report.json'), 'utf8'))
    assert.equal(report.shown, 1)
    assert.equal(report.finished, 1)
    for (const key of ['passiveWindow', 'focusChecked', 'focusUnchanged']) assert.equal(report[key], true)
    assert.ok(report.decodedVideoFrames > 0 && statSync(join(options.evidence, 'render.png')).size > 0)
    result = { ...result, passed: true, sessionID: state.sessionId, announcementID: shown.id, originalContext: true, mainTranscriptUnchanged: true, foregroundMilliseconds: end.at - work[0].at }
  } catch (error) {
    result.error = error.stack ?? String(error)
    throw error
  } finally {
    process.off('SIGINT', interrupted)
    process.off('SIGTERM', interrupted)
    for (const entry of pending.values()) entry.fail(new Error('Verification cleanup'))
    pending.clear()
    const cleanupErrors = []
    for (const name of ['history.jsonl', 'queue.json', 'settings.json', 'errors.log']) {
      try { if (existsSync(join(data, name))) copyFileSync(join(data, name), join(options.evidence, name)) }
      catch (error) { cleanupErrors.push(error.message) }
    }
    for (const entry of [...processes].reverse()) {
      try {
        if (entry.exited) continue
        if (entry.name === 'host') entry.child.stdin.end()
        for (let i = 0; i < 20 && !entry.exited; i++) await new Promise(done => setTimeout(done, 100))
        if (!entry.exited) {
          const current = identity(entry.child.pid)
          assert.deepEqual(current, entry.identity, 'Refusing to stop a reused PID')
          spawnSync('taskkill', ['/PID', String(entry.child.pid), '/T', '/F'], { windowsHide: true, timeout: 5000 })
          for (let i = 0; i < 30 && !entry.exited; i++) await new Promise(done => setTimeout(done, 100))
        }
      } catch (error) { cleanupErrors.push(error.message) }
    }
    const processExited = processes.every(entry => entry.exited)
    try { if (processExited) rmSync(scratch, { recursive: true, force: true }) }
    catch (error) { cleanupErrors.push(error.message) }
    const cleanup = { scratchRemoved: !existsSync(scratch), processExited, hostExited: processes.filter(entry => entry.name === 'host').every(entry => entry.exited), nativeExited: processes.filter(entry => entry.name === 'native').every(entry => entry.exited), errors: cleanupErrors, processes: processes.map(({ child: _child, ...entry }) => entry) }
    if (!cleanup.scratchRemoved || !processExited || cleanupErrors.length) result.passed = false
    save('cleanup.json', cleanup)
    save('result.json', result)
    assert.ok(cleanup.scratchRemoved && cleanup.processExited && !cleanupErrors.length, 'Owned processes and scratch must be cleaned up')
  }
  console.log(`PASS: Pi ${options.scenario}; one final main announcement; original context retained; main transcript unchanged`)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await verify(parseArguments(process.argv.slice(2))) }
  catch (error) { console.error(error.message); process.exitCode = 1 }
}
