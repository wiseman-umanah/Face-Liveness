# liveness-py

Python bindings for the [Face Liveness](../../README.md) Rust core, built with [PyO3](https://pyo3.rs) and [maturin](https://www.maturin.rs).

> Early development: only `version()` is exposed so far.

## Develop

Requires Rust and [uv](https://docs.astral.sh/uv/). `uv sync` builds the extension in place.

```bash
uv sync
uv run pytest
```

## Usage

```python
import liveness_py

print(liveness_py.version())
```

Supports Python 3.9+ (built against the stable ABI, `abi3`).

## License

Apache-2.0
