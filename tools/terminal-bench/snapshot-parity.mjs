// Proves the Rust emulator's getSnapshot() path is correct for the real app:
// the renderer replays `snapshotAnsi` into xterm, so the Rust-serialized ANSI
// must reproduce the same visible grid when fed through xterm.
//
//   corpus --> [Rust addon] --serializeAnsi--> ANSI --> [xterm replay] --> grid
//   must equal the Rust addon's own visible grid.
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const [addonPath, corpusPath] = process.argv.slice(2)
const { HeadlessTerminal } = require(addonPath)

const COLS = 120
const ROWS = 40
const SCROLLBACK = 5000
const CHUNK = 4096
const corpus = readFileSync(corpusPath)

// 1) Run corpus through the Rust engine, capture its visible grid + snapshot ANSI.
const term = new HeadlessTerminal(COLS, ROWS, SCROLLBACK)
for (let i = 0; i < corpus.length; i += CHUNK) {
  term.write(corpus.subarray(i, i + CHUNK))
}
const rustVisible = term.snapshot().join('\n')
const snapshotAnsi = term.serializeAnsi()

// 2) Replay that snapshot ANSI into xterm (what the renderer actually does).
const xt = new Terminal({ cols: COLS, rows: ROWS, scrollback: SCROLLBACK, allowProposedApi: true })
await new Promise((resolve) => xt.write(snapshotAnsi, resolve))
const buf = xt.buffer.active
const rows = []
for (let r = 0; r < ROWS; r++) {
  const line = buf.getLine(buf.baseY + r)
  rows.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
}
const xtermReplay = rows.join('\n')

const ok = xtermReplay === rustVisible
console.log(`snapshot size      : ${snapshotAnsi.length} bytes of ANSI`)
console.log(`rust visible rows  : ${term.snapshot().length}`)
console.log(`xterm replay match : ${ok ? '✅ identical visible grid' : '❌ MISMATCH'}`)
if (!ok) {
  const a = rustVisible.split('\n')
  const b = xtermReplay.split('\n')
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) {
      console.log(
        `  row ${i}:\n    rust : ${JSON.stringify(a[i])}\n    xterm: ${JSON.stringify(b[i])}`
      )
    }
  }
  process.exit(1)
}
