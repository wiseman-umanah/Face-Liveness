# Face Liveness

**Face Liveness** is an in-development project exploring face-liveness functionality with a shared Rust core and language bindings for Python and Node.js.

The goal is to keep core functionality in Rust while making it accessible to applications written in different programming languages.

> **Project status:** Early development. APIs and functionality may change. Check the implementation before relying on this project in production.

## Architecture

The repository is organized as a Rust workspace.

```text
face-liveness/
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── liveness-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── liveness-py/
│   │   ├── Cargo.toml
│   │   ├── pyproject.toml
│   │   └── src/
│   ├── liveness-node/
│   └── liveness-wasm/
│       ├── Cargo.toml
│       ├── package.json
│       └── src/
└── README.md
```

### Components

- **`liveness-core`** — the Rust core: face detection and landmarks, action detectors, challenge flow.
- **`liveness-py`** — the [`faceliveness`](crates/liveness-py/README.md) Python package (PyO3 + maturin).
- **`liveness-node`** — the [`faceliveness`](crates/liveness-node/README.md) npm package (napi-rs).
- **`liveness-wasm`** — the WebAssembly build for browsers (wasm-bindgen), used by [`demo/`](demo/README.md).

The bindings are thin wrappers: all logic lives in `liveness-core`, and the shared cases in
`tests/conformance/cases.json` check that Rust, Python and Node give identical results.

## Prerequisites

Install the following tools:

