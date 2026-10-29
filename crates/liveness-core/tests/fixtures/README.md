# Recorded sessions

Per-frame face measurements recorded with `prototype/probe.py` (MediaPipe Face Mesh,
~17-20 fps). Only numbers, no images. Columns: `ts_ms, ear, mouth, yaw`; frames without
a face are not written, so a jump in `ts_ms` means the face was lost.

| File | Source | Notes |
|---|---|---|
| `session_01.csv` | `prototype/session_01.csv` | Blinks, mouth opens, head turns |
| `session_02.csv` | `prototype/session_02.csv` | Contains a 5.7 s face-lost gap |
| `session_03.csv` | `prototype/session.csv` | Believed to be the glasses recording; blink dips stay above 0.75 x baseline |

There are no hand labels yet (true blink/turn counts), so tests lock in current
behaviour and Python parity rather than accuracy. Record labelled sessions to measure accuracy.

# Images

| File | Source | Notes |
|---|---|---|
| `astronaut_face.png` | 256x256 crop (x 96-352, y 0-256) of scikit-image's `astronaut.png`: NASA photo of astronaut Eileen Collins, public domain | Face for the vision tests |
| `astronaut_face_mediapipe.csv` | MediaPipe FaceLandmarker 0.10.14 (`face_landmarker.task` float16/1) run on `astronaut_face.png` | Reference landmarks and signals the Rust pipeline is compared against |
