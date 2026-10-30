# faceliveness

On-device face liveness checks for Node.js. The user is asked to do a few random actions
(blink, turn left, turn right, open mouth) in front of the camera, and the check passes only
if they do them in order. Face detection and landmarks run inside the package (MediaPipe's
face models on a Rust core): no native dependencies to install and no cloud service.

```bash
npm install faceliveness
```

Node.js 22+, prebuilt for Linux (x64, arm64), macOS (Intel, Apple Silicon) and Windows x64.
Fully typed.

## Quick start

```ts
import { Engine } from 'faceliveness'

const engine = Engine.random(3) // e.g. ['turn_left', 'blink', 'open_mouth']

// For each camera frame (raw RGB bytes, e.g. from ffmpeg or a decoded WebRTC frame):
const result = await engine.processAsync(frame, { width: 640, height: 480, timestampMs })

if (result.done) {
  console.log('liveness:', result.status, result.failReason ?? '')
} else {
  console.log('please:', result.currentChallenge)
}
```

`processAsync` runs on Node's worker pool so your server stays responsive; await each call
before sending the next frame. `process` is the synchronous version.

## API

### `new Engine(challenges, options?)` / `Engine.random(count, options?)`

One liveness attempt. Challenges are `'blink' | 'turn_left' | 'turn_right' | 'open_mouth'`.
`Engine.random` picks them in an unpredictable order (from OS randomness), which is what you
want in production so attempts can't be pre-recorded.

Options: `totalTimeoutMs` (30000), `stepTimeoutMs` (10000), `faceLostMs` (1000),
`minRealScore` (fail a completed session whose mean `realScore` is below it; off by default,
calibrate first) and `seed` (a `bigint`, for tests only).

### `engine.process(frame, options)` / `engine.processAsync(frame, options)`

- `frame`: `Buffer` or `Uint8Array` of pixels.
- `options`: `{ width, height, timestampMs, format?, stride?, rotation?, mirrored? }`
  - `format`: `'rgb'` (default), `'bgr'`, `'rgba'`, `'bgra'` or `'nv21'`
  - `rotation`: clockwise degrees (0/90/180/270) that turn the frame upright
  - `mirrored`: `true` for selfie-preview frames, so "left" still means the user's left

### `FrameResult`

| Field | |
|---|---|
| `status` | `'in_progress'`, `'passed'` or `'failed'` |
| `done` | `true` once passed or failed (it won't change after that) |
| `currentChallenge`, `remaining` | what to ask the user for now, and how many are left |
| `failReason` | `'timeout'`, `'step_timeout'`, `'face_lost'`, `'multiple_faces'` or `'spoof_suspected'` |
| `actions` | actions recognised in this frame |
| `faceCount`, `faces` | faces in view and their boxes |
| `signals` | `ear` (eye openness), `mouth` (mouth openness), `yaw` (head turn) |
| `landmarks` | 478 face-mesh points `[x, y]` of the main face |
| `realScore` | passive anti-spoofing: 0-1 probability the face is real, not a photo or screen (measured on every 3rd frame with a face, otherwise empty) |

### `new Session(challenges, options?)` and `session.update(timestampMs, { faceCount?, ear?, mouth?, yaw? })`

The same challenge logic without image processing, for when you already compute face
measurements yourself (for example with MediaPipe in the browser).

## Server-side tickets and receipts

A result computed on the user's device can't be trusted. Run the engine on your server and let
other services check a signed receipt:

```ts
import { Engine, SigningKey, verifyReceipt } from 'faceliveness'

const key = SigningKey.generate() // once; store key.toBytes() as a secret
const ticket = key.issueTicket({ count: 3 }) // signed JWT: random challenges, 2 min expiry

const engine = Engine.fromTicket(ticket, key.publicKey)
// ... await engine.processAsync(frame, ...) for each frame the client streams, until done ...
const receipt = engine.receipt(key) // signed JWT with the outcome and a frame hash

// Any other service, with only the public key:
const r = verifyReceipt(receipt, key.publicKey)
if (r.passed && !alreadyUsed(r.nonce)) {
  // accept each nonce once
}
```

Tickets and receipts are standard Ed25519 (`EdDSA`) JWTs, so any JWT library can verify them.

## Security notes

- Challenge-response stops printed photos (they can't blink, and tilting one doesn't count as a
  head turn) and makes pre-recorded video unlikely to match. It does not stop a live deepfake
  or an injected virtual camera.
- `realScore` is uncalibrated for your cameras, so it is only reported by default. Record
  genuine and attack videos and run `tools/calibrate_spoof.py` before setting `minRealScore`.
- Full details: [threat model](https://github.com/wiseman-umanah/Face-Liveness/blob/master/docs/threat-model.md).

## Develop

```bash
pnpm install
pnpm build   # native addon (faceliveness.<platform>.node) + index.d.ts
pnpm test
pnpm bench
```

## License

Apache-2.0. Bundles MediaPipe face models (Apache-2.0); see
[models/README.md](https://github.com/wiseman-umanah/Face-Liveness/blob/master/crates/liveness-core/models/README.md).
