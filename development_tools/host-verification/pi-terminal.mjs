import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { installedExtension, parseArguments as parseHostArguments, resolvePi } from './pi.mjs'

export function parseArguments(args) {
  assert.ok(['Background', 'Smoke'].includes(args[0]), 'Scenario must be Background or Smoke')
  return { ...parseHostArguments(['Background', ...args.slice(1)]), scenario: args[0] }
}

export function terminalArguments(pi, session, extension, prompt) {
  return ['-Xallow-non-tty', '-Xplain', '--', pi.command, ...pi.args,
    '--session', session, '--session-dir', dirname(session), '--no-skills', '--no-prompt-templates',
    '--no-context-files', '--tools', 'bash', '--no-extensions',
    ...(extension ? ['--extension', extension] : []), ...(prompt ? [prompt] : [])]
}

export function sessionRecords(bytes) {
  const text = bytes.toString('utf8')
  return text.slice(0, text.lastIndexOf('\n') + 1).split('\n').filter(Boolean).map(line => JSON.parse(line))
}

const textContent = message => typeof message.content === 'string' ? message.content : (message.content ?? []).filter(block => block.type === 'text').map(block => block.text).join('')

export function inspectTask(records, { marker, completion, command }) {
  const header = records[0]
  assert.ok(header?.type === 'session' && header.id, 'Persisted main Pi session must have an ID')
  const messages = records.filter(row => row.type === 'message')
  const users = messages.filter(row => row.message.role === 'user')
  assert.equal(users.length, 1, 'Exactly one original user prompt')
  assert.ok(textContent(users[0].message).includes(marker), 'Original marker must be in the main prompt')
  const calls = messages.flatMap(row => (row.message.content instanceof Array ? row.message.content : []).filter(block => block.type === 'toolCall').map(block => ({ row, block })))
  assert.equal(calls.length, 1, 'Exactly one awaited tool call')
  const call = calls[0]
  assert.equal(call.block.name, 'bash')
  assert.equal(call.block.arguments.command, command)
  assert.equal(call.block.arguments.timeout, 120)
  const results = messages.filter(row => row.message.role === 'toolResult')
  assert.equal(results.length, 1, 'Exactly one authoritative tool result')
  const tool = results[0]
  assert.equal(tool.message.toolCallId, call.block.id)
  assert.equal(tool.message.toolName, 'bash')
  assert.equal(tool.message.isError, false)
  assert.ok(textContent(tool.message).includes(completion), 'Awaited tool must return its unique completion token')
  const milliseconds = Date.parse(tool.timestamp) - Date.parse(call.row.timestamp)
  assert.ok(milliseconds >= 65000, 'Foreground tool must really take at least 65 seconds')
  const final = messages.at(-1)
  assert.equal(final.message.role, 'assistant')
  assert.equal(final.message.stopReason, 'stop', 'Main task must finish successfully')
  assert.equal(textContent(final.message).trim(), 'Foreground work finished.')
  assert.ok(Date.parse(final.timestamp) >= Date.parse(tool.timestamp))
  return { sessionID: header.id, milliseconds, finalTimestamp: final.timestamp, records: messages }
}

