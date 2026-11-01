import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { inflateSync } from 'node:zlib'

import test from 'ava'

import { Engine, SigningKey, verifyFaceRecord, verifyReceipt } from '../index.js'

const NOW = 1_800_000_000
const face = readFileSync(join(import.meta.dirname, '../../../tests/conformance/astronaut_face.rgb'))
const size = { width: 256, height: 256 }

test('engine captures an aligned face record', (t) => {
  const engine = new Engine(['blink'])
  t.is(engine.capture(), null)
  engine.process(face, { ...size, timestampMs: 0 })
  const c = engine.capture()!
  t.is(c.size, 112)
  t.is(c.timestampMs, 0)
  t.true(c.quality > 0.5 && c.quality <= 1)
  t.deepEqual([...c.png.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
  t.is(c.sha256, createHash('sha256').update(c.png).digest('hex'))
  t.is(c.rgb.length, 112 * 112 * 3)
})

test('captureSize option sets the record size', (t) => {
  const engine = new Engine(['blink'], { captureSize: 160 })
  engine.process(face, { ...size, timestampMs: 0 })
  t.is(engine.capture()!.size, 160)
})

test('PNG holds exactly the raw pixels', (t) => {
  const engine = new Engine(['blink'])
  engine.process(face, { ...size, timestampMs: 0 })
  const { png, rgb, size: side } = engine.capture()!
  // Collect IDAT chunks, inflate, undo the per-row filters.
  const idat: Buffer[] = []
  for (let pos = 8; pos < png.length; ) {
    const length = png.readUInt32BE(pos)
    if (png.toString('latin1', pos + 4, pos + 8) === 'IDAT') idat.push(png.subarray(pos + 8, pos + 8 + length))
    pos += 12 + length
  }
  const raw = inflateSync(Buffer.concat(idat))
  const stride = side * 3
  const out = Buffer.alloc(side * stride)
  for (let y = 0; y < side; y++) {
    const filter = raw[y * (stride + 1)]
    for (let x = 0; x < stride; x++) {
      const v = raw[y * (stride + 1) + 1 + x]
      const a = x >= 3 ? out[y * stride + x - 3] : 0
      const b = y > 0 ? out[(y - 1) * stride + x] : 0
      const c = x >= 3 && y > 0 ? out[(y - 1) * stride + x - 3] : 0
      const p = a + b - c
      const paeth = Math.abs(p - a) <= Math.abs(p - b) && Math.abs(p - a) <= Math.abs(p - c) ? a : Math.abs(p - b) <= Math.abs(p - c) ? b : c
      out[y * stride + x] = (v + [0, a, b, (a + b) >> 1, paeth][filter]) & 255
    }
  }
  t.true(out.equals(rgb))
})

test('receipt vouches for the face record, but not after a failed session', (t) => {
  const key = SigningKey.generate()
  const ticket = key.issueTicket({ challenges: ['blink'], now: NOW })
  const engine = Engine.fromTicket(ticket, key.publicKey, { now: NOW })
  // A still photo can't blink: the step times out.
  for (let i = 0; i < 30 && !engine.process(face, { ...size, timestampMs: i * 500 }).done; i++);
  const receipt = engine.receipt(key, { now: NOW + 20 })
  const c = engine.capture()!
  const r = verifyReceipt(receipt, key.publicKey, NOW + 30)
  t.is(r.faceSha256, c.sha256)
  t.true(Math.abs(r.faceQuality! - c.quality) < 1e-6)
  t.throws(() => verifyFaceRecord(receipt, key.publicKey, c.png, NOW + 30), { message: /did not pass/ })
})
