# faceliveness-web

On-device face liveness checks in the browser: the Face Liveness Rust engine compiled to
WebAssembly. The user performs random actions (blink, turn left, turn right, open mouth) in
front of the camera; face detection, landmarks and passive anti-spoofing run locally, and the
session's best face is kept as an aligned face record for matching.

```bash
npm install faceliveness-web
```

For servers use [`faceliveness`](https://www.npmjs.com/package/faceliveness) (Node.js, native)
or [`faceliveness`](https://pypi.org/project/faceliveness/) (Python).

## Usage

Run the engine in a Web Worker: a frame takes tens of milliseconds.

```js
// worker.js
import init, { Engine } from 'faceliveness-web'

await init() // loads liveness_wasm_bg.wasm (~24 MB, ~8 MB compressed)
const engine = Engine.random(3) // e.g. ['turn_left', 'blink', 'open_mouth']

self.onmessage = ({ data }) => {
  // data.pixels: ctx.getImageData(0, 0, w, h).data from a canvas showing the camera
  const result = engine.process(data.pixels, { width: data.width, height: data.height, timestampMs: data.ts })
  self.postMessage({ result, capture: result.done ? engine.capture() : null })
}
```

`result.currentChallenge` is what to ask for next; `result.done` and `result.status`
(`'passed'` / `'failed'`) end the attempt. `engine.capture()` returns the face record:
a 112x112 aligned face as PNG bytes plus its quality.

The frame layout defaults to `'rgba'` (canvas `ImageData`); pass `format`, `rotation` and
`mirrored` for other sources. Full types ship in `liveness_wasm.d.ts`.

## Trust

A result computed in the browser can be changed by the user, so it is **not tamper-proof**. Use
it for instant feedback, and let your server make the decision: stream frames to a server
running `faceliveness` (`Engine.fromTicket`), which signs a receipt; `verifyReceipt` and
`verifyFaceRecord` are also available here to check one.

Working example (browser + server): the
[demo](https://github.com/wiseman-umanah/Face-Liveness/tree/master/demo).
Threat model: [docs/threat-model.md](https://github.com/wiseman-umanah/Face-Liveness/blob/master/docs/threat-model.md).

## License

Apache-2.0. Bundles MediaPipe face models and minivision MiniFASNet anti-spoofing models
(both Apache-2.0).
