import { readFileSync } from 'node:fs'
import { dirname, join, resolve, isAbsolute } from 'node:path'
import { fileURLToPath } from 'node:url'
import { applyEdits, findNodeAtLocation, getNodeValue, modify, parseTree } from 'jsonc-parser/lib/esm/main.js'

export function parseJSONC(text) {
  const errors = []
  const tree = parseTree(text.replace(/^\ufeff/, ' '), errors, { allowTrailingComma: true })
  if (!tree || errors.length) throw new Error('Invalid JSONC configuration')
  function check(node) {
    if (node.type === 'object') {
      const keys = node.children.map(property => property.children[0].value)
      if (new Set(keys).size !== keys.length) throw new Error('Duplicate JSONC key')
    }
    for (const child of node.children ?? []) check(child)
  }
  check(tree)
  return { value: getNodeValue(tree) }
}

function ownedPackage(reference, directory, installed) {
  if (reference === installed) return true
  if (typeof reference !== 'string') return false
  let path
  if (reference.startsWith('file:')) path = fileURLToPath(reference)
  else if (isAbsolute(reference) || reference.startsWith('.')) path = resolve(directory, reference)
  else return false
  try {
    const metadata = JSON.parse(readFileSync(join(path, 'package.json'), 'utf8'))
    const entry = metadata.exports?.['.']
    const extension = entry === './opencode-plugin/index.js' ? 'js' : 'ts'
    if (metadata.name !== 'civilized-agent' || entry !== `./opencode-plugin/index.${extension}` || metadata.exports?.['./tui'] !== `./opencode-plugin/tui.${extension}`) return false
    return /id:\s*["']civilized-agent["']/.test(readFileSync(join(path, entry), 'utf8'))
  } catch { return false }
}

function mergeOptions(previous, next) {
  if (!next || Array.isArray(next) || typeof next !== 'object') return next
  const result = Object.assign(Object.create(null), previous && !Array.isArray(previous) && typeof previous === 'object' ? previous : {})
  for (const [key, value] of Object.entries(next)) result[key] = mergeOptions(result[key], value)
  return result
}

function registrations(root, directory, installed) {
  const plugins = root.value?.plugins
  if (plugins !== undefined && !Array.isArray(plugins)) throw new Error('Expected plugins to be an array')
  return (plugins ?? []).filter(value => ownedPackage(typeof value === 'string' ? value : value?.package, directory, installed)).map(value => ({ value }))
}

function combinedRegistration(entries, installed) {
  let result = installed
  for (const entry of entries) {
    if (typeof entry.value === 'object' && Object.keys(entry.value).some(key => key !== 'package')) result = mergeOptions(typeof result === 'string' ? {} : result, entry.value)
  }
  if (typeof result === 'object') result.package = installed
  return result
}

function editValue(text, path, value) {
  const current = path.reduce((node, key) => node?.[key], parseJSONC(text).value)
  if (current && value && !Array.isArray(current) && !Array.isArray(value) && typeof current === 'object' && typeof value === 'object') {
    for (const [key, item] of Object.entries(value)) {
      text = editValue(text, [...path, key], item)
    }
    return text
  }
  if (JSON.stringify(current) === JSON.stringify(value)) return text
  if (value === undefined) {
    const array = findNodeAtLocation(parseTree(text.replace(/^\ufeff/, ' '), [], { allowTrailingComma: true }), path.slice(0, -1))
    const index = path.at(-1)
    const node = array.children[index]
    const next = array.children[index + 1]
    const previous = array.children[index - 1]
    const start = !next && previous ? previous.offset + previous.length : node.offset
    const end = next ? next.offset : array.offset + array.length - 1
    return applyEdits(text, [{ offset: start, length: end - start, content: '' }])
  }
  return applyEdits(text, modify(text.replace(/^\ufeff/, ' '), path, value, { formattingOptions: { insertSpaces: true, tabSize: 2, eol: text.includes('\r\n') ? '\r\n' : '\n' } }))
}

export function updateRegistration(text, configDirectory, installed, remove = false, registration) {
  const bom = text.startsWith('\ufeff') ? '\ufeff' : ''
  return bom + renderRegistration(text.slice(bom.length), configDirectory, installed, remove, registration)
}

function renderRegistration(text, configDirectory, installed, remove, registration) {
  const root = parseJSONC(text)
  if (!root.value || Array.isArray(root.value) || typeof root.value !== 'object') throw new Error('Expected a JSONC configuration object')
  const plugins = root.value.plugins
  const owned = registrations(root, configDirectory, installed)
  registration ??= combinedRegistration(owned, installed)
  if (!plugins) return remove ? text : editValue(text, ['plugins'], [registration])
  const target = owned.findLast(child => child.value && typeof child.value === 'object') ?? owned.at(-1)
  const targetIndex = target ? plugins.lastIndexOf(target.value) : -1
  if (!remove && targetIndex >= 0) text = editValue(text, ['plugins', targetIndex], registration)
  for (let index = plugins.length - 1; index >= 0; index--) {
    if (!owned.some(child => child.value === plugins[index]) || (!remove && index === targetIndex)) continue
    text = editValue(text, ['plugins', index], undefined)
  }
  if (!remove && targetIndex < 0) text = editValue(text, ['plugins', plugins.length], registration)
  parseJSONC(text)
  return text
}

export function updateRegistrations(documents, installed) {
  const entries = documents.map(document => registrations(parseJSONC(document.text), dirname(document.config), installed))
  const primary = entries.findLastIndex(items => items.length)
  const registration = combinedRegistration(entries.flat(), installed)
  return documents.map((document, index) => ({ ...document, result: updateRegistration(document.text, dirname(document.config), installed, index !== (primary < 0 ? documents.length - 1 : primary), registration) }))
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.slice(2).join(' ') !== '--render') throw new Error('Expected --render')
  const request = JSON.parse(readFileSync(0, 'utf8'))
  process.stdout.write(JSON.stringify(updateRegistrations(request.documents, request.installed.replaceAll('\\', '/'))))
}