- [Rust](https://www.rust-lang.org/tools/install)
- [Node.js](https://nodejs.org/)
- [pnpm](https://pnpm.io/installation)
- [uv](https://docs.astral.sh/uv/)

For Python extension development, use a Python version supported by the project's PyO3 and Maturin configuration.

## Getting Started

Clone the repository:

```bash
git clone https://github.com/wiseman-umanah/Face-Liveness.git
cd face-liveness
```

### Build the Rust workspace

```bash
cargo build --workspace
```

Run the tests:

```bash
cargo test --workspace
```

Check formatting:

```bash
cargo fmt --all -- --check
```

Run Clippy:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### Replay a recording

`crates/liveness-core/tests/fixtures/session_*.csv` are recorded sessions (per-frame face measurements, no images). Replay one through the detectors:

```bash
cargo run -p liveness-core --release --example replay -- crates/liveness-core/tests/fixtures/session_01.csv
```

This shows every detected blink, mouth-open and head turn, totals, and the cost per frame. Add `--sweep` for blink counts across thresholds, or run a liveness session against the recording:

```bash
cargo run -p liveness-core --release --example replay -- \
  crates/liveness-core/tests/fixtures/session_03.csv --challenges left,right,mouth --from 25
```

Record your own with `prototype/probe.py my_session.csv` (needs `opencv-python` and `mediapipe`).

### Run on a photo, video or webcam

The Rust core finds faces and landmarks itself, using MediaPipe's face models converted to ONNX
and run in pure Rust (see `crates/liveness-core/models/README.md`). The first release build takes
several minutes.

```bash
# A photo: faces found, eye/mouth/head measurements, time per frame
cargo run -p liveness-core --release --example analyze_image -- crates/liveness-core/tests/fixtures/astronaut_face.png

# A video file, via ffmpeg (set --size and --fps to the video's)
ffmpeg -loglevel error -i clip.mp4 -f rawvideo -pix_fmt rgb24 - \
  | cargo run -p liveness-core --release --example video -- --size 640x480 --fps 30 --csv clip.csv

# Your webcam, as a live liveness check (Linux)
ffmpeg -loglevel error -f v4l2 -video_size 640x480 -i /dev/video0 -vf fps=15 -f rawvideo -pix_fmt rgb24 - \
  | cargo run -p liveness-core --release --example video -- --size 640x480 --live --random 3
```

`--csv` writes the same format as `probe.py`, so a video can be replayed with the `replay` example
or compared with Google's MediaPipe using `tools/mediapipe_csv.py` and `tools/compare_csv.py`.

### Browser demo

The same engine compiled to WebAssembly, with a demo page that runs on-device or, with the
bundled Node server, as a server-verified check with signed receipts:

```bash
crates/liveness-wasm/build.sh && cp -r crates/liveness-wasm/pkg demo/pkg
node demo/server.mjs        # http://localhost:8080 (needs crates/liveness-node built)
```

See [demo/README.md](demo/README.md). `.github/workflows/pages.yml` publishes the on-device
version to GitHub Pages (enable Pages with "GitHub Actions" as the source).

### Python package

```bash
cd crates/liveness-py
uv sync          # builds the extension with maturin
uv run pytest
```

```python
import faceliveness

engine = faceliveness.Engine.random(3)
result = engine.process(frame, timestamp_ms, format="bgr")  # an OpenCV frame
print(result.status, result.current_challenge)
```

See [crates/liveness-py/README.md](crates/liveness-py/README.md) for the full API.

### Node.js package

```bash
cd crates/liveness-node
pnpm install
pnpm build       # native addon + TypeScript types
pnpm test
```

```ts
import { Engine } from 'faceliveness'

const engine = Engine.random(3)
const result = await engine.processAsync(frame, { width: 640, height: 480, timestampMs })
console.log(result.status, result.currentChallenge)
```

See [crates/liveness-node/README.md](crates/liveness-node/README.md) for the full API.

### Conformance tests

`tests/conformance/cases.json` holds recorded sessions and photo runs with their expected
outcomes. `cargo test -p liveness-core --test conformance`, `uv run pytest` and `pnpm test`
all run them; Rust is the reference, and a case with `"expected": null` makes the Rust test
print the actual result to paste in.

## Releasing

`.github/workflows/release.yml` builds wheels and npm packages for Linux (x64, arm64), macOS
(Intel, Apple Silicon) and Windows x64, tests them, and publishes when a `v*` tag is pushed.

One-time setup:

1. GitHub → Settings → Environments: create `release` (add yourself as required reviewer to
   approve each publish).
2. PyPI: add a [trusted publisher](https://docs.pypi.org/trusted-publishers/) for project
   `faceliveness`, this repository, workflow `release.yml`, environment `release`.
3. npm: create an automation token and save it as `NPM_TOKEN` in the `release` environment.

To release, set the same version in `Cargo.toml`, `crates/liveness-py/pyproject.toml` and
`crates/liveness-node/package.json`, commit, then tag `vX.Y.Z` and push the tag. Run the
workflow manually (Actions → Release → Run workflow) to build everything without publishing.

## Development

The project uses:

- Rust and Cargo for the shared core and native compilation.
- PyO3 and Maturin for Python interoperability.
- napi-rs for Node.js interoperability.
- uv for Python project and environment management.
- pnpm for Node.js dependencies.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines.

## Security

- **Challenge-response:** random actions with time limits. Printed photos can't pass: they
  can't blink, and tilting one doesn't register as a head turn (tested).
- **Passive anti-spoofing:** MiniFASNet scores each frame for print/screen texture. It's
  report-only until you calibrate a threshold with `tools/calibrate_spoof.py`.
- **Server-side trust:** signed tickets fix the challenges; signed receipts (Ed25519 JWTs) carry
  the outcome and a SHA-256 chain over every frame, verifiable by any JWT library.

Not defended: live deepfakes, injected virtual cameras and 3D masks. See the
[threat model](docs/threat-model.md) for every attack, its status and the evidence.

## Roadmap

Done:

- Adaptive blink, mouth-open and head-turn detectors, tuned on recorded sessions.
- Face detection and 478-point landmarks in pure Rust, matching MediaPipe.
- `faceliveness` packages for Python and Node.js, with cross-language conformance tests.
- Passive anti-spoofing and server-verifiable signed tickets and receipts.
- Browser (WebAssembly) build and a live demo, on-device or server-verified.

Next:

- Calibrate anti-spoofing on recorded genuine and attack sessions.
- iOS and Android bindings.
- Smaller packages and faster inference.

## Contributing

Contributions, bug reports, and suggestions are welcome.

Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

Licensed under the [Apache License 2.0](LICENSE).
