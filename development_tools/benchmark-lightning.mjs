// Benchmark every Lightning frame: release build, repeated alternating runs, per-phase percentiles, a stage breakdown from
// a separate profiled run, output digests that must not change, and an optional live jitter run of the real announcer.
// Usage: node development_tools/benchmark-lightning.mjs [--runs 5] [--cycles 3] [--scale 2] [--fps 120] [--live] [--out dir]
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cpus, loadavg, tmpdir } from 'node:os'

const root = fileURLToPath(new URL('..', import.meta.url))
const crate = join(root, 'native-announcer')
const args = process.argv.slice(2)
const option = (name, fallback) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : fallback }
const runs = Number(option('--runs', 5))
const cycles = option('--cycles', '3')
const scale = option('--scale', '2')
const fps = option('--fps', '120')
const out = resolve(option('--out', join(root, 'temp/benchmark-lightning', new Date().toISOString().replace(/[:.]/g, '-'))))
const live = args.includes('--live')
const liveSeconds = Number(option('--live-seconds', 14))
mkdirSync(out, { recursive: true })

const build = spawnSync('cargo', ['build', '--release', '--locked', '--manifest-path', join(crate, 'Cargo.toml')], { stdio: 'inherit' })
if (build.status !== 0) process.exit(build.status ?? 1)
const binary = join(crate, 'target/release/herald')
const assets = join(crate, 'resources')

const percentile = (sorted, p) => sorted.length ? sorted[Math.min(sorted.length - 1, Math.ceil(p / 100 * sorted.length) - 1)] : NaN
const stats = values => {
  const sorted = [...values].sort((a, b) => a - b)
  return { n: sorted.length, p50: percentile(sorted, 50), p95: percentile(sorted, 95), p99: percentile(sorted, 99), max: sorted.at(-1) ?? NaN }
}
const median = values => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.floor(sorted.length / 2)] }
const f = value => Number.isFinite(value) ? value.toFixed(2) : '-'

function bench(extra, name) {
  const file = join(out, `${name}.json`)
  const result = spawnSync(binary, ['--assets', assets, '--benchmark-lightning', file, '--cycles', cycles, '--scale', scale, '--fps', fps, ...extra], { stdio: 'inherit', env: { ...process.env, HERALD_DATA: join(out, 'data') } })
  if (result.status !== 0) throw new Error(`benchmark ${name} failed with ${result.status}`)
  return JSON.parse(readFileSync(file, 'utf8'))
}

const lines = []
const say = line => { lines.push(line); console.log(line) }
say(`# Lightning benchmark ${new Date().toISOString()}`)
say(`machine: ${cpus()[0]?.model} x${cpus().length}, load ${loadavg().map(v => v.toFixed(2)).join(' ')}, scale ${scale}, cadence ${fps} fps, ${cycles} cycles/run, ${runs} runs`)

