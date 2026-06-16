// Node counterpart to rust/crates/orca-terminal/examples/bench.rs.
// Feeds the SAME corpus file through @xterm/headless — the engine Orca ships —
// in the same 4096-byte chunks, times a full parse, and fingerprints the final
// visible grid with the same FNV-1a hash so the two engines can be compared for
// both throughput and output parity.
import { readFileSync, writeFileSync } from 'node:fs'
import pkg from '@xterm/headless'
const { Terminal } = pkg

const ROWS = 40
const COLS = 120
const SCROLLBACK = 5000
const CHUNK = 4096

const corpusPath = process.argv[2]
const outPath = process.argv[3]

const corpus = readFileSync(corpusPath) // Buffer of raw bytes
const term = new Terminal({
  cols: COLS,
  rows: ROWS,
  scrollback: SCROLLBACK,
  allowProposedApi: true
})

// xterm's write is async (it batches across macrotasks); writing all chunks then
// awaiting one final flush callback measures the time to fully parse the stream.
const start = process.hrtime.bigint()
for (let i = 0; i < corpus.length; i += CHUNK) {
  term.write(corpus.subarray(i, i + CHUNK))
}
await new Promise((resolve) => term.write('', resolve))
const end = process.hrtime.bigint()

const ms = Number(end - start) / 1e6
const mb = corpus.length / (1024 * 1024)
const mbPerS = mb / (ms / 1000)

// Final visible grid: the active buffer's viewport rows, trailing blanks trimmed
// (translateToString(true) == trimRight), joined with \n — identical to the
// Rust snapshot()/row_text() contract.
const buf = term.buffer.active
const rows = []
for (let r = 0; r < ROWS; r++) {
  const line = buf.getLine(buf.baseY + r)
  // trim trailing whitespace to match Rust row_text()'s trim_end(): xterm keeps
  // background-colored trailing spaces as real cells, Rust drops them — same
  // glyphs, different convention. Normalize both to compare rendered text.
  rows.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
}
const visible = rows.join('\n')

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
const visibleSha = fnv1aHex(Buffer.from(visible, 'utf8'))

console.error(
  `xterm : ${mb.toFixed(2)} MB in ${ms.toFixed(1)} ms = ${mbPerS.toFixed(1)} MB/s  (scrollback ${buf.length - ROWS} lines)`
)

if (outPath) {
  writeFileSync(
    outPath,
    JSON.stringify({
      engine: '@xterm/headless',
      bytes: corpus.length,
      ms: Number(ms.toFixed(3)),
      mb_per_s: Number(mbPerS.toFixed(2)),
      visible_sha: visibleSha,
      scrollback: buf.length - ROWS
    })
  )
  writeFileSync(`${outPath}.grid.txt`, visible)
}
