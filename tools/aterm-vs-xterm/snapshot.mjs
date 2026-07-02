// Differential-conformance snapshot tool — xterm.js (Node) leg.
//
// Reads ALL raw bytes from stdin, feeds them to @xterm/headless configured as a
// 24x80 grid, waits for the write to flush, then emits exactly 24 lines — one
// per grid row, each row's visible text with trailing whitespace stripped.
//
// Output format is IDENTICAL to the aterm (Rust) leg so the two snapshots can
// be diffed byte-for-byte.
//
//   printf 'hi\x1b[2;5HX\x1b[31mY' | node snapshot.mjs
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { resolve, dirname } from 'node:path'
const require = createRequire(import.meta.url)
// The @xterm/headless baseline lives in the bench harness (root deps dropped it
// when aterm became the sole engine) — `gauntlet bootstrap` installs it there.
const { Terminal } = require(
  resolve(
    dirname(fileURLToPath(import.meta.url)),
    '../terminal-bench/node_modules/@xterm/headless/lib-headless/xterm-headless.js'
  )
)

const ROWS = 24,
  COLS = 80

// Read ALL stdin bytes as a single raw Buffer.
function readStdin() {
  return new Promise((resolve, reject) => {
    const chunks = []
    process.stdin.on('data', (c) => chunks.push(c))
    process.stdin.on('end', () => resolve(Buffer.concat(chunks)))
    process.stdin.on('error', reject)
  })
}

const buf = await readStdin()

const term = new Terminal({ rows: ROWS, cols: COLS, allowProposedApi: true })
term.write(buf)
// Flush: write('') resolves only after all prior writes have been parsed.
await new Promise((r) => term.write('', r))

const out = []
for (let row = 0; row < ROWS; row++) {
  const line = term.buffer.active.getLine(row)?.translateToString(true) ?? ''
  out.push(line)
}
process.stdout.write(`${out.join('\n')}\n`)