const liveOnly = args.includes('--live-only')
let digests = {}, mismatched = 0, summary = {}
if (!liveOnly) {
// Timed runs alternate the two targets so drift and thermal state hit both; the profiled run is reported separately.
const timed = []
for (let run = 0; run < runs; run++) {
  for (const target of run % 2 ? ['preview', 'announcement'] : ['announcement', 'preview']) timed.push({ run, target, report: bench(['--targets', target], `timed-${run}-${target}`) })
}
for (const { report } of timed) for (const [key, digest] of Object.entries(report.digests)) {
  if (digests[key] && digests[key] !== digest) mismatched++
  digests[key] = digest
}
say(`build: ${timed[0].report.build}; frames per run: ${timed.map(t => t.report.frames.length).join(',')}; digest mismatches across runs: ${mismatched}`)
say(`digests: ${JSON.stringify(digests)}`)
say(`decoded video frames per run: ${JSON.stringify(timed[0].report.decodedVideoFrames)}`)

const warm = frame => frame.cycle > 0
const groups = new Map()
for (const { run, report } of timed) for (const frame of report.frames) {
  if (!warm(frame)) continue
  for (const phase of [frame.phase + (frame.phase === 'holding' ? (frame.burst ? ' burst' : ' calm') : ''), 'all']) {
    const key = `${frame.target}|${frame.preset}|${phase}`
    if (!groups.has(key)) groups.set(key, { all: [], runs: new Map() })
    const group = groups.get(key)
    group.all.push(frame.ms)
    if (!group.runs.has(run)) group.runs.set(run, [])
    group.runs.get(run).push(frame.ms)
  }
}
say('\n## Frame time per phase (warm cycles, all runs pooled; p99 range = min..max of per-run p99)')
say('| target | preset | phase | frames | p50 ms | p95 ms | p99 ms | p99 range | max ms | fps at p99 |')
say('|---|---|---|---|---|---|---|---|---|---|')
for (const [key, group] of [...groups].sort()) {
  const [target, preset, phase] = key.split('|')
  const s = stats(group.all)
  const perRun = [...group.runs.values()].map(values => stats(values).p99)
  summary[key] = { ...s, p99Runs: perRun }
  say(`| ${target} | ${preset} | ${phase} | ${s.n} | ${f(s.p50)} | ${f(s.p95)} | ${f(s.p99)} | ${f(Math.min(...perRun))}..${f(Math.max(...perRun))} | ${f(s.max)} | ${f(1000 / s.p99)} |`)
}
const cold = timed.flatMap(t => t.report.frames.filter(frame => frame.cycle === 0 && frame.at === 0).map(frame => frame.ms))
say(`\ncold first frame (includes the first video read): median ${f(median(cold))} ms, max ${f(Math.max(...cold))} ms`)

// Stage breakdown from one profiled run (timers add overhead, so these numbers are not mixed into the table above).
const profiled = bench(['--profile'], 'profiled')
const stageNames = Object.keys(profiled.frames.find(frame => frame.stages)?.stages ?? {})
const derived = frame => {
  const s = frame.stages
  const inner = (s.contours ?? 0) + (s.geometry ?? 0) + (s.distance ?? 0) + (s.composite ?? 0)
  return { ...s, raster: Math.max(0, (s.lightning ?? 0) - inner), other: Math.max(0, frame.ms - stageNames.filter(n => !['contours', 'geometry', 'distance', 'composite'].includes(n)).reduce((sum, n) => sum + (s[n] ?? 0), 0)) }
}
const columns = ['video', 'bubble', 'text', 'scanlines', 'portrait', 'contours', 'geometry', 'raster', 'distance', 'composite', 'canvas', 'present', 'other']
say('\n## Stage breakdown, mean ms per frame (profiled run, warm cycles; raster = lightning minus contours, geometry, distance and composite)')
say(`| target | phase | ${columns.join(' | ')} | total |`)
say(`|---|---|${columns.map(() => '---').join('|')}|---|`)
const stageGroups = new Map()
for (const frame of profiled.frames.filter(warm)) {
  const phase = frame.phase + (frame.phase === 'holding' ? (frame.burst ? ' burst' : ' calm') : '')
  const key = `${frame.target}|${phase}`
  if (!stageGroups.has(key)) stageGroups.set(key, [])
  stageGroups.get(key).push({ ...derived(frame), total: frame.ms })
}
for (const [key, frames] of [...stageGroups].sort()) {
  const [target, phase] = key.split('|')
  const mean = name => frames.reduce((sum, frame) => sum + (frame[name] ?? 0), 0) / frames.length
  say(`| ${target} | ${phase} | ${columns.map(name => f(mean(name))).join(' | ')} | ${f(mean('total'))} |`)
}
const videoReads = profiled.frames.map(frame => frame.stages?.video ?? 0).filter(value => value > 0)
say(`\nvideo decode reads (pipe read of one 128x128 frame): n ${videoReads.length}, p50 ${f(stats(videoReads).p50)} ms, p99 ${f(stats(videoReads).p99)} ms, max ${f(stats(videoReads).max)} ms`)

}

