import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { test } from 'node:test'
import { APPS, BUNDLE_ID, backupName, infoPlist, isAnnouncer, macArch, ownedExecutable, parseArguments } from '../deploy-macos.mjs'

test('application bundles keep the announcer out of the Dock and open Settings by name', () => {
  const [announcer, settings] = APPS
  assert.deepEqual([announcer.name, settings.name], ['Herald', 'Herald Settings'])
  assert.equal(settings.identifier, `${BUNDLE_ID}.settings`)
  const background = infoPlist({ ...announcer, version: '1.2.3' })
  assert.match(background, /<key>CFBundleExecutable<\/key>\n {2}<string>Herald<\/string>/)
  assert.match(background, /<key>LSUIElement<\/key>\n {2}<true\/>/)
  assert.match(background, /NSAppleEventsUsageDescription/)
  const regular = infoPlist({ ...settings, version: '1.2.3' })
  assert.match(regular, /<string>Herald Settings<\/string>/)
  assert.doesNotMatch(regular, /LSUIElement/)
  assert.match(infoPlist({ name: 'A & <B>', identifier: 'x', version: '1' }), /A &amp; &lt;B&gt;/)
})

test('property lists are valid', { skip: process.platform !== 'darwin' }, async t => {
  const directory = await mkdtemp(join(tmpdir(), 'herald-plist-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  for (const app of APPS) {
    const path = join(directory, `${app.name}.plist`)
    await writeFile(path, infoPlist({ ...app, version: '0.3.0' }))
    assert.equal(spawnSync('plutil', ['-lint', path]).status, 0)
  }
})

test('options, architectures and process ownership', () => {
  assert.deepEqual(parseArguments([]), { applications: '/Applications', checks: true, hosts: true, start: true, reloadOpenCode: false, dryRun: false })
  const options = parseArguments(['--applications', '/tmp/apps', '--skip-checks', '--skip-host-registration', '--no-start'])
  assert.equal(options.applications, resolve('/tmp/apps'))
  assert.equal(options.checks || options.hosts || options.start, false)
  assert.throws(() => parseArguments(['--force']), /Unknown option/)
  assert.throws(() => parseArguments(['--applications']), /Missing value/)
  assert.equal(backupName('/Users/a/.claude/plugins/known_marketplaces.json', '/Users/a'), 'claude__plugins__known_marketplaces.json')
  assert.equal(parseArguments(['--codex-home', '/tmp/codex']).codexHome, resolve('/tmp/codex'))
  assert.equal(backupName('/Users/a/.codex/config.toml', '/Users/a'), 'codex__config.toml')
  assert.equal(macArch('x64'), 'x64')
  assert.equal(macArch('arm64'), 'arm64')
  assert.throws(() => macArch('ia32'), /Unsupported/)
  const announcer = resolve('/Applications/Herald.app')
  assert.ok(ownedExecutable(resolve('/Applications/Herald.app/Contents/MacOS/Herald'), [announcer]))
  assert.ok(!ownedExecutable(resolve('/Applications/Herald.app.old/Contents/MacOS/Herald'), [announcer]))
  assert.ok(isAnnouncer({ executable: '/Applications/Herald.app/Contents/MacOS/Herald', args: '/Applications/Herald.app/Contents/MacOS/Herald' }))
  assert.ok(!isAnnouncer({ executable: '/x/bin/herald-darwin-x64', args: '/x/bin/herald-darwin-x64 --settings' }))
  assert.ok(!isAnnouncer({ executable: '/x/bin/herald-darwin-x64', args: '/x/bin/herald-darwin-x64 --bridge --assets /x' }))
  assert.ok(!isAnnouncer({ executable: '/Applications/Herald Settings.app/Contents/MacOS/Herald Settings', args: '/Applications/Herald Settings.app/Contents/MacOS/Herald Settings' }))
})
