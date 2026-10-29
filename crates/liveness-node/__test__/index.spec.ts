import { createRequire } from 'node:module'

import test from 'ava'

import { version } from '../index.js'

const pkg = createRequire(import.meta.url)('../package.json')

test('version matches the package version', (t) => {
  t.is(version(), pkg.version)
})
