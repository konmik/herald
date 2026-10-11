import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { test } from 'node:test'
import { APP, BUNDLE_ID, HELPER, HELPER_PATH, LEGACY_SETTINGS, backupName, bundleOf, infoPlist, isAnnouncer, macArch, ownedExecutable, parseArguments } from '../deploy-macos.mjs'

test('Herald.app opens Settings in the Dock and carries the announcer as a background helper', () => {
  assert.deepEqual([APP.name, APP.identifier], ['Herald', BUNDLE_ID])
  assert.deepEqual([HELPER.name, HELPER.identifier], ['Herald Announcer', `${BUNDLE_ID}.announcer`])
  assert.equal(HELPER_PATH, 'Contents/Helpers/Herald Announcer.app')
  assert.deepEqual([LEGACY_SETTINGS.name, LEGACY_SETTINGS.identifier], ['Herald Settings', `${BUNDLE_ID}.settings`])
  const main = infoPlist({ ...APP, version: '1.2.3' })
  assert.match(main, /<key>CFBundleExecutable<\/key>\n {2}<string>Herald<\/string>/)
  assert.doesNotMatch(main, /LSUIElement/)
  const helper = infoPlist({ ...HELPER, version: '1.2.3' })
  assert.match(helper, /<key>CFBundleExecutable<\/key>\n {2}<string>Herald Announcer<\/string>/)
  assert.match(helper, /<key>LSUIElement<\/key>\n {2}<true\/>/)
  assert.match(helper, /NSAppleEventsUsageDescription/)
  assert.match(infoPlist({ name: 'A & <B>', identifier: 'x', version: '1' }), /A &amp; &lt;B&gt;/)
})

test('property lists are valid', { skip: process.platform !== 'darwin' }, async t => {
  const directory = await mkdtemp(join(tmpdir(), 'herald-plist-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  for (const app of [APP, HELPER]) {
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
  assert.ok(ownedExecutable(resolve('/Applications/Herald.app/Contents/Helpers/Herald Announcer.app/Contents/MacOS/Herald Announcer'), [announcer]))
  assert.ok(!ownedExecutable(resolve('/Applications/Herald.app.old/Contents/MacOS/Herald'), [announcer]))
  assert.equal(bundleOf('/Applications/Herald.app/Contents/Helpers/Herald Announcer.app/Contents/MacOS/Herald Announcer'), '/Applications/Herald.app/Contents/Helpers/Herald Announcer.app')
  assert.equal(bundleOf('/x/bin/herald-darwin-x64'), undefined)
  // The same path is the announcer in the old Herald.app and Settings in the new one; the bundle's LSUIElement decides.
  const main = '/Applications/Herald.app/Contents/MacOS/Herald'
  assert.ok(isAnnouncer({ executable: main, args: main, background: true }))
  assert.ok(!isAnnouncer({ executable: main, args: main, background: false }))
  const helper = '/Applications/Herald.app/Contents/Helpers/Herald Announcer.app/Contents/MacOS/Herald Announcer'
  assert.ok(isAnnouncer({ executable: helper, args: helper, background: true }))
  assert.ok(isAnnouncer({ executable: '/x/bin/herald-darwin-x64', args: '/x/bin/herald-darwin-x64 --assets /x' }))
  assert.ok(!isAnnouncer({ executable: '/x/bin/herald-darwin-x64', args: '/x/bin/herald-darwin-x64 --settings' }))
  assert.ok(!isAnnouncer({ executable: '/x/bin/herald-darwin-x64', args: '/x/bin/herald-darwin-x64 --bridge --assets /x' }))
  const legacy = '/Applications/Herald Settings.app/Contents/MacOS/Herald Settings'
  assert.ok(!isAnnouncer({ executable: legacy, args: legacy, background: false }))
})
