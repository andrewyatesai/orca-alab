// Verifies a live Session capture: the exact bytes the emulator saw (rawB64) are
// fed through xterm.js (ground truth), and the Session's own snapshotAnsi is
// replayed through xterm. If the two visible grids match, the engine that ran in
// the real Session produced a snapshot that renders identically to xterm parsing
// the same live PTY stream. Exit non-zero on mismatch.
import { readFileSync } from 'node:fs'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const cap = JSON.parse(readFileSync(process.argv[2], 'utf8'))
const COLS = cap.cols
const ROWS = cap.rows

function gridOf(term) {
  const buf = term.buffer.active
  const rows = []
  for (let r = 0; r < ROWS; r++) {
    const line = buf.getLine(buf.baseY + r)
    rows.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return rows.join('\n')
}

async function feed(bytes) {
  const t = new Terminal({ cols: COLS, rows: ROWS, scrollback: 5000, allowProposedApi: true })
  await new Promise((res) => t.write(bytes, res))
  return gridOf(t)
}

// Ground truth: xterm parsing exactly what the emulator consumed live.
const truth = await feed(Buffer.from(cap.rawB64, 'base64'))
// The Session's snapshot, replayed through xterm (what the renderer would show).
const snap = await feed(cap.snapshotAnsi)

const ok = truth === snap
const label = `${cap.cmd} ${(cap.args || []).join(' ')}`.trim()
console.log(`[${cap.engine}] ${label}`)
console.log(
  `  bytes=${cap.rawBytes} scrollback=${cap.scrollbackLines} fg=${cap.foreground} alt=${cap.modes?.alternateScreen}`
)
console.log(`  live-snapshot renders identical to xterm ground truth: ${ok ? '✅' : '❌ MISMATCH'}`)
if (!ok) {
  const a = truth.split('\n')
  const b = snap.split('\n')
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
