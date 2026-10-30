import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import test from 'ava'

import { Engine, SigningKey, verifyReceipt, verifyTicket } from '../index.js'

const NOW = 1_800_000_000
const face = readFileSync(join(import.meta.dirname, '../../../tests/conformance/astronaut_face.rgb'))
const size = { width: 256, height: 256 }

test('key round trip hides nothing it should not', (t) => {
  const key = SigningKey.generate()
  t.is(key.toBytes().length, 32)
  t.is(key.publicKey.length, 32)
  t.deepEqual(SigningKey.fromBytes(key.toBytes()).publicKey, key.publicKey)
  t.throws(() => SigningKey.fromBytes(new Uint8Array(5)), { message: /32 bytes/ })
})

test('tickets verify, expire and reject other keys', (t) => {
  const key = SigningKey.generate()
  const token = key.issueTicket({ challenges: ['blink', 'turn_left'], ttlSecs: 60, now: NOW })
  const ticket = verifyTicket(token, key.publicKey, NOW + 30)
  t.deepEqual(ticket.challenges, ['blink', 'turn_left'])
  t.is(ticket.expiresAt, NOW + 60)
  t.is(verifyTicket(key.issueTicket({ count: 4, now: NOW }), key.publicKey, NOW).challenges.length, 4)
  t.throws(() => verifyTicket(token, key.publicKey, NOW + 61), { message: /expired/ })
  t.throws(() => verifyTicket(token, SigningKey.generate().publicKey, NOW), { message: /signature/ })
})

test('ticketed session ends with a verifiable receipt', (t) => {
  const key = SigningKey.generate()
  const token = key.issueTicket({ challenges: ['open_mouth'], ttlSecs: 120, now: NOW })
  const engine = Engine.fromTicket(token, key.publicKey, { now: NOW + 1 })
  t.deepEqual(engine.challenges, ['open_mouth'])
  t.throws(() => engine.receipt(key, { now: NOW + 1 }), { message: /not finished/ })

  let frames = 0
  let result
  const scored: boolean[] = []
  do {
    // A still photo can't open its mouth: the step times out after 10 s.
    result = engine.process(face, { ...size, timestampMs: frames * 500 })
    frames++
    scored.push(result.realScore != null)
  } while (!result.done)
  // Anti-spoofing runs on every 3rd frame with a face.
  t.deepEqual(scored.slice(0, 4), [true, false, false, true])
  t.is(result.failReason, 'step_timeout')

  const r = verifyReceipt(engine.receipt(key, { now: NOW + 20 }), key.publicKey, NOW + 30)
  t.false(r.passed)
  t.is(r.failReason, 'step_timeout')
  t.is(r.frames, frames)
  t.is(r.nonce, verifyTicket(token, key.publicKey, NOW).nonce)
  t.regex(r.framesSha256, /^[0-9a-f]{64}$/)
})

test('fromTicket rejects tickets signed by someone else', (t) => {
  const forged = SigningKey.generate().issueTicket({ challenges: ['blink'], now: NOW })
  t.throws(() => Engine.fromTicket(forged, SigningKey.generate().publicKey, { now: NOW }), { message: /signature/ })
})
