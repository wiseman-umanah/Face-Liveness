// Runs the liveness engine (WebAssembly) off the main thread so the page stays responsive.
import init, { Engine } from './pkg/liveness_wasm.js'

let engine = null

self.onmessage = async ({ data: msg }) => {
  try {
    if (msg.type === 'init') {
      const start = performance.now()
      await init()
      self.postMessage({ type: 'ready', ms: performance.now() - start })
    } else if (msg.type === 'start') {
      engine?.free()
      // The engine loads its models here, which takes a moment.
      engine = msg.challenges ? new Engine(msg.challenges, msg.options) : Engine.random(msg.count, msg.options)
      self.postMessage({ type: 'started', challenges: engine.challenges })
    } else if (msg.type === 'frame') {
      const start = performance.now()
      const result = engine.process(new Uint8Array(msg.pixels), {
        width: msg.width,
        height: msg.height,
        timestampMs: msg.timestampMs,
      })
      self.postMessage({ type: 'result', result, ms: performance.now() - start })
    }
  } catch (error) {
    self.postMessage({ type: 'error', message: String(error?.message ?? error) })
  }
}
