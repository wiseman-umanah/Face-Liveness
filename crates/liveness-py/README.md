# faceliveness

On-device face liveness checks for Python. The user is asked to do a few random actions
(blink, turn left, turn right, open mouth) in front of the camera, and the check passes only
if they do them in order. Face detection and landmarks run inside the package (MediaPipe's
face models on a Rust core): no OpenCV, MediaPipe or cloud service needed.

```bash
pip install faceliveness
```

Python 3.9+, prebuilt for Linux (x86_64, aarch64), macOS (Intel, Apple Silicon) and Windows.

## Quick start: webcam with OpenCV

```python
import time

import cv2
import faceliveness

engine = faceliveness.Engine.random(3)  # e.g. ["turn_left", "blink", "open_mouth"]
camera = cv2.VideoCapture(0)
start = time.monotonic()

while True:
    ok, frame = camera.read()
    if not ok:
        break
    timestamp_ms = int((time.monotonic() - start) * 1000)
    result = engine.process(frame, timestamp_ms, format="bgr")  # OpenCV frames are BGR

    if result.done:
        print("liveness:", result.status, result.fail_reason or "")
        break
    print("please:", result.current_challenge, end="\r")
```

## API

### `Engine(challenges, *, total_timeout_ms=30000, step_timeout_ms=10000, face_lost_ms=1000, min_real_score=None)`

One liveness attempt. `challenges` is a list of `"blink"`, `"turn_left"`, `"turn_right"`,
`"open_mouth"`. `Engine.random(count)` picks them in an unpredictable order (from OS
randomness), which is what you want in production so attempts can't be pre-recorded. `min_real_score` fails a completed session whose mean `real_score` is below it;
it's off by default, so calibrate it first (see Security notes).

### `engine.process(frame, timestamp_ms, *, format=None, rotation=0, mirrored=False) -> FrameResult`

- `frame`: uint8 array `(height, width, 3)` or `(height, width, 4)`, e.g. from NumPy, OpenCV
  or `numpy.asarray(pil_image)`. NV21 (Android) frames as `(height * 3 // 2, width)`.
  Raw `bytes` work too with `width=`, `height=` and `format=`.
- `format`: `"rgb"` (default for 3 channels), `"bgr"` (OpenCV), `"rgba"` (default for 4),
  `"bgra"` or `"nv21"`.
- `rotation`: clockwise degrees (0/90/180/270) that turn the frame upright, for phone cameras.
- `mirrored`: `True` for selfie-preview frames, so "left" still means the user's left.

The Python GIL is released while a frame is processed, so other threads keep running.

### `FrameResult`

| Field | |
|---|---|
| `status` | `"in_progress"`, `"passed"` or `"failed"` |
| `done` | `True` once passed or failed (it won't change after that) |
| `current_challenge`, `remaining` | what to ask the user for now, and how many are left |
| `fail_reason` | `"timeout"`, `"step_timeout"`, `"face_lost"`, `"multiple_faces"` or `"spoof_suspected"` |
| `actions` | actions recognised in this frame |
| `face_count`, `faces` | faces in view and their boxes |
| `signals` | `ear` (eye openness), `mouth` (mouth openness), `yaw` (head turn) |
| `landmarks` | 478 face-mesh points `(x, y)` of the main face |
| `real_score` | passive anti-spoofing: 0-1 probability the face is real, not a photo or screen (measured on every 3rd frame with a face, otherwise empty) |

### `Session(challenges, ...)` and `session.update(timestamp_ms, *, face_count=1, ear=, mouth=, yaw=)`

The same challenge logic without image processing, for when you already compute face
measurements yourself (for example with MediaPipe in a browser).

## Server-side tickets and receipts

A result computed on the user's device can't be trusted. Run the engine on your server and let
other services check a signed receipt:

```python
key = faceliveness.SigningKey.generate()      # once; store key.to_bytes() as a secret
ticket = key.issue_ticket(count=3)            # signed JWT: random challenges, 2 min expiry

engine = faceliveness.Engine.from_ticket(ticket, key.public_key)
# ... engine.process(frame, ts) for each frame the client streams, until result.done ...
receipt = engine.receipt(key)                 # signed JWT with the outcome and a frame hash

# Any other service, with only the public key:
r = faceliveness.verify_receipt(receipt, key.public_key)
if r.passed and not already_used(r.nonce):    # accept each nonce once
    ...
```

Tickets and receipts are standard Ed25519 (`EdDSA`) JWTs, so any JWT library can verify them.

## Face record for matching

Each session keeps its best face: the frame that is most frontal, eyes open, mouth closed,
sharp, well lit and large enough, aligned to the standard 112x112 template most
face-recognition models expect (ArcFace, SFace, MobileFaceNet...). Pass `capture_size=` for
another size.

```python
c = engine.capture                  # FaceCapture or None
c.png                               # aligned face as PNG bytes: store/send these exact bytes
c.quality                           # 0-1; below ~0.3 matching will be unreliable
np.frombuffer(c.rgb, np.uint8).reshape(c.size, c.size, 3)   # as a NumPy array
```

The signed receipt records `face_sha256` (the PNG's hash) and `face_quality`, so liveness and
the face record are one signed fact. In your face-matching service, accept a face only like
this:

```python
r = faceliveness.verify_face_record(receipt, public_key, face_png)
# raises unless: genuine and unexpired receipt, session passed, and face_png is the exact
# image the receipt vouches for. Then also reject a nonce you've seen before.
```

This stops someone from passing liveness with their own face and then submitting a photo of
someone else for matching.

## Security notes

- Challenge-response stops printed photos (they can't blink, and tilting one doesn't count as a
  head turn) and makes pre-recorded video unlikely to match. It does not stop a live deepfake
  or an injected virtual camera.
- `real_score` is uncalibrated for your cameras, so it is only reported by default. Record
  genuine and attack videos and run `tools/calibrate_spoof.py` before setting `min_real_score`.
- Full details: [threat model](https://github.com/wiseman-umanah/Face-Liveness/blob/master/docs/threat-model.md).

## Develop

Requires Rust and [uv](https://docs.astral.sh/uv/). `uv sync` builds the extension.

```bash
uv sync
uv run pytest
```

## License

Apache-2.0. Bundles MediaPipe face models (Apache-2.0); see
[models/README.md](https://github.com/wiseman-umanah/Face-Liveness/blob/master/crates/liveness-core/models/README.md).
