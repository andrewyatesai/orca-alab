// Verifies a resize capture. The stream embeds a marker (ESC P X ESC \\, a DCS
// no-op xterm ignores) splitting pre-resize from post-resize bytes. We feed
// xterm the pre-resize bytes, resize it exactly like the Session did, feed the
// post-resize bytes, and compare its visible grid to the Session snapshot
// replayed through xterm at the FINAL size. Exit non-zero on mismatch.
import { readFileSync } from 'node:fs'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const cap = JSON.parse(readFileSync(process.argv[2], 'utf8'))
const FINAL_COLS = cap.cols
const FINAL_ROWS = cap.rows
const MARKER = Buffer.from('\x1bPX\x1b\\', 'latin1') // DCS X ST — xterm ignores

function gridOf(term, rows) {
  const buf = term.buffer.active
  const out = []
  for (let r = 0; r < rows; r++) {
    const line = buf.getLine(buf.baseY + r)
    out.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return out.join('\n')
}

function splitOnMarker(buf) {
  const i = buf.indexOf(MARKER)
  if (i < 0) {
    return [buf, Buffer.alloc(0)]
  }
  return [buf.subarray(0, i), buf.subarray(i + MARKER.length)]
}

async function write(term, bytes) {
  await new Promise((res) => term.write(bytes, res))
}

// Ground truth: start at original size, feed pre-resize, resize, feed post-resize.
const raw = Buffer.from(cap.rawB64, 'base64')
const [pre, post] = splitOnMarker(raw)
const truth = new Terminal({
  cols: cap.startCols,
  rows: cap.startRows,
  scrollback: 5000,
  allowProposedApi: true
})
await write(truth, pre)
truth.resize(FINAL_COLS, FINAL_ROWS)
await write(truth, post)
const truthGrid = gridOf(truth, FINAL_ROWS)

// Session snapshot replayed at final size.
const snapTerm = new Terminal({
  cols: FINAL_COLS,
  rows: FINAL_ROWS,
  scrollback: 5000,
  allowProposedApi: true
})
await write(snapTerm, cap.snapshotAnsi)
const snapGrid = gridOf(snapTerm, FINAL_ROWS)

const ok = truthGrid === snapGrid
console.log(
  `[${cap.engine}] ${cap.cmd} ${(cap.args || []).join(' ')} resize ${cap.startCols}x${cap.startRows}->${FINAL_COLS}x${FINAL_ROWS}`
)
console.log(`  bytes=${cap.rawBytes} identical: ${ok ? 'ok' : 'MISMATCH'}`)
if (!ok) {
  const a = truthGrid.split('\n')
  const b = snapGrid.split('\n')
  let shown = 0
  for (let i = 0; i < Math.max(a.length, b.length) && shown < 8; i++) {
    if (a[i] !== b[i]) {
      console.log(
        `  row ${i}:\n    truth: ${JSON.stringify(a[i])}\n    snap : ${JSON.stringify(b[i])}`
      )
      shown++
    }
  }
  process.exit(1)
}
