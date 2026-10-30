# Browser demo

A liveness check in the browser: the Rust engine compiled to WebAssembly runs in a Web Worker,
draws the face mesh live and prompts for random actions.

## Run it

```bash
crates/liveness-wasm/build.sh              # WebAssembly engine -> crates/liveness-wasm/pkg
cp -r crates/liveness-wasm/pkg demo/pkg

# On-device mode only: any static file server
python3 -m http.server 8000 --directory demo      # open http://localhost:8000

# Server-verified mode: needs the Node package built (cd crates/liveness-node && pnpm build)
node demo/server.mjs                               # open http://localhost:8080
```

Browsers only allow the camera on `localhost` or HTTPS, so to try it from a phone, put the
server behind HTTPS (for example a tunnel or a reverse proxy).

## Two modes

**On-device** (GitHub Pages, any static host): everything runs in the browser and no video
leaves the device. The result is labelled "not tamper-proof": a user can change what the page
reports, so don't trust it for anything that matters.

**Server-verified** (`server.mjs`): the server issues a signed ticket (it picks the
challenges), the browser streams frames to it, and the server runs the authoritative check
with its own clock and signs a receipt. The page then asks `/api/verify` to check the receipt,
as another of your services would; verifying the same receipt twice fails. The local engine
keeps drawing the face mesh, but the prompts and the verdict come from the server.

`server.mjs` has no dependencies besides the `faceliveness` package; read it as the reference
for wiring this into your own backend.

## Speed

WebAssembly with SIMD, single-threaded. On a 2013 laptop CPU (i5-4300U) a frame takes about
90 ms (~10 fps); newer machines are much faster. At 10 fps the detectors still catch about 90%
of blinks and nearly all turns and mouth-opens in the recorded sessions, and every step allows
10 s to retry. The engine is ~24 MB (7.9 MB brotli, 10 MB gzip), mostly the four models.
