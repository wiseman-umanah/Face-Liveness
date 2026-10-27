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

### Node.js bindings

Navigate to the Node.js crate:

```bash
cd crates/liveness-node
```

Install JavaScript dependencies:

```bash
pnpm install
```

Build the native addon using the build script defined in `package.json`.

If the package defines a `build` script, run:

```bash
pnpm build
```

The package's generated JavaScript entry point and native addon must be available before you can import the module.

For the current version function, the API is intended to be callable as follows:

```javascript
const { liveness_node } = require("./");

console.log(liveness_node());
```

Run this example from the Node.js package directory after building the addon.

### Python bindings

The Python bindings live in `crates/liveness-py`.

Python installation and build instructions will be documented here as the Python API and packaging configuration are finalized.

For development, use the project's `pyproject.toml` and `uv.lock` to manage its Python environment and dependencies.

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

The project license will be documented here once the repository-wide licensing decision is finalized.
