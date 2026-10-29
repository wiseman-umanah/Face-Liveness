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

Options: `totalTimeoutMs` (30000), `stepTimeoutMs` (10000), `faceLostMs` (1000), and `seed`
(a `bigint`, for tests only).

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
| `failReason` | `'timeout'`, `'step_timeout'`, `'face_lost'` or `'multiple_faces'` |
| `actions` | actions recognised in this frame |
| `faceCount`, `faces` | faces in view and their boxes |
| `signals` | `ear` (eye openness), `mouth` (mouth openness), `yaw` (head turn) |
| `landmarks` | 478 face-mesh points `[x, y]` of the main face |

### `new Session(challenges, options?)` and `session.update(timestampMs, { faceCount?, ear?, mouth?, yaw? })`

The same challenge logic without image processing, for when you already compute face
measurements yourself (for example with MediaPipe in the browser).

## Security notes

- Run the check where the user can't tamper with it (your server), or treat a client-side
  result as untrusted.
- Challenge-response liveness stops photos and replayed videos of the wrong actions. It does
  not by itself stop a live deepfake or virtual-camera injection.

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
