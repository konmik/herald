import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { APP, BUNDLE_ID, CODEX_PLUGIN_FILES, HELPER, HELPER_PATH, LEGACY_SETTINGS, backupName, bundleOf, copyPiExtension, infoPlist, isAnnouncer, macArch, ownedExecutable, parseArguments, verifyPayload } from '../deploy-macos.mjs'

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
  assert.equal(parseArguments(['--pi-agent-dir', '/tmp/pi']).piAgentDirectory, resolve('/tmp/pi'))
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

async function payloadFixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'herald-macos-payload-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const contents = new Map([
    ...['register-opencode.mjs', 'register-pi.mjs', 'pi-plugin/index.ts', 'pi-plugin/bridge.mjs', 'pi-plugin/herald.mjs', 'pi-plugin/fork.mjs', 'index.ts', 'tui.ts', 'opencode-plugin/index.js', 'opencode-plugin/tui.js', 'claude-plugin/.claude-plugin/plugin.json', 'claude-plugin/.claude-plugin/marketplace.json', 'claude-plugin/.claude-plugin/packaged.json', 'claude-plugin/hooks/hooks.json', 'claude-plugin/hooks/register.ts', ...CODEX_PLUGIN_FILES.map(name => `codex-plugin/${name}`)].map(path => [path, 'fixture']),
    ['package.json', JSON.stringify({ pi: { extensions: ['./pi-plugin/index.ts'] } })],
  ])
  for (const prefix of ['native-announcer', 'claude-plugin/native-announcer', 'codex-plugin/native-announcer']) {
    contents.set(`${prefix}/bin/herald-darwin-arm64`, 'binary')
    contents.set(`${prefix}/resources/characters.json`, '{"fixture":{"animationPath":"videos/fixture.mp4"}}')
    contents.set(`${prefix}/resources/videos/fixture.mp4`, 'video')
  }
  const manifest = { schemaVersion: 1, name: 'herald', platform: 'darwin', arch: 'arm64', files: [] }
  for (const [path, text] of contents) {
    await mkdir(dirname(join(directory, path)), { recursive: true })
    await writeFile(join(directory, path), text)
    manifest.files.push({ path, sha256: createHash('sha256').update(text).digest('hex'), size: Buffer.byteLength(text) })
  }
  const manifestPath = join(directory, 'bundle-manifest.json')
  await writeFile(manifestPath, JSON.stringify(manifest))
  return { directory, contents, manifest, manifestPath }
}

test('macOS payload requires the Pi extension and its own bridge', async t => {
  const { directory, contents, manifest, manifestPath } = await payloadFixture(t)
  await verifyPayload(directory, 'arm64')
  for (const path of ['pi-plugin/index.ts', 'pi-plugin/bridge.mjs', 'pi-plugin/herald.mjs', 'pi-plugin/fork.mjs', 'register-pi.mjs']) {
    await rm(join(directory, path))
    await writeFile(manifestPath, JSON.stringify({ ...manifest, files: manifest.files.filter(file => file.path !== path) }))
    await assert.rejects(verifyPayload(directory, 'arm64'), /Required runtime file missing/)
    await writeFile(join(directory, path), contents.get(path))
  }
  await writeFile(manifestPath, JSON.stringify(manifest))
  await writeFile(join(directory, 'pi-plugin/index.ts'), 'tampered')
  await assert.rejects(verifyPayload(directory, 'arm64'), /Payload verification failed/)
  await writeFile(join(directory, 'pi-plugin/index.ts'), contents.get('pi-plugin/index.ts'))
  for (const path of ['claude-plugin/scripts/bridge.mjs', 'claude-plugin/scripts/runtime.mjs', 'claude-plugin/scripts/session-title.mjs', 'pi-plugin/tests/fixture.mjs', 'pi-plugin/node_modules/unused-sdk/index.js']) {
    await mkdir(dirname(join(directory, path)), { recursive: true })
    await writeFile(join(directory, path), 'forbidden')
    const file = { path, sha256: createHash('sha256').update('forbidden').digest('hex'), size: 9 }
    await writeFile(manifestPath, JSON.stringify({ ...manifest, files: [...manifest.files, file] }))
    await assert.rejects(verifyPayload(directory, 'arm64'), /Obsolete Node runtime|Unexpected plugin runtime/)
    await rm(join(directory, path))
  }
  await writeFile(manifestPath, JSON.stringify(manifest))
  await verifyPayload(directory, 'arm64')
})

test('macOS staging copies every real Pi helper unchanged without tests or an SDK', async t => {
  const { directory, manifest, manifestPath } = await payloadFixture(t)
  const source = fileURLToPath(new URL('../../pi-plugin/', import.meta.url))
  const names = ['index.ts', ...(await readdir(source, { withFileTypes: true })).filter(entry => entry.isFile() && entry.name.endsWith('.mjs')).map(entry => entry.name)].sort()
  assert.ok(names.includes('bridge.mjs'))
  const destination = join(directory, 'pi-plugin')
  await copyPiExtension(source, destination)
  assert.deepEqual((await readdir(destination)).sort(), names)
  manifest.files = manifest.files.filter(file => !file.path.startsWith('pi-plugin/'))
  for (const name of names) {
    const bytes = await readFile(join(source, name))
    assert.deepEqual(await readFile(join(destination, name)), bytes)
    manifest.files.push({ path: `pi-plugin/${name}`, sha256: createHash('sha256').update(bytes).digest('hex'), size: bytes.length })
  }
  await writeFile(manifestPath, JSON.stringify(manifest))
  await verifyPayload(directory, 'arm64')
  for (const name of names) {
    const path = join(destination, name)
    const bytes = await readFile(path)
    await writeFile(path, 'tampered')
    await assert.rejects(verifyPayload(directory, 'arm64'), /Payload verification failed/)
    await writeFile(path, bytes)
  }
  await verifyPayload(directory, 'arm64')
})
