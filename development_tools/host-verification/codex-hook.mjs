import { appendFileSync, readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
const chunks = []
for await (const chunk of process.stdin) chunks.push(chunk)
const input = Buffer.concat(chunks).toString('utf8')
const event = JSON.parse(input)
appendFileSync(process.env.HERALD_HOST_PROOF, JSON.stringify({ at: Date.now(), ...event }) + '\n')
const config = JSON.parse(readFileSync(process.env.PLUGIN_ROOT + '/hooks/hooks.json', 'utf8'))
const command = config.hooks[event.hook_event_name][0].hooks[0].commandWindows
const result = process.platform === 'win32'
  ? spawnSync('pwsh', ['-NoProfile', '-Command', command], { input, encoding: 'utf8', timeout: 15000, windowsHide: true })
  : spawnSync(process.execPath, [process.env.PLUGIN_ROOT + '/hooks/herald.mjs'], { input, encoding: 'utf8', timeout: 15000, windowsHide: true })
if (result.status !== 0) throw new Error(result.stderr || String(result.error))
process.stdout.write('{}')
