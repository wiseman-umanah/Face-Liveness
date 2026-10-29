import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { Bench } from 'tinybench'

import { Engine } from '../index.js'

const face = readFileSync(join(import.meta.dirname, '../../../tests/conformance/astronaut_face.rgb'))
const engine = new Engine(['blink'], { totalTimeoutMs: 1e12, stepTimeoutMs: 1e12 })
let ts = 0

const bench = new Bench({ iterations: 20 })
bench.add('Engine.process (256x256 face)', () => {
  engine.process(face, { width: 256, height: 256, timestampMs: (ts += 33) })
})
await bench.run()
console.table(bench.table())
