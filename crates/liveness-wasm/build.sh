#!/usr/bin/env bash
# Builds the WebAssembly module and its JS bindings:
#   pkg/       for browsers (ES module, `--target web`), used by demo/
#   pkg-node/  the same .wasm for Node, used by the tests
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version 0.2.129
set -euo pipefail
cd "$(dirname "$0")"
root=../..

cargo build --release --target wasm32-unknown-unknown -p liveness-wasm
wasm="$root/target/wasm32-unknown-unknown/release/liveness_wasm.wasm"
wasm-bindgen --target web --out-dir pkg "$wasm"
wasm-bindgen --target nodejs --out-dir pkg-node "$wasm"
ls -la pkg/*.wasm
