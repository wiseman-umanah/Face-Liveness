// Runs the shared cases in tests/conformance/cases.json (the Rust core and the Python
// binding run the same file) and checks Node gets identical results.
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import test from 'ava'

import { Engine, Session, verifyFaceRecord, verifyReceipt } from '../index.js'

const root = join(import.meta.dirname, '../../..')
const cases = JSON.parse(readFileSync(join(root, 'tests/conformance/cases.json'), 'utf8'))

function close(expected: any, actual: any, tol: number): boolean {
  if (expected && typeof expected === 'object') {
    const keys = Object.keys(expected)
    return keys.length === Object.keys(actual ?? {}).length && keys.every((k) => close(expected[k], actual[k], tol))
  }
  return Math.abs(expected - actual) <= tol
}

// Numbers under these keys may differ by the shared tolerance; everything else must match exactly.
const APPROX = ['signals', 'real_score', 'face_quality']

function check(t: any, expected: any, actual: any) {
  const strip = (o: any) => Object.fromEntries(Object.entries(o).filter(([k]) => !APPROX.includes(k)))
  for (const key of APPROX) {
    if (expected[key] != null) {
      t.true(close(expected[key], actual[key], cases.tolerance), `${key} ${JSON.stringify(actual[key])} vs ${JSON.stringify(expected[key])}`)
    } else {
      t.is(actual[key] ?? null, expected[key] ?? null, key)
    }
  }
  t.deepEqual(strip(actual), strip(expected))
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
    let realScore: number | null = null
    let result: any
    let ts = 0
    for (let i = 0; i < c.frames; i++) {
      ts = i * c.interval_ms
      result = engine.process(data, { width: c.width, height: c.height, format: c.format, timestampMs: ts })
      faceCounts.push(result.faceCount)
      if (i === 0 && result.signals) {
        signals = result.signals
        realScore = result.realScore ?? null
      }
      if (result.done) break
    }
    check(t, c.expected, {
      status: result.status,
      fail_reason: result.failReason ?? null,
      at_ms: result.done ? ts : null,
      face_counts: faceCounts,
      signals,
      real_score: realScore,
      face_quality: engine.capture()?.quality ?? null,
    })
  })
}

// The bytes a case hands to verifyFaceRecord (one byte flipped if `face_tampered`).
function faceRecord(c: any): Buffer {
  const data = readFileSync(join(root, c.face_record))
  if (c.face_tampered) data[100] ^= 1
  return data
}

for (const c of cases.receipt_cases) {
  test(`receipt: ${c.name}`, (t) => {
    const key = Buffer.from(c.public_key_hex, 'hex')
    const verify = () =>
      c.face_record ? verifyFaceRecord(c.token, key, faceRecord(c), c.now) : verifyReceipt(c.token, key, c.now)
    if (c.expected_error) {
      t.throws(verify, { message: new RegExp(c.expected_error) })
      return
    }
    const r = verify()
    check(t, c.expected, {
      nonce: r.nonce,
      challenges: r.challenges,
      passed: r.passed,
      fail_reason: r.failReason ?? null,
      frames: r.frames,
      frames_sha256: r.framesSha256,
      real_score: r.realScore ?? null,
      face_sha256: r.faceSha256 ?? null,
      face_quality: r.faceQuality ?? null,
      issued_at: r.issuedAt,
      expires_at: r.expiresAt,
    })
  })
}
