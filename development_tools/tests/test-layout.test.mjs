import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { test } from 'node:test'
import { testLayoutViolations } from '../check-test-layout.mjs'

async function crate(t, files) {
  const root = await mkdtemp(join(process.env.LOCALAPPDATA ? join(process.env.LOCALAPPDATA, 'Temp/opencode') : tmpdir(), 'test-layout-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  for (const [path, text] of Object.entries(files)) {
    await mkdir(dirname(join(root, path)), { recursive: true })
    await writeFile(join(root, path), text)
  }
  return root
}

const declared = path => `fn private() {}\n\n#[cfg(test)]\n#[path = "${path}"]\nmod tests;\n`

test('accepts tests declared from src and stored in the mirrored unit_tests file', async t => {
  const root = await crate(t, {
    'src/main.rs': `mod audio;\n${declared('../unit_tests/main.rs')}`,
    'src/settings_app/theme.rs': declared('../../unit_tests/settings_app/theme.rs'),
    'src/window.rs': 'mod native {\n    #[cfg(test)]\n    #[path = "../../../unit_tests/window.rs"]\n    mod tests;\n}\n',
    'unit_tests/main.rs': 'use super::*;\n\n#[test]\nfn works() {}\n',
    'unit_tests/settings_app/theme.rs': '#[test]\nfn works() {}\n',
    'unit_tests/window.rs': '#[test]\nfn works() {}\n',
  })
  assert.deepEqual(testLayoutViolations(root), [])
})

test('rejects inline test modules and test functions in src', async t => {
  const root = await crate(t, {
    'src/render.rs': 'fn draw() {}\n\n#[cfg(all(test, unix))]\nmod checks {\n    #[test]\n    fn draws() {}\n}\n',
  })
  assert.deepEqual(testLayoutViolations(root), [
    'src/render.rs:4: inline test module; move its body to unit_tests/render.rs',
    'src/render.rs:5: #[test] belongs in unit_tests/render.rs',
  ])
})

test('rejects test files that no source module declares, because they never run', async t => {
  const root = await crate(t, { 'src/main.rs': 'fn main() {}\n', 'unit_tests/main.rs': '#[test]\nfn lost() {}\n' })
  assert.deepEqual(testLayoutViolations(root), ['unit_tests/main.rs: not declared by src/main.rs, so its tests never run'])
})

test('rejects a test module that points somewhere other than its mirrored file', async t => {
  const root = await crate(t, { 'src/state.rs': declared('../unit_tests/other.rs'), 'unit_tests/other.rs': '' })
  assert.deepEqual(testLayoutViolations(root), [
    'src/state.rs:5: test module must be declared with #[path = "../unit_tests/state.rs"]',
    'unit_tests/other.rs: not declared by src/other.rs, so its tests never run',
  ])
})
