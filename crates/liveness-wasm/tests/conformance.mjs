// Runs tests/conformance/cases.json against the WebAssembly build (the same .wasm the
// browser loads) and checks it matches the Rust reference. Usage: ./build.sh && node tests/conformance.mjs
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { join } from 'node:path'

const require = createRequire(import.meta.url)
const wasm = require('../pkg-node/liveness_wasm.js')

const root = join(import.meta.dirname, '../../..')
const cases = JSON.parse(readFileSync(join(root, 'tests/conformance/cases.json'), 'utf8'))
const APPROX = ['signals', 'real_score']

function close(expected, actual, tol) {
  if (expected && typeof expected === 'object') {
    return Object.keys(expected).length === Object.keys(actual ?? {}).length &&
      Object.keys(expected).every((k) => close(expected[k], actual[k], tol))
  }
  return Math.abs(expected - actual) <= tol
}

function check(name, expected, actual) {
  for (const key of APPROX) {
    if (expected[key] != null) assert.ok(close(expected[key], actual[key], cases.tolerance), `${name}: ${key} ${JSON.stringify(actual[key])}`)
    else assert.equal(actual[key] ?? null, expected[key] ?? null, `${name}: ${key}`)
  }
  const strip = (o) => Object.fromEntries(Object.entries(o).filter(([k]) => !APPROX.includes(k)))
  assert.deepEqual(strip(actual), strip(expected), name)
}

const options = (c) => ({
  totalTimeoutMs: c.config.total_timeout_ms,
  stepTimeoutMs: c.config.step_timeout_ms,
  faceLostMs: c.config.face_lost_ms,
})

let passed = 0
for (const c of cases.session_cases) {
  const session = new wasm.Session(c.challenges, options(c))
  const rows = readFileSync(join(root, c.recording), 'utf8').trim().split('\n').slice(1)
  const actions = []
  let r, ts
  for (const row of rows) {
    const [t, ear, mouth, yaw] = row.split(',').map(Number)
    ts = t
    if (ts < c.from_ms) continue
    r = session.update(ts, { faceCount: 1, ear, mouth, yaw })
    for (const a of r.actions) actions.push([ts, a])
    if (r.done) break
  }
  check(c.name, c.expected, { status: r.status, fail_reason: r.failReason ?? null, at_ms: r.done ? ts : null, actions })
  passed++
}

for (const c of cases.engine_cases) {
  const data = readFileSync(join(root, c.image))
  if (c.format === 'bgr') for (let i = 0; i < data.length; i += 3) [data[i], data[i + 2]] = [data[i + 2], data[i]]
  const engine = new wasm.Engine(c.challenges, options(c))
  const faceCounts = []
  let r, ts, signals = null, realScore = null
  for (let i = 0; i < c.frames; i++) {
    ts = i * c.interval_ms
    r = engine.process(data, { width: c.width, height: c.height, format: c.format, timestampMs: ts })
    faceCounts.push(r.faceCount)
    if (i === 0 && r.signals) [signals, realScore] = [r.signals, r.realScore ?? null]
    if (r.done) break
  }
  check(c.name, c.expected, {
    status: r.status, fail_reason: r.failReason ?? null, at_ms: r.done ? ts : null,
    face_counts: faceCounts, signals, real_score: realScore,
  })
  passed++
}

for (const c of cases.receipt_cases) {
  const key = Buffer.from(c.public_key_hex, 'hex')
  if (c.expected_error) {
    assert.throws(() => wasm.verifyReceipt(c.token, key, c.now), new RegExp(c.expected_error), c.name)
  } else {
    const r = wasm.verifyReceipt(c.token, key, c.now)
    check(c.name, c.expected, {
      nonce: r.nonce, challenges: r.challenges, passed: r.passed, fail_reason: r.failReason ?? null,
      frames: r.frames, frames_sha256: r.framesSha256, real_score: r.realScore ?? null,
      issued_at: r.issuedAt, expires_at: r.expiresAt,
    })
  }
  passed++
}
console.log(`conformance: ${passed} cases passed`)

// Speed, as the browser would see it (single thread, WebAssembly SIMD).
const face = readFileSync(join(root, 'tests/conformance/astronaut_face.rgb'))
const engine = new wasm.Engine(['blink'], { totalTimeoutMs: 1e12, stepTimeoutMs: 1e12 })
const times = []
for (let i = 0; i < 30; i++) {
  const t = performance.now()
  engine.process(face, { width: 256, height: 256, format: 'rgb', timestampMs: i * 33 })
  times.push(performance.now() - t)
}
times.sort((a, b) => a - b)
const mean = times.reduce((a, b) => a + b) / times.length
console.log(`wasm per frame: mean ${mean.toFixed(1)} ms, median ${times[15].toFixed(1)} ms (anti-spoofing every 3rd frame)`)
