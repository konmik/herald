import assert from 'node:assert/strict'
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { installLinuxMenu } from '../install-linux-menu.mjs'

const root = fileURLToPath(new URL('../..', import.meta.url))

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'herald-menu-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  return directory
}

test('installs a repeatable settings entry without replacing other applications', { skip: process.platform !== 'linux' }, async t => {
  const directory = await fixture(t)
  const applications = join(directory, 'applications')
  await mkdir(applications)
  await writeFile(join(applications, 'other.desktop'), 'unrelated')
  const env = { XDG_DATA_HOME: directory }
  const path = await installLinuxMenu(root, { env, refresh: false })
  const content = await readFile(path, 'utf8')
  assert.ok(content.includes('Name=Herald Settings\n'))
  assert.ok(content.includes(`Exec=sh "${join(root, 'development_tools/open-linux-settings.sh')}"\n`))
  assert.ok(content.includes(`Icon=${join(root, 'native-announcer/resources/portraits/flamboyant-herald.png')}\n`))
  assert.ok(content.includes('Terminal=false\n'))
  await installLinuxMenu(root, { env, refresh: false })
  assert.equal(await readFile(path, 'utf8'), content)
  assert.equal(await readFile(join(applications, 'other.desktop'), 'utf8'), 'unrelated')
})

test('opens existing settings using the Omarchy editor without altering them', { skip: process.platform !== 'linux' }, async t => {
  const directory = await fixture(t)
  const bin = join(directory, 'bin')
  const data = join(directory, 'settings with spaces')
  const result = join(directory, 'arguments')
  await mkdir(bin)
  await mkdir(data)
  const settings = '{"volume":37}\n'
  await writeFile(join(data, 'settings.json'), settings)
  const editor = join(bin, 'omarchy')
  await writeFile(editor, '#!/bin/sh\nprintf "%s\\n" "$@" > "$RESULT"\n')
  await chmod(editor, 0o755)
  const child = spawnSync('/bin/sh', [join(root, 'development_tools/open-linux-settings.sh')], {
    env: { PATH: `${bin}:/usr/bin:/bin`, HERALD_DATA: data, RESULT: result }, encoding: 'utf8',
  })
  assert.equal(child.status, 0, child.stderr)
  assert.equal(await readFile(result, 'utf8'), `launch\neditor\n${data}/settings.json\n`)
  assert.equal(await readFile(join(data, 'settings.json'), 'utf8'), settings)
})

test('initializes missing settings under XDG_DATA_HOME before opening the editor', { skip: process.platform !== 'linux' }, async t => {
  const directory = await fixture(t)
  const bin = join(directory, 'bin')
  const data = join(directory, 'data')
  const result = join(directory, 'arguments')
  await mkdir(bin)
  const editor = join(bin, 'omarchy')
  await writeFile(editor, '#!/bin/sh\nprintf "%s\\n" "$@" > "$RESULT"\n')
  await chmod(editor, 0o755)
  const child = spawnSync('/bin/sh', [join(root, 'development_tools/open-linux-settings.sh')], {
    env: { PATH: `${bin}:/usr/bin:/bin`, XDG_DATA_HOME: data, RESULT: result }, encoding: 'utf8',
  })
  assert.equal(child.status, 0, child.stderr)
  assert.equal(await readFile(result, 'utf8'), `launch\neditor\n${data}/herald/settings.json\n`)
  assert.equal(await readFile(join(data, 'herald/settings.json'), 'utf8'), '{}\n')
})
