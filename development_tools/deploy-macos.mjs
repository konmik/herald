// Build a release Herald payload, install Herald.app and Herald Settings.app, register the Claude, Codex and OpenCode plugins and restart the announcer.
import { spawnSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { copyFile, cp, lstat, mkdir, readdir, readFile, realpath, rename, rm, stat, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

export const BUNDLE_ID = 'io.github.konmik.herald'
export const PLUGIN_ID = 'herald@herald-local'
export const CODEX_PLUGIN_FILES = ['.codex-plugin/plugin.json', '.agents/plugins/marketplace.json', 'hooks/hooks.json', 'hooks/herald.mjs']
// Herald.app runs the announcer outside the Dock; Herald Settings.app is the Settings entry, like the Linux menu item.
export const APPS = [
  { name: 'Herald', identifier: BUNDLE_ID, background: true, payload: true },
  { name: 'Herald Settings', identifier: `${BUNDLE_ID}.settings`, background: false, payload: false },
]

const root = fileURLToPath(new URL('..', import.meta.url))

function escapeXml(value) {
  return String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;')
}

export function infoPlist({ name, identifier, version, background }) {
  const entries = [
    ['CFBundleDevelopmentRegion', 'en'],
    ['CFBundleDisplayName', name],
    ['CFBundleExecutable', name],
    ['CFBundleIconFile', 'herald'],
    ['CFBundleIdentifier', identifier],
    ['CFBundleInfoDictionaryVersion', '6.0'],
    ['CFBundleName', name],
    ['CFBundlePackageType', 'APPL'],
    ['CFBundleShortVersionString', version],
    ['CFBundleVersion', version],
    ['LSApplicationCategoryType', 'public.app-category.utilities'],
    ['NSHighResolutionCapable', true],
  ]
  // The announcer stays out of the Dock and asks System Events for meeting windows before speaking.
  if (background) entries.push(['LSUIElement', true], ['NSAppleEventsUsageDescription', 'Herald checks for open meeting windows so it stays quiet during calls.'])
  const body = entries.map(([key, value]) => `  <key>${escapeXml(key)}</key>\n  ${value === true ? '<true/>' : `<string>${escapeXml(value)}</string>`}`).join('\n')
  return `<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0">\n<dict>\n${body}\n</dict>\n</plist>\n`
}

export function macArch(arch = process.arch) {
  if (arch === 'arm64') return 'arm64'
  if (arch === 'x64') return 'x64'
  throw new Error(`Unsupported macOS architecture: ${arch}`)
}

export function parseArguments(argv) {
  const options = { applications: '/Applications', checks: true, hosts: true, start: true, reloadOpenCode: false, dryRun: false }
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index]
    const value = () => { if (index + 1 >= argv.length) throw new Error(`Missing value for ${argument}`); return argv[++index] }
    if (argument === '--applications') options.applications = resolve(value())
    else if (argument === '--claude-config') options.claudeConfig = resolve(value())
    else if (argument === '--opencode-config') options.openCodeConfig = resolve(value())
    else if (argument === '--codex-home') options.codexHome = resolve(value())
    else if (argument === '--skip-checks') options.checks = false
    else if (argument === '--skip-host-registration') options.hosts = false
    else if (argument === '--no-start') options.start = false
    else if (argument === '--reload-opencode') options.reloadOpenCode = true
    else if (argument === '--dry-run') options.dryRun = true
    else throw new Error(`Unknown option: ${argument}`)
  }
  return options
}

/** True when an executable lies inside one of the given runtime directories. */
export function ownedExecutable(executable, directories) {
  return directories.some(directory => executable.startsWith(resolve(directory) + sep))
}

/** Announcers are stopped before replacement; Settings windows and short bridge calls are left alone. */
export function isAnnouncer({ executable, args }) {
  return !/(^|\s)--(settings|bridge)(\s|$)/.test(args) && !executable.endsWith('/MacOS/Herald Settings')
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed with exit code ${result.status}`)
  return result
}

function output(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} ${args.join(' ')} failed: ${result.stderr}`)
  return result.stdout
}

