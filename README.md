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
│   └── liveness-node/
│       ├── Cargo.toml
│       ├── package.json
│       └── src/
└── README.md
```

### Components

- **`liveness-core`** — shared functionality implemented in Rust.
- **`liveness-py`** — Python bindings built with PyO3 and Maturin.
- **`liveness-node`** — Node.js bindings built with napi-rs.

The language bindings are intended to expose shared Rust functionality without duplicating its implementation.

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

### Node.js bindings

Navigate to the Node.js crate:

```bash
cd crates/liveness-node
```

Install dependencies, build the native addon and run the tests:

```bash
pnpm install
pnpm build
pnpm test
```

Then, from the same directory:

```javascript
const { version } = require("./");

console.log(version());
```

### Python bindings

The Python bindings live in `crates/liveness-py` and support Python 3.9+.

```bash
cd crates/liveness-py
uv sync          # builds the extension with maturin
uv run pytest
```

```python
import liveness_py

print(liveness_py.version())
```

## Development

The project uses:

- Rust and Cargo for the shared core and native compilation.
- PyO3 and Maturin for Python interoperability.
- napi-rs for Node.js interoperability.
- uv for Python project and environment management.
- pnpm for Node.js dependencies.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development guidelines.

## Roadmap

Planned work may include:

- Expanding the shared Rust API.
- Exposing supported functionality through Python and Node.js.
- Adding automated tests for the core and language bindings.
- Improving cross-platform builds and packaging.
- Developing and evaluating face-liveness functionality.

The roadmap may change as the project evolves.

## Contributing

Contributions, bug reports, and suggestions are welcome.

Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

Licensed under the [Apache License 2.0](LICENSE).
