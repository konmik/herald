// Fails when Rust unit tests live inside native-announcer/src instead of native-announcer/unit_tests.
// Convention: the tests of src/<module>.rs live in unit_tests/<module>.rs and are declared at the
// end of the module as `#[cfg(test)] #[path = "<relative>/unit_tests/<module>.rs"] mod tests;`.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { join, posix, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const rustFiles = directory => existsSync(directory) ? readdirSync(directory).flatMap(name => {
  const path = join(directory, name)
  return statSync(path).isDirectory() ? rustFiles(path) : name.endsWith('.rs') ? [path] : []
}).sort() : []

const modulePath = (root, file) => relative(root, file).split('\\').join('/')

// rustc resolves #[path] from the source file's directory, or from <dir>/<stem>/<inline modules>
// when the declaration is nested inside inline modules.
function expectedPath(module, lines, index) {
  const enclosing = []
  for (let line = index - 1, depth = lines[index].match(/^\s*/)[0].length; line >= 0 && depth > 0; line--) {
    const match = lines[line].match(/^(\s*)(?:pub(?:\([^)]*\))?\s+)?mod (\w+) \{\s*$/)
    if (match && match[1].length < depth) { enclosing.unshift(match[2]); depth = match[1].length }
  }
  const directory = posix.dirname(posix.join('src', module))
  const base = enclosing.length ? posix.join(directory, posix.basename(module, '.rs'), ...enclosing) : directory
  return posix.relative(base, posix.join('unit_tests', module))
}

export function testLayoutViolations(crate) {
  const source = join(crate, 'src')
  const tests = join(crate, 'unit_tests')
  const violations = []
  const declared = new Set()
  for (const file of rustFiles(source)) {
    const module = modulePath(source, file)
    const lines = readFileSync(file, 'utf8').split('\n')
    lines.forEach((line, index) => {
      const code = line.replace(/\/\/.*$/, '')
      if (/#\[(?:\w+::)*test\]/.test(code)) violations.push(`src/${module}:${index + 1}: #[test] belongs in unit_tests/${module}`)
      const name = code.match(/^\s*(?:pub(?:\([^)]*\))?\s+)?mod (\w+)\s*(?:\{|;)/)?.[1]
      if (!name) return
      let first = index
      while (first > 0 && lines[first - 1].trim().startsWith('#[')) first--
      const attributes = lines.slice(first, index)
      // A test module is named `tests` or compiled only for tests (`cfg(test)`, `cfg(all(test, ...))`).
      const testOnly = attributes.some(attribute => /^\s*#\[cfg\((?:test|all\(test\b.*)\)\]/.test(attribute))
      if (name !== 'tests' && !testOnly) return
      if (code.includes('{')) { violations.push(`src/${module}:${index + 1}: inline test module; move its body to unit_tests/${module}`); return }
      const path = attributes.map(attribute => attribute.match(/^\s*#\[path = "([^"]+)"\]/)?.[1]).find(Boolean)
      const expected = expectedPath(module, lines, index)
      if (path !== expected) violations.push(`src/${module}:${index + 1}: test module must be declared with #[path = "${expected}"]`)
      else if (!existsSync(join(tests, module))) violations.push(`src/${module}:${index + 1}: unit_tests/${module} does not exist`)
      else if (declared.has(module)) violations.push(`src/${module}:${index + 1}: unit_tests/${module} is declared twice`)
      else declared.add(module)
    })
  }
  for (const file of rustFiles(tests)) {
    const module = modulePath(tests, file)
    if (!declared.has(module)) violations.push(`unit_tests/${module}: not declared by src/${module}, so its tests never run`)
  }
  return violations
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const crate = process.argv[2] ?? 'native-announcer'
  const violations = testLayoutViolations(crate)
  for (const violation of violations) console.error(violation)
  if (violations.length) process.exit(1)
  console.log(`${crate}: unit tests are outside src`)
}
