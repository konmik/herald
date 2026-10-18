import fs from 'node:fs'
import { syncBuiltinESMExports } from 'node:module'
import { basename } from 'node:path'

process.send?.('started')
if (process.env.HERALD_TEST_PAUSE_STATE_WRITE === '1') {
  const rename = fs.renameSync
  fs.renameSync = (...args) => {
    const result = rename(...args)
    if (basename(String(args[1])) === 's1.json') {
      process.send?.('written')
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0)
    }
    return result
  }
  syncBuiltinESMExports()
}
