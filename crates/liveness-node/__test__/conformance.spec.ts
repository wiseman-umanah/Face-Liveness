// Runs the shared cases in tests/conformance/cases.json (the Rust core and the Python
// binding run the same file) and checks Node gets identical results.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import test from 'ava'

import { Engine, Session } from '../index.js'

const root = join(import.meta.dirname, '../../..')
const cases = JSON.parse(readFileSync(join(root, 'tests/conformance/cases.json'), 'utf8'))

function close(expected: any, actual: any, tol: number): boolean {
  if (expected && typeof expected === 'object') {
    const keys = Object.keys(expected)
    return keys.length === Object.keys(actual ?? {}).length && keys.every((k) => close(expected[k], actual[k], tol))
  }
  return Math.abs(expected - actual) <= tol
}

function check(t: any, expected: any, actual: any) {
  const { signals: es, ...eRest } = expected
  const { signals: as, ...aRest } = actual
  if (es != null) t.true(close(es, as, cases.tolerance), `signals ${JSON.stringify(as)} vs ${JSON.stringify(es)}`)
  t.deepEqual(aRest, eRest)
}

for (const c of cases.session_cases) {
  test(`session: ${c.name}`, (t) => {
    const session = new Session(c.challenges, {
      totalTimeoutMs: c.config.total_timeout_ms,
      stepTimeoutMs: c.config.step_timeout_ms,
      faceLostMs: c.config.face_lost_ms,
    })
    const rows = readFileSync(join(root, c.recording), 'utf8').trim().split('\n').slice(1)
    const actions: [number, string][] = []
    let result: any
    let ts = 0
    for (const row of rows) {
      const [tsText, ear, mouth, yaw] = row.split(',')
      ts = Number(tsText)
      if (ts < c.from_ms) continue
      result = session.update(ts, { faceCount: 1, ear: Number(ear), mouth: Number(mouth), yaw: Number(yaw) })
      for (const a of result.actions) actions.push([ts, a])
      if (result.done) break
    }
    check(t, c.expected, {
      status: result.status,
      fail_reason: result.failReason ?? null,
      at_ms: result.done ? ts : null,
      actions,
    })
  })
}

for (const c of cases.engine_cases) {
  test(`engine: ${c.name}`, (t) => {
    const data = readFileSync(join(root, c.image))
    if (c.format === 'bgr') {
      for (let i = 0; i < data.length; i += 3) [data[i], data[i + 2]] = [data[i + 2], data[i]]
    }
    const engine = new Engine(c.challenges, {
      totalTimeoutMs: c.config.total_timeout_ms,
      stepTimeoutMs: c.config.step_timeout_ms,
      faceLostMs: c.config.face_lost_ms,
    })
    const faceCounts: number[] = []
    let signals: any = null
    let result: any
    let ts = 0
    for (let i = 0; i < c.frames; i++) {
      ts = i * c.interval_ms
      result = engine.process(data, { width: c.width, height: c.height, format: c.format, timestampMs: ts })
      faceCounts.push(result.faceCount)
      if (i === 0 && result.signals) signals = result.signals
      if (result.done) break
    }
    check(t, c.expected, {
      status: result.status,
      fail_reason: result.failReason ?? null,
      at_ms: result.done ? ts : null,
      face_counts: faceCounts,
      signals,
    })
  })
}
