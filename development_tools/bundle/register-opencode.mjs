import { readFileSync } from 'node:fs'
import { dirname, join, resolve, isAbsolute } from 'node:path'
import { fileURLToPath } from 'node:url'

export function parseJSONC(text) {
  const tokens = [...text.matchAll(/\s+|\/\/[^\r\n]*|\/\*[\s\S]*?\*\/|"(?:\\.|[^"\\])*"|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|true\b|false\b|null\b|[{}[\]:,]|./g)]
    .filter(([token]) => !/^\s|^\/\//.test(token) && !token.startsWith('/*'))
  let index = 0
  function take(expected) {
    const token = tokens[index++]
    if (!token || (expected && token[0] !== expected)) throw new Error('Invalid JSONC configuration')
    return token
  }
  function value() {
    const token = take()
    const node = { start: token.index, end: token.index + token[0].length, value: undefined, children: [] }
    if (token[0] === '{' || token[0] === '[') {
      const object = token[0] === '{'
      const close = object ? '}' : ']'
      node.value = object ? Object.create(null) : []
      while (tokens[index]?.[0] !== close) {
        let key
        if (object) { key = JSON.parse(take()[0]); if (typeof key !== 'string' || Object.hasOwn(node.value, key)) throw new Error('Duplicate or invalid JSONC key'); take(':') }
        const child = value()
        child.key = key
        node.children.push(child)
        if (object) node.value[key] = child.value
        else node.value.push(child.value)
        if (tokens[index]?.[0] !== ',') break
        child.comma = take(',').index
      }
      const end = take(close)
      node.end = end.index + 1
    } else { node.value = JSON.parse(token[0]) }
    return node
  }
  const node = value()
  if (index !== tokens.length) throw new Error('Invalid JSONC configuration')
  return node
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
    if (metadata.name !== 'civilized-agent' || metadata.exports?.['.'] !== './opencode-plugin/index.ts' || metadata.exports?.['./tui'] !== './opencode-plugin/tui.ts') return false
    return /id:\s*["']civilized-agent["']/.test(readFileSync(join(path, 'opencode-plugin/index.ts'), 'utf8'))
  } catch { return false }
}

function mergeOptions(previous, next) {
  if (!next || Array.isArray(next) || typeof next !== 'object') return next
  const result = Object.assign(Object.create(null), previous && !Array.isArray(previous) && typeof previous === 'object' ? previous : {})
  for (const [key, value] of Object.entries(next)) result[key] = mergeOptions(result[key], value)
  return result
}

function registrations(root, directory, installed) {
  return root.children.find(child => child.key === 'plugins')?.children.filter(child => ownedPackage(typeof child.value === 'string' ? child.value : child.value?.package, directory, installed)) ?? []
}

function combinedRegistration(entries, installed) {
  let result = installed
  for (const entry of entries) {
    if (typeof entry.value === 'object' && Object.keys(entry.value).some(key => key !== 'package')) result = mergeOptions(typeof result === 'string' ? {} : result, entry.value)
  }
  if (typeof result === 'object') result.package = installed
  return result
}

function editValue(node, value, edits) {
  if (node.value && value && !Array.isArray(node.value) && !Array.isArray(value) && typeof node.value === 'object' && typeof value === 'object') {
    const missing = []
    for (const [key, item] of Object.entries(value)) {
      const child = node.children.find(child => child.key === key)
      if (child) editValue(child, item, edits)
      else missing.push(`${JSON.stringify(key)}: ${JSON.stringify(item)}`)
    }
    if (missing.length) {
      const last = node.children.at(-1)
      edits.push({ start: node.end - 1, end: node.end - 1, text: `${last && last.comma === undefined ? ',' : ''}${missing.join(',')}` })
    }
  } else if (JSON.stringify(node.value) !== JSON.stringify(value)) edits.push({ start: node.start, end: node.end, text: JSON.stringify(value) })
}

export function updateRegistration(text, configDirectory, installed, remove = false, registration) {
  const root = parseJSONC(text)
  if (!root.value || Array.isArray(root.value) || typeof root.value !== 'object') throw new Error('Expected a JSONC configuration object')
  const plugins = root.children.find(child => child.key === 'plugins')
  const owned = registrations(root, configDirectory, installed)
  registration ??= combinedRegistration(owned, installed)
  const targetEntry = owned.findLast(child => typeof child.value === 'object') ?? owned.at(-1)
  const edits = []
  if (!plugins) {
    if (remove) return text
    const last = root.children.at(-1)
    edits.push({ start: root.end - 1, end: root.end - 1, text: `${last && last.comma === undefined ? ',' : ''}\n  "plugins": [${JSON.stringify(registration)}]\n` })
  } else {
    if (!Array.isArray(plugins.value)) throw new Error('Expected plugins to be an array')
    let retained = false
    for (const [index, child] of plugins.children.entries()) {
      const reference = typeof child.value === 'string' ? child.value : child.value?.package
      if (!ownedPackage(reference, configDirectory, installed)) continue
      if (child === targetEntry && !remove) {
        retained = true
        editValue(child, registration, edits)
      } else {
        const previous = plugins.children[index - 1]
        edits.push(child.comma !== undefined
          ? { start: child.start, end: child.comma + 1, text: '' }
          : { start: previous?.comma ?? child.start, end: child.end, text: '' })
      }
    }
    if (!retained && !remove) {
      const last = plugins.children.at(-1)
      edits.push({ start: plugins.end - 1, end: plugins.end - 1, text: `${last && last.comma === undefined ? ',' : ''}\n    ${JSON.stringify(registration)}\n  ` })
    }
  }
  const merged = []
  for (const edit of edits.sort((a, b) => a.start - b.start)) {
    const previous = merged.at(-1)
    if (previous && !previous.text && !edit.text && edit.start <= previous.end) previous.end = Math.max(previous.end, edit.end)
    else merged.push(edit)
  }
  let result = text
  for (const edit of merged.reverse()) result = result.slice(0, edit.start) + edit.text + result.slice(edit.end)
  parseJSONC(result)
  return result
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
