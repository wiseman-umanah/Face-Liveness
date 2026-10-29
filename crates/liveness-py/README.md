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

### `Engine(challenges, *, total_timeout_ms=30000, step_timeout_ms=10000, face_lost_ms=1000)`

One liveness attempt. `challenges` is a list of `"blink"`, `"turn_left"`, `"turn_right"`,
`"open_mouth"`. `Engine.random(count)` picks them in an unpredictable order (from OS
randomness), which is what you want in production so attempts can't be pre-recorded.

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
| `fail_reason` | `"timeout"`, `"step_timeout"`, `"face_lost"` or `"multiple_faces"` |
| `actions` | actions recognised in this frame |
| `face_count`, `faces` | faces in view and their boxes |
| `signals` | `ear` (eye openness), `mouth` (mouth openness), `yaw` (head turn) |
| `landmarks` | 478 face-mesh points `(x, y)` of the main face |

### `Session(challenges, ...)` and `session.update(timestamp_ms, *, face_count=1, ear=, mouth=, yaw=)`

The same challenge logic without image processing, for when you already compute face
measurements yourself (for example with MediaPipe in a browser).

## Security notes

- Run the check where the user can't tamper with it (your server), or treat a client-side
  result as untrusted.
- Challenge-response liveness stops photos and replayed videos of the wrong actions. It does
  not by itself stop a live deepfake or virtual-camera injection.

## Develop

Requires Rust and [uv](https://docs.astral.sh/uv/). `uv sync` builds the extension.

```bash
uv sync
uv run pytest
```

## License

Apache-2.0. Bundles MediaPipe face models (Apache-2.0); see
[models/README.md](https://github.com/wiseman-umanah/Face-Liveness/blob/master/crates/liveness-core/models/README.md).