function hasCommand(name) {
  return spawnSync('/bin/sh', ['-c', 'command -v "$1"', 'sh', name], { encoding: 'utf8' }).status === 0
}

async function assertNoLinks(path) {
  let current = resolve(path)
  while (current !== dirname(current)) {
    const info = await lstat(current).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
    // macOS ships /tmp, /var and /etc as system links; refuse links anywhere else.
    if (info?.isSymbolicLink() && !['/tmp', '/var', '/etc'].includes(current)) throw new Error(`Links are not allowed: ${current}`)
    current = dirname(current)
  }
}

async function files(directory, base = directory) {
  const result = []
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isSymbolicLink()) throw new Error(`Links are not allowed: ${path}`)
    if (entry.isDirectory()) result.push(...await files(path, base))
    else result.push(relative(base, path).split(sep).join('/'))
  }
  return result.sort()
}

async function sha256(path) {
  return createHash('sha256').update(await readFile(path)).digest('hex')
}

async function copyLicenses(metafile, destination) {
  const metadata = JSON.parse(await readFile(metafile, 'utf8'))
  const seen = new Set()
  for (const input of Object.keys(metadata.inputs)) {
    const match = input.replaceAll('\\', '/').match(/^(.*(?:^|\/)node_modules\/(?:@[^/]+\/)?[^/]+)(?:\/|$)/)
    if (!match) continue
    const directory = resolve(root, match[1])
    if (seen.has(directory)) continue
    seen.add(directory)
    const pkg = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'))
    let licenses = (await readdir(directory, { withFileTypes: true })).filter(entry => entry.isFile() && /^(LICENSE|LICENCE|COPYING|NOTICE)(\b|[-._])/i.test(entry.name)).map(entry => join(directory, entry.name))
    if (!licenses.length && pkg.name.startsWith('@opencode/')) {
      const pinned = join(root, 'development_tools/licenses', `opencode-${pkg.version}-LICENSE`)
      if (existsSync(pinned)) licenses = [pinned]
    }
    if (!licenses.length) throw new Error(`Bundled package has no license file: ${pkg.name}@${pkg.version}`)
    const target = join(destination, `${pkg.name.replace('/', '-')}-${pkg.version}`)
    await mkdir(target, { recursive: true })
    for (const license of licenses) await copyFile(license, join(target, license.split(sep).at(-1)))
  }
}

/** Stage the same runtime payload as the Windows bundle, with a darwin binary. */
async function buildPayload(stage, binary, arch) {
  const payload = join(stage, 'herald')
  for (const name of ['.claude-plugin/plugin.json', '.claude-plugin/marketplace.json', 'hooks/hooks.json', 'hooks/register.ts']) {
    await mkdir(dirname(join(payload, 'claude-plugin', name)), { recursive: true })
    await copyFile(join(root, 'claude-plugin', name), join(payload, 'claude-plugin', name))
  }
  for (const name of CODEX_PLUGIN_FILES) {
    await mkdir(dirname(join(payload, 'codex-plugin', name)), { recursive: true })
    await copyFile(join(root, 'codex-plugin', name), join(payload, 'codex-plugin', name))
  }
  const runtime = join(payload, 'native-announcer')
  await mkdir(join(runtime, 'bin'), { recursive: true })
  await copyFile(binary, join(runtime, 'bin', `herald-darwin-${arch}`))
  await mkdir(join(runtime, 'resources'), { recursive: true })
  await copyFile(join(root, 'native-announcer/resources/characters.json'), join(runtime, 'resources/characters.json'))
  await cp(join(root, 'native-announcer/resources/videos'), join(runtime, 'resources/videos'), { recursive: true })
  const pkg = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'))
  const exports = Object.fromEntries(Object.entries(pkg.exports).map(([key, value]) => [key, value.replace(/\.ts$/, '.js')]))
  await writeFile(join(payload, 'package.json'), JSON.stringify({ name: pkg.name, version: pkg.version, type: 'module', private: true, exports }, null, 2) + '\n')
  const metafile = join(stage, 'modules.json')
  const licenses = join(payload, 'licenses/javascript')
  for (const entry of ['index', 'tui']) {
    run('bun', ['build', join(root, `opencode-plugin/${entry}.ts`), '--target', 'bun', '--format', 'esm', '--minify', '--outdir', join(payload, 'opencode-plugin'), `--metafile=${metafile}`])
    await copyLicenses(metafile, licenses)
  }
  await writeFile(join(payload, 'index.ts'), `export { default } from "${exports['.']}"\n`)
  await writeFile(join(payload, 'tui.ts'), `export { default } from "${exports['./tui']}"\n`)
  await writeFile(join(payload, 'claude-plugin/.claude-plugin/packaged.json'), '{"schemaVersion":1}\n')
  run('bun', ['build', join(root, 'development_tools/bundle/register-opencode.mjs'), '--target', 'node', '--format', 'esm', '--minify', `--outfile=${join(payload, 'register-opencode.mjs')}`, `--metafile=${metafile}`])
  await copyLicenses(metafile, licenses)
  await rm(metafile, { force: true })
  return { payload, version: pkg.version }
}

