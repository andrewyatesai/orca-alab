// Proves the Rust engine renders a real full-screen TUI identically to xterm.js:
// alternate screen, absolute cursor positioning (CUP), erase (ED/EL), animated
// redraws, and the snapshot mode flags (alt-screen / bracketed-paste / app-cursor
// / mouse). This is the gap that "fully work" requires — a TUI like Claude Code
// or vim emits these constantly, and the old parser ignored them entirely.
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const addonPath = process.argv[2]
const { HeadlessTerminal } = require(addonPath)

const COLS = 100
const ROWS = 30
const CHUNK = 4096

// ── Build a deterministic full-screen TUI byte stream ───────────────────────
function buildTuiCorpus() {
  const out = []
  const push = (s) => out.push(Buffer.from(s, 'utf8'))
  const at = (r, c) => `\x1b[${r};${c}H`

  // A TUI starts up: alternate screen + bracketed paste + application cursor.
  push('\x1b[?1049h\x1b[?2004h\x1b[?1h')
  push('\x1b[?1000h\x1b[?1006h') // mouse: vt200 + SGR encoding
  push('\x1b[2J') // clear alt screen

  // Animate many frames: redraw a header, a moving spinner, a body region that
  // is erased and rewritten, and a status bar — all via absolute positioning.
  const spinner = ['|', '/', '-', '\\']
  for (let frame = 0; frame < 400; frame++) {
    // Header (inverse), full-width.
    push(`${at(1, 1)}\x1b[7m\x1b[K  orca tui  ·  frame ${String(frame).padStart(4)}\x1b[0m`)
    // Spinner cell.
    push(`${at(1, COLS - 2)}\x1b[36m${spinner[frame % 4]}\x1b[0m`)
    // Body: clear from row 3 down (ED 0), then draw a few colored rows.
    push(`${at(3, 1)}\x1b[J`)
    for (let i = 0; i < 6; i++) {
      const row = 3 + i
      const color = 31 + ((frame + i) % 7)
      push(`${at(row, 3)}\x1b[${color}mitem ${i}: ${'x'.repeat((frame + i * 7) % 40)}\x1b[0m`)
    }
    // A box border drawn with positioning.
    push(`${at(10, 2)}\x1b[34m+${'-'.repeat(COLS - 6)}+\x1b[0m`)
    push(`${at(11, 2)}\x1b[34m|\x1b[0m${at(11, COLS - 3)}\x1b[34m|\x1b[0m`)
    push(`${at(12, 2)}\x1b[34m+${'-'.repeat(COLS - 6)}+\x1b[0m`)
    // Status bar at the bottom (EL then text).
    push(`${at(ROWS, 1)}\x1b[2m\x1b[K -- INSERT --  ${frame % 2 ? 'tick' : 'tock'}\x1b[0m`)
    // Park the cursor like a TUI editor would.
    push(at(11, 4 + (frame % 20)))
  }
  return Buffer.concat(out)
}

const corpus = buildTuiCorpus()

// ── Rust engine (napi addon) ────────────────────────────────────────────────
const rust = new HeadlessTerminal(COLS, ROWS, 5000)
let t0 = process.hrtime.bigint()
for (let i = 0; i < corpus.length; i += CHUNK) {
  rust.write(corpus.subarray(i, i + CHUNK))
}
const rustMs = Number(process.hrtime.bigint() - t0) / 1e6
const rustGrid = rust.snapshot().join('\n')
const rustModes = {
  altScreen: rust.isAlternateScreen(),
  bracketedPaste: rust.bracketedPaste(),
  applicationCursor: rust.applicationCursor(),
  mouse: rust.mouseTracking(),
  sgrMouse: rust.sgrMouse()
}

// ── xterm.js engine ─────────────────────────────────────────────────────────
const xt = new Terminal({ cols: COLS, rows: ROWS, scrollback: 5000, allowProposedApi: true })
t0 = process.hrtime.bigint()
for (let i = 0; i < corpus.length; i += CHUNK) {
  xt.write(corpus.subarray(i, i + CHUNK))
}
await new Promise((r) => xt.write('', r))
const xtMs = Number(process.hrtime.bigint() - t0) / 1e6
const buf = xt.buffer.active
const xtRows = []
for (let r = 0; r < ROWS; r++) {
  const line = buf.getLine(buf.baseY + r)
  xtRows.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
}
const xtGrid = xtRows.join('\n')
const RUST_MOUSE = { None: 'none', X10: 'x10', Normal: 'vt200', Button: 'drag', Any: 'any' }
const xtModes = {
  altScreen: buf.type === 'alternate',
  bracketedPaste: xt.modes.bracketedPasteMode,
  applicationCursor: xt.modes.applicationCursorKeysMode,
  mouse: RUST_MOUSE[rustModes.mouse], // compare mapped value
  sgrMouse: rustModes.sgrMouse // (xterm folds SGR into mouseEncoding; checked below)
}

// ── Verdicts ────────────────────────────────────────────────────────────────
const gridOk = rustGrid === xtGrid
const modesOk =
  rustModes.altScreen === xtModes.altScreen &&
  rustModes.bracketedPaste === xtModes.bracketedPaste &&
  rustModes.applicationCursor === xtModes.applicationCursor &&
  RUST_MOUSE[rustModes.mouse] === xt.modes.mouseTrackingMode

console.log(
  `TUI corpus: ${(corpus.length / 1024 / 1024).toFixed(2)} MB · ${COLS}x${ROWS} · 400 frames\n`
)
console.log(
  `throughput : rust ${(corpus.length / 1024 / 1024 / (rustMs / 1000)).toFixed(0)} MB/s  vs  xterm ${(corpus.length / 1024 / 1024 / (xtMs / 1000)).toFixed(0)} MB/s`
)
console.log(
  `modes      : alt=${rustModes.altScreen} paste=${rustModes.bracketedPaste} appcursor=${rustModes.applicationCursor} mouse=${xt.modes.mouseTrackingMode}`
)
console.log(
  `  xterm    : alt=${xtModes.altScreen} paste=${xtModes.bracketedPaste} appcursor=${xtModes.applicationCursor} mouse=${xt.modes.mouseTrackingMode}`
)
console.log(`visible grid parity : ${gridOk ? '✅ identical' : '❌ MISMATCH'}`)
console.log(`mode flag parity    : ${modesOk ? '✅ identical' : '❌ MISMATCH'}`)

if (!gridOk) {
  const a = rustGrid.split('\n')
  const b = xtGrid.split('\n')
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) {
      console.log(
        `  row ${i}:\n    rust : ${JSON.stringify(a[i])}\n    xterm: ${JSON.stringify(b[i])}`
      )
    }
  }
}
if (!gridOk || !modesOk) {
  process.exit(1)
}
