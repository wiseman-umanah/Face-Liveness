# liveness_node

Node.js bindings for the [Face Liveness](../../README.md) Rust core, built with [napi-rs](https://napi.rs).

> Early development: only `version()` is exposed so far.

## Develop

```bash
pnpm install
pnpm build   # builds the native addon (liveness_node.<platform>.node)
pnpm test    # ava
pnpm lint    # oxlint
```

## Usage

```javascript
const { version } = require('liveness_node')

console.log(version())
```

## License

Apache-2.0
