import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import test from 'ava'

import { Engine, Session, version } from '../index.js'

const pkg = createRequire(import.meta.url)('../package.json')
const face = readFileSync(join(import.meta.dirname, '../../../tests/conformance/astronaut_face.rgb'))
const size = { width: 256, height: 256 }

test('version matches the package version', (t) => {
  t.is(version(), pkg.version)
})

test('process returns a full result for a face', (t) => {
  const engine = new Engine(['turn_left', 'blink'])
  t.deepEqual(engine.challenges, ['turn_left', 'blink'])
  const r = engine.process(face, { ...size, timestampMs: 0 })
  t.is(r.status, 'in_progress')
  t.is(r.currentChallenge, 'turn_left')
  t.is(r.remaining, 2)
  t.false(r.done)
  t.is(r.faceCount, 1)
  t.is(r.landmarks?.length, 478)
  t.true(r.faces[0].score > 0.5)
  t.true(r.signals!.ear > 0.2)
})

test('Uint8Array, BGR and RGBA frames agree', (t) => {
  const rgb = new Engine(['blink']).process(new Uint8Array(face), { ...size, timestampMs: 0 })
  const bgr = Buffer.from(face)
  for (let i = 0; i < bgr.length; i += 3) [bgr[i], bgr[i + 2]] = [bgr[i + 2], bgr[i]]
  const rgba = Buffer.alloc(256 * 256 * 4, 255)
  for (let i = 0, j = 0; i < face.length; i += 3, j += 4) face.copy(rgba, j, i, i + 3)
  for (const [data, format] of [
    [bgr, 'bgr'],
    [rgba, 'rgba'],
  ] as const) {
    const r = new Engine(['blink']).process(data, { ...size, format, timestampMs: 0 })
    t.true(Math.abs(r.signals!.ear - rgb.signals!.ear) < 1e-6, format)
  }
})

test('processAsync keeps the event loop free', async (t) => {
  const engine = new Engine(['blink'])
  let ticks = 0
  const timer = setInterval(() => ticks++, 1)
  for (let i = 0; i < 5; i++) {
    const r = await engine.processAsync(face, { ...size, timestampMs: i * 100 })
    t.is(r.faceCount, 1)
  }
  clearInterval(timer)
  // A blocking call would let the timer fire at most a few times.
  t.true(ticks > 20, `timer fired ${ticks} times`)
})

test('random uses OS randomness unless seeded', (t) => {
  const a = Engine.random(4, { seed: 7n }).challenges
  t.deepEqual(Engine.random(4, { seed: 7n }).challenges, a)
  const orders = new Set(Array.from({ length: 5 }, () => Session.random(8).challenges.join()))
  t.true(orders.size > 1)
})

test('invalid input is reported', (t) => {
  t.throws(() => new Engine(['smile' as any]), { message: /unknown challenge/ })
  t.throws(() => new Engine([]), { message: /empty/ })
  const engine = new Engine(['blink'])
  t.throws(() => engine.process(face, { ...size, timestampMs: 0, format: 'rgba' }), { message: /buffer too small/ })
  t.throws(() => engine.process(face, { ...size, timestampMs: 0, rotation: 45 }), { message: /rotation/ })
  t.throws(() => engine.process(face, { ...size, timestampMs: -1 }), { message: /negative/ })
})

test('session works from measurements alone', (t) => {
  const s = new Session(['open_mouth'])
  for (let ts = 0; ts < 300; ts += 50) s.update(ts, { ear: 0.3, mouth: 0.05, yaw: 0.5 })
  let r
  for (let ts = 300; ts < 800; ts += 50) {
    r = s.update(ts, { ear: 0.3, mouth: 0.6, yaw: 0.5 })
    if (r.done) break
  }
  t.is(r!.status, 'passed')
  t.is(s.update(900, { faceCount: 0 }).status, 'passed')
  t.throws(() => s.update(1000, { ear: 0.3 }), { message: /all of ear/ })
})
