# Python prototype

The scripts used to record sessions and tune the detectors before they were ported to Rust.
Useful if you want to record your own data or re-tune thresholds.

## Setup

Tested with Python 3.12, mediapipe 0.10.14 (it uses the legacy `mp.solutions` API) and
opencv-python:

```bash
python3.12 -m venv prototype/.venv
prototype/.venv/bin/pip install "mediapipe==0.10.14" opencv-python
```

## Record a session

```bash
prototype/.venv/bin/python prototype/probe.py my_session.csv [--camera 0]
```

Shows the camera with live eye (EAR), mouth and head-turn (yaw) values and writes one row per
frame with a face: `ts_ms,ear,mouth,yaw`. Press Esc to stop. It won't overwrite an existing file
unless you pass `--force`.

## Analyze it

```bash
python3 prototype/analyze.py my_session.csv    # value percentiles, blink counts at several thresholds
python3 prototype/analyze2.py my_session.csv   # adaptive blink detector across a parameter grid
```

`analyze2.py` is the algorithm the Rust `BlinkDetector` implements; the Rust replay test checks
it reproduces these counts exactly. Replay a recording through the Rust detectors with:

```bash
cargo run -p liveness-core --release --example replay -- my_session.csv
```

## Recordings stay local

Recordings are measurements of a real person's face, so `.gitignore` keeps `prototype/*.csv`
out of the repository. Sample recordings (shared with consent) are in
`crates/liveness-core/tests/fixtures/`. If you contribute recordings of other people, get their
consent first.
