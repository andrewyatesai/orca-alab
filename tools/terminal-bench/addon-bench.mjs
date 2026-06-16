// Loads the REAL napi addon (native/orca-node → orca_node.node) into a Node
// process and runs the same corpus through it — proving the Rust terminal engine
// executes in-process via `require('...node')`, matches xterm's output, and is
// faster. This is the "Rust in the shipping app" path (Electron exposes the same
// Node-API, so the identical .node loads there with no rebuild).
import { readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const addonPath = process.argv[2]
const corpusPath = process.argv[3]
const outPath = process.argv[4]

const { HeadlessTerminal, engine } = require(addonPath)

const COLS = 120
const ROWS = 40
const SCROLLBACK = 5000
const CHUNK = 4096

const corpus = readFileSync(corpusPath)
const term = new HeadlessTerminal(COLS, ROWS, SCROLLBACK)

const start = process.hrtime.bigint()
for (let i = 0; i < corpus.length; i += CHUNK) {
  term.write(corpus.subarray(i, i + CHUNK))
}
const end = process.hrtime.bigint()

const ms = Number(end - start) / 1e6
const mb = corpus.length / (1024 * 1024)
const mbPerS = mb / (ms / 1000)
const visible = term.snapshot().join('\n')

const FNV_OFFSET = 0xcbf29ce484222325n
const FNV_PRIME = 0x00000100000001b3n
const MASK = 0xffffffffffffffffn
function fnv1aHex(bytes) {
  let h = FNV_OFFSET
  for (const b of bytes) {
    h ^= BigInt(b)
    h = (h * FNV_PRIME) & MASK
  }
  return h.toString(16).padStart(16, '0')
}

console.error(
  `addon : ${mb.toFixed(2)} MB in ${ms.toFixed(1)} ms = ${mbPerS.toFixed(1)} MB/s  (engine=${engine()}, scrollback ${term.scrollbackLen()} lines)`
)

if (outPath) {
  writeFileSync(
    outPath,
    JSON.stringify({
      engine: `napi:${engine()}`,
      bytes: corpus.length,
      ms: Number(ms.toFixed(3)),
      mb_per_s: Number(mbPerS.toFixed(2)),
      visible_sha: fnv1aHex(Buffer.from(visible, 'utf8')),
      scrollback: term.scrollbackLen()
    })
  )
}