export async function verify(options) {
  assert.equal(process.platform, 'win32', 'Pi terminal verification requires Windows')
  const smoke = options.scenario === 'Smoke'
  const root = fileURLToPath(new URL('../..', import.meta.url))
  const pi = resolvePi(options.cli)
  const winpty = resolve(process.env.WINPTY_BINARY ?? join(process.env.ProgramFiles ?? 'C:/Program Files', 'Git/usr/bin/winpty.exe'))
  assert.ok(existsSync(winpty), 'Set WINPTY_BINARY to an existing winpty.exe')
  const integration = smoke ? { scope: 'no extensions PTY smoke' } : options.extension
    ? { extension: options.extension, scope: 'explicit extension' }
    : { ...installedExtension(process.env.PI_CODING_AGENT_DIR ?? join(homedir(), '.pi/agent')), scope: 'installed settings' }
  if (!smoke) assert.ok(existsSync(integration.extension), 'Herald extension does not exist')
  const binary = resolve(process.env.HERALD_BINARY ?? join(root, 'native-announcer/target/debug/herald.exe'))
  const assets = resolve(process.env.HERALD_ASSETS ?? (process.env.HERALD_BINARY ? join(dirname(binary), '../resources') : join(root, 'native-announcer/resources')))
  if (!smoke) assert.ok(existsSync(binary) && existsSync(assets), 'Herald binary and assets must exist')
  assert.ok(!existsSync(options.evidence), 'Use a new evidence directory')
  mkdirSync(options.evidence, { recursive: true })
  const scratch = mkdtempSync(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'herald-pi-terminal-'))
  const data = join(scratch, 'data')
  const session = join(scratch, 'main.jsonl')
  mkdirSync(data)
  const marker = 'herald-context-' + randomUUID()
  const summaryMarker = 'herald-summary-' + randomUUID()
  const completion = 'foreground-complete-' + randomUUID()
  const command = `pwsh -NoProfile -Command "Start-Sleep -Seconds 65; Write-Output '${completion}'"`
  const prompt = `Original conversation marker ${marker}. This is an isolated lifecycle test. Do not read or edit files. Run exactly one foreground bash tool command ${command} with a timeout of 120 seconds. Await it completely; do not detach it or spawn agents. Then reply with exactly "Foreground work finished." Do not echo conversation markers. No other work.`
  const summaryPrompt = `Summarize the final completed outcome in one sentence. Include the unique herald-context marker from the original conversation and the exact token ${summaryMarker}. Do not run tools.`
  writeFileSync(join(data, 'settings.json'), JSON.stringify({ quietMode: true, scheduleEnabled: false, summaryPrompt }))
  const env = { ...process.env, TERM: 'xterm-256color', HERALD_DATA: data, HERALD_BINARY: binary, HERALD_ASSETS: assets, HERALD_EXTERNAL_COMPANION: '1' }
  delete env.PI_SESSION_ID
  delete env.PI_SESSION_FILE
  delete env.HERALD_PI_SUBAGENT
  const totalDeadline = Date.now() + (smoke ? 30000 : 240000)
  const deadline = totalDeadline - 20000
  const processes = []
  let failure
  let transcript = ''
  let result = { passed: false, scenario: options.scenario, integration, transport: 'winpty interactive TUI', unverified: ['Audible output', 'Mouse dismissal', 'Subagents', 'Cancellation/restart', 'Detached background work', ...(smoke ? ['Native playback', 'Model generation'] : [])] }
  const save = (name, value) => writeFileSync(join(options.evidence, name), JSON.stringify(value, null, 2))
  const pause = milliseconds => new Promise(done => setTimeout(done, Math.min(milliseconds, Math.max(0, totalDeadline - Date.now()))))
  const queryOptions = () => ({ encoding: 'utf8', windowsHide: true, timeout: Math.min(5000, Math.max(1, totalDeadline - Date.now())) })
  const identity = pid => {
    const query = spawnSync('pwsh', ['-NoProfile', '-Command', `$p=Get-Process -Id ${pid} -ErrorAction SilentlyContinue; if($p){@{pid=$p.Id;start=$p.StartTime.ToUniversalTime().ToString('o');path=$p.Path}|ConvertTo-Json -Compress}else{'null'}`], queryOptions())
    assert.equal(query.status, 0, query.stderr)
    return JSON.parse(query.stdout)
  }
  const captureTree = entry => {
    assert.deepEqual(identity(entry.child.pid), entry.identity, 'Owned process identity changed')
    const query = spawnSync('pwsh', ['-NoProfile', '-Command', `$all=Get-CimInstance Win32_Process; $ids=@(${entry.child.pid}); $found=@(); do{$next=@($all|Where-Object {$_.ParentProcessId -in $ids -and $_.ProcessId -notin $ids}); $ids+=@($next.ProcessId); $found+=$next}while($next.Count); @($found|ForEach-Object {$p=Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; if($p){@{identity=@{pid=$p.Id;start=$p.StartTime.ToUniversalTime().ToString('o');path=$p.Path};commandLine=$_.CommandLine;parentPID=$_.ParentProcessId}}})|ConvertTo-Json -Depth 4 -Compress`], queryOptions())
    assert.equal(query.status, 0, query.stderr)
    const descendants = query.stdout.trim() ? JSON.parse(query.stdout) : []
    entry.descendants ??= []
    for (const item of [descendants].flat()) if (!entry.descendants.some(saved => saved.identity.pid === item.identity.pid && saved.identity.start === item.identity.start)) entry.descendants.push(item)
    manifest()
    return entry.descendants
  }
  const manifest = () => save('instance.json', { driverPID: process.pid, scratch, session, binary, assets, pi, winpty, integration, processes: processes.map(({ child: _child, ...entry }) => entry) })
  const own = (name, executable, args) => {
    for (const channel of ['stdout', 'stderr']) writeFileSync(join(options.evidence, `${name}-${channel}.txt`), '')
    const child = spawn(executable, args, { cwd: scratch, env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] })
    const entry = { name, child, executable, args, exited: false }
    processes.push(entry)
    child.on('exit', (code, signal) => { entry.exited = true; entry.code = code; entry.signal = signal })
    child.on('error', error => { entry.exited = true; failure = error })
    child.stdin.on('error', error => { if (!entry.closing) failure = error })
    for (const channel of ['stdout', 'stderr']) child[channel].on('data', chunk => {
      writeFileSync(join(options.evidence, `${name}-${channel}.txt`), chunk, { flag: 'a' })
      if (name === 'host' && channel === 'stdout') transcript += chunk.toString('utf8')
    })
    assert.ok(child.pid, 'Failed to start ' + name)
    entry.identity = identity(child.pid)
    assert.ok(entry.identity, 'Owned process exited during launch')
    assert.equal(resolve(entry.identity.path).toLowerCase(), resolve(executable).toLowerCase())
    entry.sha256 = createHash('sha256').update(readFileSync(entry.identity.path)).digest('hex')
    manifest()
    return entry
  }
  const rows = name => existsSync(join(data, name)) ? sessionRecords(readFileSync(join(data, name))) : []
  const wait = async (predicate, label) => {
    while (true) {
      if (failure) throw failure
      assert.ok(Date.now() < deadline, 'Timed out waiting for ' + label)
      assert.ok(!processes.find(entry => entry.name === 'host')?.exited, 'Pi TUI exited before ' + label)
      const value = predicate()
      if (value) return value
      assert.ok(!processes.find(entry => entry.name === 'native')?.exited || label === 'native report', 'Herald exited before ' + label)
      await pause(50)
    }
  }
  const input = (host, value) => {
    writeFileSync(join(options.evidence, 'input.jsonl'), JSON.stringify({ at: new Date().toISOString(), value }) + '\n', { flag: 'a' })
    host.child.stdin.write(value)
  }
  const interrupted = () => { failure = new Error('Verification interrupted') }
  process.on('SIGINT', interrupted)
  process.on('SIGTERM', interrupted)
  try {
    let native
    if (!smoke) {
      native = own('native', binary, ['--isolated', '--assets', assets, '--test-seconds', '210', '--report', join(options.evidence, 'report.json'), '--snapshot', join(options.evidence, 'render.png')])
      await wait(() => existsSync(join(data, 'inbox')), 'native inbox')
    }
    const host = own('host', winpty, terminalArguments(pi, session, integration.extension, smoke ? undefined : prompt))
    await wait(() => transcript.includes('\x1b]0;') && /─{10}/.test(transcript) && /\(auto\)|ctrl|esc/i.test(transcript), 'interactive terminal rendering')
    const descendants = captureTree(host)
    assert.ok(descendants.some(item => resolve(item.identity.path).toLowerCase() === resolve(pi.command).toLowerCase() && item.commandLine.includes(pi.cli)), 'Real installed Pi must run inside the owned PTY tree')
    save('terminal-ready.json', { interactive: true, stdoutBytes: Buffer.byteLength(transcript), at: new Date().toISOString() })
    result.interactiveRendered = true
    if (smoke) {
      input(host, '/quit\r')
      for (let i = 0; i < 100 && !host.exited; i++) await pause(50)
      assert.ok(host.exited && host.code === 0, 'Pi must exit cleanly through its TUI input pipe')
      result = { ...result, passed: true, interactiveRendered: true, inputShutdown: true }
    } else {
      const completed = await wait(() => {
        if (!existsSync(session)) return false
        const bytes = readFileSync(session)
        const records = sessionRecords(bytes)
        const messages = records.filter(row => row.type === 'message')
        const final = messages.at(-1)
        if (final?.message.role === 'assistant' && ['aborted', 'error'].includes(final.message.stopReason)) throw new Error('Main Pi task failed: ' + textContent(final.message))
        if (final?.message.role !== 'assistant' || textContent(final.message).trim() !== 'Foreground work finished.') {
          assert.equal(rows('history.jsonl').length, 0, 'No announcement before main task completion')
          return false
        }
        return { bytes, task: inspectTask(records, { marker, completion, command }) }
      }, 'authoritative final main session records')
      writeFileSync(join(options.evidence, 'session-before.jsonl'), completed.bytes)
      save('main-before.json', completed.task)
      assert.equal(rows('history.jsonl').length, 0, 'Capture completion before summary is rendered')
      const shown = await wait(() => rows('history.jsonl')[0], 'rendered announcement')
      assert.equal(shown.source, 'pi')
      assert.equal(shown.sessionID, 'pi:' + completed.task.sessionID)
      assert.ok(shown.id.startsWith('pi:' + completed.task.sessionID + ':'))
      assert.ok(Date.parse(shown.shownAt) >= Date.parse(completed.task.finalTimestamp))
      assert.ok(shown.text.includes(marker) && shown.text.includes(summaryMarker), 'Forked summary must retain original context and use isolated summary token')
      if (existsSync(join(data, 'queue.json'))) copyFileSync(join(data, 'queue.json'), join(options.evidence, 'queue-during.json'))
      await wait(() => existsSync(join(data, 'queue.json')) && JSON.parse(readFileSync(join(data, 'queue.json'), 'utf8')).length === 0, 'drained queue')
      const after = readFileSync(session)
      writeFileSync(join(options.evidence, 'session-after.jsonl'), after)
      assert.ok(after.equals(completed.bytes), 'Summary fork must leave main session byte-for-byte unchanged')
      assert.ok(!after.toString('utf8').includes(summaryMarker) && !after.toString('utf8').includes(summaryPrompt))
      save('main-after.json', inspectTask(sessionRecords(after), { marker, completion, command }))
      await wait(() => native.exited, 'native report')
      assert.equal(native.code, 0)
      assert.equal(rows('history.jsonl').length, 1, 'Exactly one main announcement')
      assert.deepEqual(JSON.parse(readFileSync(join(data, 'queue.json'), 'utf8')), [])
      assert.ok(readFileSync(session).equals(completed.bytes), 'Main transcript must stay unchanged through playback completion')
      assert.ok(!existsSync(join(data, 'errors.log')), 'No native runtime errors')
      const report = JSON.parse(readFileSync(join(options.evidence, 'report.json'), 'utf8'))
      assert.equal(report.shown, 1)
      assert.equal(report.finished, 1)
      for (const key of ['passiveWindow', 'focusChecked', 'focusUnchanged']) assert.equal(report[key], true)
      assert.ok(report.decodedVideoFrames > 0 && statSync(join(options.evidence, 'render.png')).size > 0)
      result = { ...result, passed: true, sessionID: completed.task.sessionID, announcementID: shown.id, foregroundMilliseconds: completed.task.milliseconds, originalContext: true, mainTranscriptUnchanged: true, nativeAnimationAndFocus: true }
    }
  } catch (error) {
    result.error = error.stack ?? String(error)
    throw error
  } finally {
    const errors = []
    for (const name of ['history.jsonl', 'queue.json', 'settings.json', 'errors.log']) {
      try { if (existsSync(join(data, name))) copyFileSync(join(data, name), join(options.evidence, name)) }
      catch (error) { errors.push(error.message) }
    }
    try { if (existsSync(session)) copyFileSync(session, join(options.evidence, 'session-final.jsonl')) }
    catch (error) { errors.push(error.message) }
    for (const entry of [...processes].reverse()) {
      try {
        entry.closing = true
        if (!entry.exited && entry.name === 'host') {
          input(entry, '/quit\r')
          for (let i = 0; i < 40 && !entry.exited; i++) await pause(50)
          if (!entry.exited) input(entry, '\x03')
          for (let i = 0; i < 20 && !entry.exited; i++) await pause(50)
          if (!entry.exited) input(entry, '\x03')
          for (let i = 0; i < 20 && !entry.exited; i++) await pause(50)
        }
        if (!entry.exited) {
          assert.deepEqual(identity(entry.child.pid), entry.identity, 'Refusing to stop a reused PID or different executable')
          captureTree(entry)
          const killed = spawnSync('taskkill', ['/PID', String(entry.child.pid), '/T', '/F'], queryOptions())
          save(entry.name + '-forced-cleanup.json', { identity: entry.identity, status: killed.status, stdout: killed.stdout, stderr: killed.stderr })
          for (let i = 0; i < 40 && !entry.exited; i++) await pause(50)
          assert.ok(entry.exited, 'Owned process tree did not exit')
        }
        for (const descendant of [...(entry.descendants ?? [])].reverse()) {
          const current = identity(descendant.identity.pid)
          if (!current || current.pid !== descendant.identity.pid || current.start !== descendant.identity.start || current.path !== descendant.identity.path) {
            descendant.exited = true
            continue
          }
          assert.deepEqual(current, descendant.identity, 'Refusing to stop a reused descendant PID')
          const killed = spawnSync('pwsh', ['-NoProfile', '-Command', `$p=Get-Process -Id ${current.pid} -ErrorAction Stop; if($p.StartTime.ToUniversalTime().ToString('o') -ne '${current.start}' -or $p.Path -ne '${current.path.replaceAll("'", "''")}'){throw 'Process identity changed'}; Stop-Process -Id $p.Id -Force -ErrorAction Stop`], queryOptions())
          assert.equal(killed.status, 0, killed.stderr)
          descendant.forcedCleanup = true
          descendant.exited = !identity(descendant.identity.pid)
          assert.ok(descendant.exited, 'Owned descendant did not exit')
        }
      } catch (error) { errors.push(error.message) }
    }
    const processExited = processes.every(entry => entry.exited && (entry.descendants ?? []).every(descendant => descendant.exited))
    try { if (processExited) rmSync(scratch, { recursive: true, force: true }) }
    catch (error) { errors.push(error.message) }
    process.off('SIGINT', interrupted)
    process.off('SIGTERM', interrupted)
    const cleanup = { scratchRemoved: !existsSync(scratch), processExited, hostExited: processes.filter(entry => entry.name === 'host').every(entry => entry.exited), nativeExited: processes.filter(entry => entry.name === 'native').every(entry => entry.exited), errors }
    const host = processes.find(entry => entry.name === 'host')
    if (host) {
      result.terminalExitCode = host.code
      result.terminalShutdownClean = host.code === 0
      if (host.code !== 0) result.unverified.push('Clean winpty shutdown')
    }
    if (!cleanup.scratchRemoved || !processExited || errors.length) result.passed = false
    manifest()
    save('cleanup.json', cleanup)
    save('result.json', result)
    assert.ok(cleanup.scratchRemoved && processExited && !errors.length, 'Owned processes and scratch must be cleaned up')
  }
  console.log(`PASS: Pi interactive TUI ${options.scenario}`)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await verify(parseArguments(process.argv.slice(2))) }
  catch (error) { console.error(error.message); process.exitCode = 1 }
}
