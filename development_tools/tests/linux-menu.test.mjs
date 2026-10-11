import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { setTimeout } from 'node:timers/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { installLinuxMenu } from '../install-linux-menu.mjs'

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'herald-menu-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  return directory
}

async function runtimeFixture(t, name = 'runtime') {
  const root = join(await fixture(t), name)
  const bin = join(root, 'native-announcer/bin')
  const portraits = join(root, 'native-announcer/resources/portraits')
  await mkdir(bin, { recursive: true })
  await mkdir(portraits, { recursive: true })
  await writeFile(join(bin, `herald-linux-${process.arch}`), 'native executable fixture', { mode: 0o755 })
  await writeFile(join(portraits, 'flamboyant-herald.png'), 'icon fixture')
  return root
}

test('installs a repeatable settings entry without replacing other applications', { skip: process.platform !== 'linux' }, async t => {
  const root = await runtimeFixture(t)
  const directory = await fixture(t)
  const applications = join(directory, 'applications')
  await mkdir(applications)
  await writeFile(join(applications, 'other.desktop'), 'unrelated')
  const env = { XDG_DATA_HOME: directory }
  const path = await installLinuxMenu(root, { env, refresh: false })
  const content = await readFile(path, 'utf8')
  assert.ok(content.includes('Name=Herald Settings\n'))
  assert.ok(content.includes(`Exec=/usr/bin/env -- "${join(root, `native-announcer/bin/herald-linux-${process.arch}`)}" --settings --assets "${join(root, 'native-announcer/resources')}"\n`))
  assert.equal(content.includes('editor'), false)
  assert.ok(content.includes(`Icon=${join(root, 'native-announcer/resources/portraits/flamboyant-herald.png')}\n`))
  assert.ok(content.includes('Terminal=false\n'))
  await installLinuxMenu(root, { env, refresh: false })
  assert.equal(await readFile(path, 'utf8'), content)
  assert.equal(await readFile(join(applications, 'other.desktop'), 'utf8'), 'unrelated')
})

test('installing the graphical launcher preserves existing settings', { skip: process.platform !== 'linux' }, async t => {
  const root = await runtimeFixture(t)
  const directory = await fixture(t)
  const data = join(directory, 'herald')
  await mkdir(data)
  const settings = '{"volume":37}\n'
  await writeFile(join(data, 'settings.json'), settings)
  const path = await installLinuxMenu(root, { env: { XDG_DATA_HOME: directory }, refresh: false })
  assert.equal(path, join(directory, 'applications/herald-settings.desktop'))
  assert.ok((await readFile(path, 'utf8')).includes(' --settings --assets '))
  assert.equal(await readFile(join(data, 'settings.json'), 'utf8'), settings)
})

test('rejects an installation without a native settings executable', { skip: process.platform !== 'linux' }, async t => {
  const directory = await fixture(t)
  await assert.rejects(installLinuxMenu(directory, { env: { XDG_DATA_HOME: directory }, refresh: false }), { code: 'ENOENT' })
})

test('the desktop launcher preserves native arguments in paths with special characters', { skip: process.platform !== 'linux' }, async t => {
  const root = await runtimeFixture(t, 'runtime with % " $ ` \\ characters')
  const directory = await fixture(t)
  const result = join(directory, 'arguments')
  const binary = join(root, `native-announcer/bin/herald-linux-${process.arch}`)
  await writeFile(binary, '#!/bin/sh\nprintf "%s\\n" "$0" "$@" > "$RESULT"\n')
  const path = await installLinuxMenu(root, { env: { XDG_DATA_HOME: directory }, refresh: false })
  const launch = spawnSync('gio', ['launch', path], { env: { ...process.env, RESULT: result }, encoding: 'utf8', timeout: 10000 })
  assert.equal(launch.status, 0, launch.stderr)
  let argumentsText
  for (let attempt = 0; attempt < 100; attempt++) {
    argumentsText = await readFile(result, 'utf8').catch(error => {
      if (error.code === 'ENOENT') return undefined
      throw error
    })
    if (argumentsText) break
    await setTimeout(10)
  }
  assert.equal(argumentsText, `${binary}\n--settings\n--assets\n${join(root, 'native-announcer/resources')}\n`)
})

test('rejects paths that would inject desktop entry lines', { skip: process.platform !== 'linux' }, async t => {
  const directory = await fixture(t)
  await assert.rejects(installLinuxMenu(join(directory, 'runtime\nExec=other'), { env: { XDG_DATA_HOME: directory }, refresh: false }), /must not contain carriage returns or newlines/)
})
