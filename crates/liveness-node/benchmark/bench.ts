import { Bench } from 'tinybench'

import { version } from '../index.js'

const bench = new Bench()

bench.add('native version()', () => {
  version()
})

await bench.run()

console.table(bench.table())