if (live || liveOnly) {
  // Live run: the real announcer window with video and Lightning, isolated data, instrumented with HERALD_FRAME_LOG.
  const data = join(tmpdir(), `herald-live-${process.pid}`)
  rmSync(data, { recursive: true, force: true }); mkdirSync(data, { recursive: true })
  writeFileSync(join(data, 'settings.json'), JSON.stringify({ volume: 0 }))
  const log = join(out, 'live-frames.jsonl')
  rmSync(log, { force: true })
  const result = spawnSync(binary, ['--isolated', '--demo', 'claude', '--test-seconds', String(liveSeconds), '--assets', assets],
    { stdio: 'inherit', env: { ...process.env, HERALD_DATA: data, HERALD_FRAME_LOG: log } })
  say(`\n## Live announcer run (${liveSeconds}s, exit ${result.status})`)
  if (existsSync(log)) {
    const frames = readFileSync(log, 'utf8').trim().split('\n').map(line => JSON.parse(line))
    writeFileSync(join(out, 'live-summary.json'), JSON.stringify(liveSummary(frames), null, 2))
    for (const line of liveLines(liveSummary(frames))) say(line)
  } else say('no frame log was written')
  rmSync(data, { recursive: true, force: true })

  // Live preview run: the Settings window's inline Lightning preview, worker frames and frames the UI took.
  const previewLog = join(out, 'live-preview.jsonl')
  rmSync(previewLog, { force: true })
  const preview = spawnSync('python3', [join(root, 'development_tools/benchmark-lightning-preview.py'), binary, assets, previewLog, String(liveSeconds)], { stdio: 'inherit' })
  say(`\n## Live Settings preview run (${liveSeconds}s, exit ${preview.status})`)
  if (existsSync(previewLog)) {
    const frames = readFileSync(previewLog, 'utf8').trim().split('\n').map(line => JSON.parse(line))
    for (const kind of ['preview-worker', 'preview-ui']) {
      const summary = liveSummary(frames.filter(frame => frame.kind === kind), 16)
      writeFileSync(join(out, `live-${kind}.json`), JSON.stringify(summary, null, 2))
      say(`\n${kind}`)
      for (const line of liveLines(summary)) say(line)
    }
  } else say('no preview frame log was written')
}

export function liveSummary(frames, fixedTarget) {
  const byPhase = new Map()
  for (const frame of frames) {
    const phase = ['leader', 'impact', 'propagation', 'decay'].includes(frame.phase) ? 'entrance' : frame.phase
    if (!byPhase.has(phase)) byPhase.set(phase, [])
    byPhase.get(phase).push(frame)
  }
  return Object.fromEntries([...byPhase].map(([phase, list]) => {
    const target = fixedTarget ?? (phase === 'holding' ? 33 : 16)
    const intervals = list.map(frame => frame.interval).filter(value => value != null)
    return [phase, { frames: list.length, targetInterval: target, work: stats(list.map(frame => frame.work)), interval: stats(intervals),
      late: stats(list.map(frame => frame.late)), dropped: intervals.reduce((sum, value) => sum + Math.max(0, Math.round(value / target) - 1), 0),
      present: stats(list.map(frame => frame.stages?.present ?? 0)), video: stats(list.map(frame => frame.stages?.video ?? 0).filter(v => v > 0)) }]
  }))
}

export function liveLines(summary) {
  const result = ['| phase | frames | target interval | interval p50 | p99 | max | work p50 | work p99 | work max | wake late p99 | present p99 | dropped |', '|---|---|---|---|---|---|---|---|---|---|---|---|']
  for (const [phase, s] of Object.entries(summary)) result.push(`| ${phase} | ${s.frames} | ${s.targetInterval} | ${f(s.interval.p50)} | ${f(s.interval.p99)} | ${f(s.interval.max)} | ${f(s.work.p50)} | ${f(s.work.p99)} | ${f(s.work.max)} | ${f(s.late.p99)} | ${f(s.present.p99)} | ${s.dropped} |`)
  return result
}

writeFileSync(join(out, 'summary.json'), JSON.stringify({ digests, mismatched, summary }, null, 2))
writeFileSync(join(out, 'report.md'), lines.join('\n') + '\n')
console.log(`\nwrote ${join(out, 'report.md')}`)
if (mismatched) process.exit(2)
