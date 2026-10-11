import { readFileSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import { lstat, mkdir, open, readFile, rename, rm, unlink, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { applyEdits, getNodeValue, modify, parseTree } from 'jsonc-parser'

export function piAgentDirectory(directory = process.env.PI_CODING_AGENT_DIR) {
  return resolve(expandHome(directory || join(homedir(), '.pi', 'agent')))
}

function expandHome(path) {
  return path === '~' ? homedir() : /^~[\\/]/.test(path) ? join(homedir(), path.slice(2)) : path
}

function ownedExtension(reference, directory) {
  const path = resolve(directory, expandHome(reference))
  const entry = /[\\/]index\.ts$/.test(path) ? path : join(path, 'index.ts')
  if (dirname(entry).split(/[\\/]/).at(-1) !== 'pi-plugin') return false
  try {
    const pkg = JSON.parse(readFileSync(join(dirname(dirname(entry)), 'package.json'), 'utf8'))
    readFileSync(entry, 'utf8')
    return pkg.name === 'herald'
      && /^\.\/opencode-plugin\/index\.(?:ts|js)$/.test(pkg.exports?.['.'] ?? '')
      && Array.isArray(pkg.pi?.extensions)
      && pkg.pi.extensions.includes('./pi-plugin/index.ts')
  } catch { return false }
}

export function updatePiRegistration(text, directory, installed) {
  const errors = []
  const tree = parseTree(text.replace(/^\ufeff/, ' '), errors, { disallowComments: true })
  if (!tree || errors.length || tree.type !== 'object') throw new Error('Expected valid Pi settings JSON object')
  function check(node) {
    if (node.type === 'object') {
      const keys = node.children.map(property => property.children[0].value)
      if (new Set(keys).size !== keys.length) throw new Error('Duplicate Pi settings key')
    }
    for (const child of node.children ?? []) check(child)
  }
  check(tree)
  const settings = getNodeValue(tree)
  const previous = settings.extensions === undefined ? [] : settings.extensions
  if (!Array.isArray(previous) || previous.some(entry => typeof entry !== 'string')) throw new Error('Expected Pi extensions to be a string array')
  const entry = join(resolve(installed), 'pi-plugin', 'index.ts').replaceAll('\\', '/')
  if (!ownedExtension(entry, directory)) throw new Error('Cannot confirm installed Herald Pi extension ownership')
  const extensions = []
  let added = false
  for (const reference of previous) {
    if (!ownedExtension(reference, directory)) extensions.push(reference)
    else if (!added) { extensions.push(entry); added = true }
  }
  if (!added) extensions.push(entry)
  if (JSON.stringify(previous) === JSON.stringify(extensions)) return text
  const bom = text.startsWith('\ufeff') ? '\ufeff' : ''
  const plain = text.slice(bom.length)
  return bom + applyEdits(plain, modify(plain, ['extensions'], extensions, { formattingOptions: { insertSpaces: true, tabSize: 2, eol: text.includes('\r\n') ? '\r\n' : '\n' } }))
}

async function assertNoLinks(path) {
  let current = resolve(path)
  while (true) {
    const info = await lstat(current).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
    if (info?.isSymbolicLink() && !['/tmp', '/var', '/etc'].includes(current)) throw new Error(`Links are not allowed: ${current}`)
    const parent = dirname(current)
    if (parent === current) return
    current = parent
  }
}

async function removeOwnedLock(path, info, text) {
  const current = await lstat(path).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
  if (!current?.isFile() || current.dev !== info.dev || current.ino !== info.ino) return false
  const contents = await readFile(path, 'utf8').catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
  if (contents !== text) return false
  await unlink(path).catch(error => { if (error.code !== 'ENOENT') throw error })
  return true
}

async function acquireLock(path) {
  const text = JSON.stringify({ owner: 'herald-pi', pid: process.pid, token: randomUUID() })
  for (let attempt = 0; attempt < 3; attempt++) {
    await assertNoLinks(path)
    const handle = await open(path, 'wx').catch(error => { if (error.code === 'EEXIST') return undefined; throw error })
    if (handle) {
      const info = await handle.stat()
      try {
        await handle.writeFile(text)
      } catch (error) {
        await handle.close()
        if (!await removeOwnedLock(path, info, text)) await removeOwnedLock(path, info, '')
        throw error
      }
      await handle.close()
      return () => removeOwnedLock(path, info, text)
    }
    const info = await lstat(path).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
    if (!info) continue
    if (!info.isFile()) throw new Error(`Unsafe Pi registration lock: ${path}`)
    const previous = await readFile(path, 'utf8').catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
    if (previous === undefined) continue
    if (previous === '') {
      if (Date.now() - info.mtimeMs < 60_000) throw new Error(`Pi registration lock is still being initialized: ${path}`)
    } else {
      let owner
      try { owner = JSON.parse(previous) } catch { throw new Error(`Unknown Pi registration lock owner: ${path}`) }
      if (owner?.owner !== 'herald-pi' || typeof owner.token !== 'string' || !owner.token || !Number.isSafeInteger(owner.pid) || owner.pid <= 0) throw new Error(`Unknown Pi registration lock owner: ${path}`)
      try {
        process.kill(owner.pid, 0)
      } catch (error) {
        if (error.code !== 'ESRCH') throw new Error(`Cannot confirm Pi registration lock owner is dead: ${path}`, { cause: error })
        await removeOwnedLock(path, info, previous)
        continue
      }
      throw new Error(`Pi registration is already active: ${path}`)
    }
    await removeOwnedLock(path, info, previous)
  }
  throw new Error(`Cannot acquire Pi registration lock: ${path}`)
}

export async function registerPi(installed, directory = piAgentDirectory(), { dryRun = false, backupDirectory } = {}) {
  directory = piAgentDirectory(directory)
  const config = join(directory, 'settings.json')
  await assertNoLinks(config)
  await assertNoLinks(join(installed, 'pi-plugin', 'index.ts'))
  const bytes = await readFile(config).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
  const text = bytes?.toString('utf8') ?? '{}\n'
  const result = updatePiRegistration(text, directory, installed)
  const changed = result !== text
  if (!changed || dryRun) return { config, changed, dryRun }
  await mkdir(directory, { recursive: true })
  const lockPath = config + '.lock'
  const releaseLock = await acquireLock(lockPath)
  const stage = join(directory, `.herald-pi-${randomUUID()}.json`)
  try {
    const current = await readFile(config).catch(error => { if (error.code === 'ENOENT') return undefined; throw error })
    if (bytes ? !current?.equals(bytes) : current !== undefined) throw new Error('Pi settings changed during registration')
    if (backupDirectory && bytes) {
      await assertNoLinks(join(backupDirectory, 'pi-settings.json'))
      await mkdir(backupDirectory, { recursive: true })
      const backup = join(backupDirectory, 'pi-settings.json')
      await writeFile(backup, bytes, { flag: 'wx' }).catch(async error => {
        if (error.code !== 'EEXIST' || !(await readFile(backup)).equals(bytes)) throw error
      })
    }
    await writeFile(stage, result, { flag: 'wx' })
    await rename(stage, config)
    return { config, changed, dryRun }
  } finally {
    try { await rm(stage, { force: true }) } finally { await releaseLock() }
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = {}
  let installed
  let directory
  const args = process.argv.slice(2)
  for (let index = 0; index < args.length; index++) {
    const argument = args[index]
    if (argument === '--dry-run') options.dryRun = true
    else if (['--installed', '--agent-dir', '--backup-directory'].includes(argument)) {
      const value = args[++index]
      if (!value) throw new Error(`Missing value for ${argument}`)
      if (argument === '--installed') installed = value
      else if (argument === '--agent-dir') directory = value
      else options.backupDirectory = value
    } else throw new Error(`Unknown option: ${argument}`)
  }
  if (!installed) throw new Error('Expected --installed')
  console.log(JSON.stringify(await registerPi(installed, directory, options)))
}
