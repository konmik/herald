import { appendFileSync, readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
const chunks = []
for await (const chunk of process.stdin) chunks.push(chunk)
const input = Buffer.concat(chunks).toString('utf8')
const event = JSON.parse(input)
appendFileSync(process.env.HERALD_HOST_PROOF, JSON.stringify({ at: Date.now(), ...event }) + '\n')
const delay = event.hook_event_name === 'PostToolUse' ? Number(process.env.HERALD_DELAY_POST_TOOL_USE_MS) : 0
if (delay > 0) await new Promise(done => setTimeout(done, delay))
const config = JSON.parse(readFileSync(process.env.PLUGIN_ROOT + '/hooks/hooks.json', 'utf8'))
const command = config.hooks[event.hook_event_name][0].hooks[0].commandWindows
const env = { ...process.env }
delete env.HERALD_DELAY_POST_TOOL_USE_MS
const spawnOptions = { input, encoding: 'utf8', timeout: 15000, windowsHide: true, env }
const result = process.platform === 'win32'
  ? spawnSync('pwsh', ['-NoProfile', '-Command', command], spawnOptions)
  : spawnSync(process.execPath, [process.env.PLUGIN_ROOT + '/hooks/herald.mjs'], spawnOptions)
if (result.status !== 0) throw new Error(result.stderr || String(result.error))
process.stdout.write('{}')
