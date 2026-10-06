import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { parseJSONC, updateRegistration, updateRegistrations } from '../bundle/register-opencode.mjs'

test('public registration preserves JSONC comments, unrelated plugins and object options while migrating duplicates', () => {
  const root = mkdtempSync(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'registration-'))
  try {
    const old = join(root, 'checkout')
    const installed = join(root, 'installed').replaceAll('\\', '/')
    mkdirSync(join(old, 'opencode-plugin'), { recursive: true })
    writeFileSync(join(old, 'package.json'), JSON.stringify({ name: 'civilized-agent', exports: { '.': './opencode-plugin/index.ts', './tui': './opencode-plugin/tui.ts' } }))
    writeFileSync(join(old, 'opencode-plugin/index.ts'), 'id: "civilized-agent"')
    const config = join(root, 'opencode.jsonc')
    const text = `{
  // keep this comment
  "model": "unrelated",
  "plugins": [
    "./civilized-agent-unrelated",
    { "package": ${JSON.stringify(old)}, "options": { "minimumSeconds": 90, }, },
    ${JSON.stringify(old)},
    "another-plugin",
  ],
}`
    writeFileSync(config, text)
    const helper = fileURLToPath(new URL('../bundle/register-opencode.mjs', import.meta.url))
    const result = spawnSync(process.execPath, [helper, '--render'], { encoding: 'utf8', input: JSON.stringify({ installed, documents: [{ config, text }] }) })
    assert.equal(result.status, 0, result.stderr)
    const updated = JSON.parse(result.stdout)[0].result
    assert.equal(readFileSync(config, 'utf8'), text)
    assert.ok(updated.includes('// keep this comment'))
    assert.ok(updated.includes('"options": { "minimumSeconds": 90, }'))
    assert.deepEqual(parseJSONC(updated).value.plugins, ['./civilized-agent-unrelated', Object.assign(Object.create(null), { package: installed, options: Object.assign(Object.create(null), { minimumSeconds: 90 }) }), 'another-plugin'])
    assert.equal(updateRegistration(updated, root, installed), updated)
    assert.equal(parseJSONC(updated).value.model, 'unrelated')
    rmSync(old, { recursive: true })
    assert.equal(updateRegistration(updated, root, installed), updated)
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('registration adds only the plugins field and rejects invalid configurations', () => {
  for (const text of ['{}', '{"model":"keep"}', '{"model":"keep",}', '{"plugins":[]}', '{"plugins":["keep"]}']) {
    const value = parseJSONC(updateRegistration(text, '.', 'C:/installed')).value
    assert.ok(value.plugins.includes('C:/installed'))
    if (text.includes('model')) assert.equal(value.model, 'keep')
  }
  for (const text of ['[]', '{"plugins":{}}', '{"plugins":[],"plugins":[]}', '{bad}']) assert.throws(() => updateRegistration(text, '.', 'C:/installed'))
})

test('upgrading a compiled package replaces its registration and preserves options', () => {
  const root = mkdtempSync(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'registration-upgrade-'))
  try {
    const old = join(root, 'old-version')
    const installed = join(root, 'new-version').replaceAll('\\', '/')
    mkdirSync(join(old, 'opencode-plugin'), { recursive: true })
    writeFileSync(join(old, 'package.json'), JSON.stringify({ name: 'civilized-agent', exports: { '.': './opencode-plugin/index.js', './tui': './opencode-plugin/tui.js' } }))
    writeFileSync(join(old, 'opencode-plugin/index.js'), 'export default {id:"civilized-agent",setup(){}}')
    const text = JSON.stringify({ plugins: [{ package: old, options: { minimumSeconds: 90 } }, 'keep'] })
    const updated = updateRegistration(text, root, installed)
    assert.deepEqual(JSON.parse(updated).plugins, [{ package: installed, options: { minimumSeconds: 90 } }, 'keep'])
    assert.equal(updateRegistration(updated, root, installed), updated)
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('consecutive duplicate registrations are removed without breaking array commas', () => {
  for (const text of ['{"plugins":["C:/installed","C:/installed","C:/installed"]}', '{"plugins":["C:/installed","C:/installed","C:/installed",]}', '{"plugins":["C:/installed","C:/installed","keep","C:/installed"]}']) {
    const plugins = parseJSONC(updateRegistration(text, '.', 'C:/installed')).value.plugins
    assert.equal(plugins.filter(item => item === 'C:/installed').length, 1)
    if (text.includes('keep')) assert.ok(plugins.includes('keep'))
  }
  assert.deepEqual(parseJSONC(updateRegistration('{"plugins":["C:/installed",{"package":"C:/installed"}]}', '.', 'C:/installed')).value.plugins, ['C:/installed'])
})

test('JSON and JSONC together keep one registration and its options', () => {
  const updates = updateRegistrations([
    { config: 'C:/profile/opencode.json', text: '{"plugins":[{"package":"C:/installed","options":{"minimumSeconds":90}},"keep"]}' },
    { config: 'C:/profile/opencode.jsonc', text: '{/* keep */ "plugins":["C:/installed","C:/installed",]}' },
  ], 'C:/installed')
  assert.deepEqual(parseJSONC(updates[0].result).value.plugins, ['keep'])
  assert.equal(parseJSONC(updates[1].result).value.plugins[0].options.minimumSeconds, 90)
  assert.ok(updates[1].result.includes('/* keep */'))
  assert.deepEqual(parseJSONC(updateRegistration('{"plugins":["C:/installed"]}', '.', 'C:/installed', true)).value.plugins, [])
})

test('later objects retain all options across duplicates and source precedence', () => {
  const updates = updateRegistrations([
    { config: 'C:/profile/opencode.json', text: '{"plugins":["C:/installed",{"package":"C:/installed","options":{"minimumSeconds":90,"nested":{"first":1,"shared":1}},"custom":true}]}' },
    { config: 'C:/profile/opencode.jsonc', text: '\ufeff{/* keep */ "plugins":["C:/installed",{"package":"C:/installed","options":{/* options */ "nested":{"second":2,"shared":3},"voice":"keep"}},"C:/installed"]}' },
  ], 'C:/installed')
  assert.deepEqual(parseJSONC(updates[0].result).value.plugins, [])
  const [plugin] = parseJSONC(updates[1].result).value.plugins
  assert.deepEqual(JSON.parse(JSON.stringify(plugin)), { package: 'C:/installed', options: { nested: { second: 2, shared: 3, first: 1 }, voice: 'keep', minimumSeconds: 90 }, custom: true })
  assert.ok(updates[1].result.startsWith('\ufeff'))
  assert.ok(updates[1].result.includes('/* options */'))
  assert.equal(updateRegistration(updates[1].result, '.', 'C:/installed'), updates[1].result)
})