/** Sign the plugin runtimes, which the app bundle later seals as resources, then copy the Claude and Codex runtimes and write the manifest. */
async function sealPayload(payload, arch, identifier) {
  output('codesign', ['--force', '--sign', '-', '--identifier', `${identifier}.runtime`, join(payload, 'native-announcer/bin', `herald-darwin-${arch}`)])
  for (const host of ['claude-plugin', 'codex-plugin']) await cp(join(payload, 'native-announcer'), join(payload, host, 'native-announcer'), { recursive: true })
  const pkg = JSON.parse(await readFile(join(payload, 'package.json'), 'utf8'))
  const listed = []
  for (const path of await files(payload)) {
    const absolute = join(payload, path)
    listed.push({ path, sha256: await sha256(absolute), size: (await stat(absolute)).size })
  }
  await writeFile(join(payload, 'bundle-manifest.json'), JSON.stringify({ schemaVersion: 1, name: 'herald', version: pkg.version, platform: 'darwin', arch, files: listed }, null, 2) + '\n')
}

export async function verifyPayload(payload, arch) {
  const manifest = JSON.parse(await readFile(join(payload, 'bundle-manifest.json'), 'utf8'))
  if (manifest.schemaVersion !== 1 || manifest.name !== 'herald' || manifest.platform !== 'darwin' || manifest.arch !== arch) throw new Error('Unsupported bundle manifest')
  const actual = (await files(payload)).filter(path => path !== 'bundle-manifest.json')
  const paths = manifest.files.map(file => file.path)
  if (JSON.stringify(actual) !== JSON.stringify([...paths].sort())) throw new Error('Bundle contains unlisted or missing files')
  for (const file of manifest.files) {
    if (file.path.split('/').some(part => !part || part === '.' || part === '..')) throw new Error(`Unsafe payload path: ${file.path}`)
    if (/\/resources\/tts\//.test(file.path)) throw new Error('Offline voice must be installed from Settings, not included in the bundle')
    if (await sha256(join(payload, file.path)) !== file.sha256) throw new Error(`Payload verification failed: ${file.path}`)
  }
  for (const required of ['register-opencode.mjs', 'package.json', 'index.ts', 'tui.ts', 'opencode-plugin/index.js', 'opencode-plugin/tui.js', 'claude-plugin/.claude-plugin/plugin.json', 'claude-plugin/.claude-plugin/marketplace.json', 'claude-plugin/.claude-plugin/packaged.json', 'claude-plugin/hooks/hooks.json', 'claude-plugin/hooks/register.ts', ...CODEX_PLUGIN_FILES.map(name => `codex-plugin/${name}`), `native-announcer/bin/herald-darwin-${arch}`, `claude-plugin/native-announcer/bin/herald-darwin-${arch}`, `codex-plugin/native-announcer/bin/herald-darwin-${arch}`]) {
    if (!paths.includes(required)) throw new Error(`Required runtime file missing: ${required}`)
  }
  if (paths.includes('claude-plugin/.claude-plugin/development.json')) throw new Error('Development fallback markers cannot be installed')
  for (const prefix of ['native-announcer', 'claude-plugin/native-announcer', 'codex-plugin/native-announcer']) {
    const characters = JSON.parse(await readFile(join(payload, prefix, 'resources/characters.json'), 'utf8'))
    if (!Object.keys(characters).length) throw new Error('Character library is empty')
    for (const character of Object.values(characters)) {
      if (!paths.includes(`${prefix}/resources/${character.animationPath}`)) throw new Error(`Character animation is missing: ${character.animationPath}`)
    }
  }
  return manifest
}

async function makeIcon(stage) {
  const iconset = join(stage, 'herald.iconset')
  await mkdir(iconset, { recursive: true })
  const source = join(root, 'native-announcer/resources/portraits/flamboyant-herald.png')
  for (const size of [16, 32, 128, 256, 512]) {
    for (const scale of [1, 2]) {
      const name = scale === 1 ? `icon_${size}x${size}.png` : `icon_${size}x${size}@2x.png`
      output('sips', ['-z', String(size * scale), String(size * scale), '-s', 'format', 'png', source, '--out', join(iconset, name)])
    }
  }
  const icon = join(stage, 'herald.icns')
  output('iconutil', ['-c', 'icns', iconset, '-o', icon])
  await rm(iconset, { recursive: true })
  return icon
}

async function makeApplication(stage, app, { binary, icon, payload, version }) {
  const bundle = join(stage, `${app.name}.app`)
  const contents = join(bundle, 'Contents')
  await mkdir(join(contents, 'MacOS'), { recursive: true })
  await mkdir(join(contents, 'Resources'), { recursive: true })
  await writeFile(join(contents, 'Info.plist'), infoPlist({ ...app, version }))
  await writeFile(join(contents, 'PkgInfo'), 'APPL????')
  await copyFile(binary, join(contents, 'MacOS', app.name))
  // Contents/Resources is the default asset folder of a bundled executable.
  await copyFile(join(root, 'native-announcer/resources/characters.json'), join(contents, 'Resources/characters.json'))
  await cp(join(root, 'native-announcer/resources/videos'), join(contents, 'Resources/videos'), { recursive: true })
  await copyFile(icon, join(contents, 'Resources/herald.icns'))
  if (app.payload) await cp(payload, join(contents, 'Resources/herald'), { recursive: true })
  output('plutil', ['-lint', join(contents, 'Info.plist')])
  output('codesign', ['--force', '--deep', '--sign', '-', bundle])
  output('codesign', ['--verify', '--deep', '--strict', bundle])
  return bundle
}

function bundleIdentifier(bundle) {
  const result = spawnSync('plutil', ['-extract', 'CFBundleIdentifier', 'raw', join(bundle, 'Contents/Info.plist')], { encoding: 'utf8' })
  return result.status === 0 ? result.stdout.trim() : undefined
}

async function installApplication(source, applications, app) {
  const destination = join(applications, `${app.name}.app`)
  await assertNoLinks(destination)
  if (existsSync(destination) && bundleIdentifier(destination) !== app.identifier) throw new Error(`Refusing to replace an application Herald did not install: ${destination}`)
  const incoming = join(applications, `.herald-stage-${randomUUID()}.app`)
  const backup = join(applications, `.herald-backup-${randomUUID()}.app`)
  run('ditto', [source, incoming])
  try {
    if (existsSync(destination)) await rename(destination, backup)
    try { await rename(incoming, destination) } catch (error) { if (existsSync(backup)) await rename(backup, destination); throw error }
  } finally {
    await rm(incoming, { recursive: true, force: true })
  }
  await rm(backup, { recursive: true, force: true })
  output('codesign', ['--verify', '--deep', '--strict', destination])
  // Register the new bundle so Spotlight, Launchpad and `open -a` find it immediately.
  spawnSync('/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister', ['-f', destination], { stdio: 'ignore' })
  return destination
}

function heraldProcesses() {
  const result = []
  for (const line of output('ps', ['-axww', '-o', 'pid=,args=']).split('\n')) {
    const match = line.match(/^\s*(\d+)\s+(.*)$/)
    if (!match || !/herald/i.test(match[2]) || Number(match[1]) === process.pid) continue
    const pid = Number(match[1])
    const open = spawnSync('lsof', ['-a', '-p', String(pid), '-d', 'txt', '-Fn'], { encoding: 'utf8' }).stdout ?? ''
    const executable = open.split('\n').find(entry => entry.startsWith('n'))?.slice(1)
    if (executable) result.push({ pid, executable, args: match[2] })
  }
  return result
}

async function stopOwnedAnnouncers(directories) {
  const stopped = []
  for (const candidate of heraldProcesses()) {
    if (!ownedExecutable(candidate.executable, directories) || !isAnnouncer(candidate)) continue
    process.kill(candidate.pid, 'SIGTERM')
    for (let attempt = 0; ; attempt++) {
      try { process.kill(candidate.pid, 0) } catch { break }
      if (attempt === 100) throw new Error(`Announcer process ${candidate.pid} did not stop`)
      await new Promise(done => setTimeout(done, 100))
    }
    stopped.push(candidate)
  }
  return stopped
}

async function readJson(path) {
  try { return JSON.parse(await readFile(path, 'utf8')) } catch (error) { if (error.code === 'ENOENT') return undefined; throw error }
}

async function claudeState(config) {
  const marketplaces = await readJson(join(config, 'plugins/known_marketplaces.json'))
  const installed = await readJson(join(config, 'plugins/installed_plugins.json'))
  const source = marketplaces?.['herald-local']?.source?.path
  const installations = (installed?.plugins?.[PLUGIN_ID] ?? []).filter(entry => entry.scope === 'user')
  for (const installation of installations) {
    if (!resolve(installation.installPath).startsWith(resolve(config, 'plugins/cache') + sep)) throw new Error(`Refusing to replace a plugin outside the Claude cache: ${installation.installPath}`)
    if ((await readJson(join(installation.installPath, '.claude-plugin/plugin.json')))?.name !== 'herald') throw new Error(`Cannot confirm plugin ownership: ${installation.installPath}`)
  }
  return { source, installations }
}

/** A visible backup file name such as `claude__settings.json` for `~/.claude/settings.json`. */
export function backupName(path, home = homedir()) {
  return relative(home, path).split(sep).map(part => part.replace(/^\.+/, '')).join('__')
}

async function snapshot(paths, backupDirectory) {
  const snapshots = []
  await mkdir(backupDirectory, { recursive: true })
  for (const path of paths) {
    await assertNoLinks(path)
    const bytes = await readFile(path).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
    snapshots.push({ path, bytes })
    if (bytes) await writeFile(join(backupDirectory, backupName(path)), bytes)
  }
  await writeFile(join(backupDirectory, 'snapshot.json'), JSON.stringify(snapshots.map(({ path, bytes }) => ({ path, existed: Boolean(bytes) })), null, 2) + '\n')
  return snapshots
}

async function restore(snapshots) {
  for (const { path, bytes } of snapshots) {
    if (bytes) await writeFile(path, bytes)
    else await rm(path, { force: true })
  }
}

async function registerClaude(payload, config, explicit) {
  const source = join(payload, 'claude-plugin')
  // Only pass a profile the user chose: with CLAUDE_CONFIG_DIR set, Claude also moves its .claude.json into that folder.
  const env = explicit ? { ...process.env, CLAUDE_CONFIG_DIR: config } : process.env
  const before = await claudeState(config)
  const changes = []
  if (before.source !== source) { run('claude', ['plugin', 'marketplace', 'add', source, '--scope', 'user'], { env }); changes.push(`added marketplace herald-local -> ${source}`) }
  if (!before.installations.length) { run('claude', ['plugin', 'install', PLUGIN_ID, '--scope', 'user'], { env }); changes.push(`installed ${PLUGIN_ID} (user scope)`) }
  const { installations } = await claudeState(config)
  if (!installations.length) throw new Error('Claude did not report a user installation')
  for (const installation of installations) {
    // Replace the cache copy with the installed runtime, as the Windows installer does.
    const destination = resolve(installation.installPath)
    const stage = join(dirname(destination), `.herald-stage-${randomUUID()}`)
    const backup = join(dirname(destination), `.herald-backup-${randomUUID()}`)
    try {
      for (const name of ['.claude-plugin', 'hooks', 'native-announcer']) await cp(join(source, name), join(stage, name), { recursive: true })
      await rename(destination, backup)
      try { await rename(stage, destination) } catch (error) { await rename(backup, destination); throw error }
      await rm(backup, { recursive: true, force: true })
    } finally { await rm(stage, { recursive: true, force: true }) }
    changes.push(`refreshed Claude plugin cache ${destination}`)
  }
  const plugins = JSON.parse(output('claude', ['plugin', 'list', '--json'], { env }))
  if (!plugins.some(plugin => plugin.id === PLUGIN_ID && plugin.scope === 'user' && plugin.enabled)) { run('claude', ['plugin', 'enable', PLUGIN_ID, '--scope', 'user'], { env }); changes.push(`enabled ${PLUGIN_ID}`) }
  return { changes, installations: installations.map(entry => entry.installPath) }
}

function codexState(env) {
  const plugins = JSON.parse(output('codex', ['plugin', 'list', '--available', '--json'], { env }))
  const entry = [...plugins.installed, ...plugins.available].find(plugin => plugin.pluginId === PLUGIN_ID)
  return { source: entry?.marketplaceSource?.source, installed: Boolean(entry?.installed), enabled: Boolean(entry?.enabled) }
}

async function registerCodex(payload, codexHome, explicit) {
  const source = join(payload, 'codex-plugin')
  const env = explicit ? { ...process.env, CODEX_HOME: codexHome } : process.env
  const before = codexState(env)
  const changes = []
  if (before.source && await realpath(before.source).catch(() => before.source) !== await realpath(source)) {
    if ((await readJson(join(before.source, '.codex-plugin/plugin.json')))?.name !== 'herald') throw new Error(`Cannot confirm ownership of the herald-local Codex marketplace at ${before.source}`)
    run('codex', ['plugin', 'marketplace', 'remove', 'herald-local'], { env })
    changes.push(`removed marketplace herald-local -> ${before.source}`)
  }
  // Both commands edit config.toml in place and are idempotent; `plugin add` also refreshes the cached copy.
  run('codex', ['plugin', 'marketplace', 'add', source], { env })
  run('codex', ['plugin', 'add', PLUGIN_ID], { env })
  changes.push(`installed ${PLUGIN_ID} from ${source}`)
  const after = codexState(env)
  if (!after.installed || !after.enabled) throw new Error('Codex did not report an enabled herald plugin')
  return { changes, hooks: 'Codex skips new or changed plugin hooks until they are trusted: open /hooks in Codex once and trust the Herald hooks.' }
}

async function registerOpenCode(payload, configDirectory, reload) {
  const existing = ['opencode.json', 'opencode.jsonc'].map(name => join(configDirectory, name)).filter(path => existsSync(path))
  const configs = existing.length ? existing : [join(configDirectory, 'opencode.jsonc')]
  await mkdir(configDirectory, { recursive: true })
  const documents = []
  for (const config of configs) documents.push({ config, text: await readFile(config, 'utf8').catch(() => '{}\n') })
  const rendered = JSON.parse(output('node', [join(payload, 'register-opencode.mjs'), '--render'], { input: JSON.stringify({ installed: payload, documents }) }))
  const changes = []
  for (const document of rendered) {
    if (document.result === documents.find(entry => entry.config === document.config).text) continue
    await writeFile(document.config, document.result)
    changes.push(`${document.config} plugins -> ${payload}`)
  }
  if (reload) run('opencode', ['api', 'post', '/api/location/reload'])
  return changes
}

async function main() {
  if (process.platform !== 'darwin') throw new Error('deploy-macos.mjs installs macOS application bundles; use deploy:plugins on Windows.')
  const options = parseArguments(process.argv.slice(2))
  const arch = macArch()
  const claudeConfig = options.claudeConfig ?? process.env.CLAUDE_CONFIG_DIR ?? join(homedir(), '.claude')
  const openCodeConfig = options.openCodeConfig ?? join(process.env.XDG_CONFIG_HOME ?? join(homedir(), '.config'), 'opencode')
  const codexHome = options.codexHome ?? process.env.CODEX_HOME ?? join(homedir(), '.codex')
  const claude = options.hosts && hasCommand('claude')
  const codex = options.hosts && hasCommand('codex')
  const openCode = options.hosts && hasCommand('opencode')
  if (options.dryRun) {
    console.log(JSON.stringify({ applications: APPS.map(app => join(options.applications, `${app.name}.app`)), payload: join(options.applications, 'Herald.app/Contents/Resources/herald'), claudeConfig: claude ? claudeConfig : null, codexHome: codex ? codexHome : null, openCodeConfig: openCode ? openCodeConfig : null, checks: options.checks, start: options.start }, null, 2))
    return
  }
  if (options.checks) {
    run('pnpm', ['run', 'check'], { cwd: root })
    if (hasCommand('claude')) run('claude', ['plugin', 'validate', join(root, 'claude-plugin')], { cwd: root })
  }
  run('cargo', ['build', '--release', '--locked', '-j', '6', '--manifest-path', join(root, 'native-announcer/Cargo.toml')], { cwd: root })
  const binary = join(process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'native-announcer/target'), 'release/herald')
  const stage = join(root, 'temp/bundles', `.herald-stage-macos-${randomUUID()}`)
  await mkdir(stage, { recursive: true })
  const summary = { applications: [], payload: undefined, stopped: [], claude: 'skipped', codex: 'skipped', openCode: 'skipped', backups: null }
  try {
    const { payload, version } = await buildPayload(stage, binary, arch)
    await sealPayload(payload, arch, BUNDLE_ID)
    await verifyPayload(payload, arch)
    const icon = await makeIcon(stage)
    const bundles = []
    for (const app of APPS) bundles.push([app, await makeApplication(join(stage, 'apps'), app, { binary, icon, payload, version })])
    const installedPayload = join(options.applications, 'Herald.app/Contents/Resources/herald')
    const previous = claude ? await claudeState(claudeConfig) : { installations: [] }
    const owned = [join(root, 'native-announcer'), join(root, 'claude-plugin'), ...APPS.map(app => join(options.applications, `${app.name}.app`)), ...(previous.source ? [previous.source, dirname(previous.source)] : []), ...previous.installations.map(entry => entry.installPath), join(codexHome, 'plugins/cache/herald-local')]
    summary.stopped = await stopOwnedAnnouncers(owned)
    for (const [app, bundle] of bundles) summary.applications.push(await installApplication(bundle, options.applications, app))
    await verifyPayload(installedPayload, arch)
    summary.payload = installedPayload
    if (options.hosts) {
      const backupDirectory = join(root, 'temp/deploy-backups', `macos-${new Date().toISOString().replaceAll(':', '-')}`)
      const watched = [...['settings.json', 'plugins/known_marketplaces.json', 'plugins/installed_plugins.json'].map(name => join(claudeConfig, name)), ...(codex ? [join(codexHome, 'config.toml')] : []), ...(openCode ? ['opencode.json', 'opencode.jsonc'].map(name => join(openCodeConfig, name)) : [])]
      const snapshots = await snapshot(watched, backupDirectory)
      summary.backups = backupDirectory
      try {
        summary.claude = claude ? await registerClaude(installedPayload, claudeConfig, Boolean(options.claudeConfig || process.env.CLAUDE_CONFIG_DIR)) : 'skipped: claude is not on PATH'
        summary.codex = codex ? await registerCodex(installedPayload, codexHome, Boolean(options.codexHome || process.env.CODEX_HOME)) : 'skipped: codex is not on PATH'
        summary.openCode = openCode ? await registerOpenCode(installedPayload, openCodeConfig, options.reloadOpenCode) : 'skipped: opencode is not on PATH'
      } catch (error) {
        await restore(snapshots)
        throw error
      }
    }
    if (options.start) {
      run('open', ['-g', join(options.applications, 'Herald.app')])
      summary.started = join(options.applications, 'Herald.app')
    }
  } finally {
    await rm(stage, { recursive: true, force: true })
  }
  console.log(JSON.stringify(summary, null, 2))
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message ?? error); process.exit(1) })
}
