// Differential fuzzer: the real test of parity. Generates large volumes of
// random-but-structured VT/ANSI byte streams from a weighted grammar, feeds the
// SAME bytes through both xterm.js (reference) and the Rust orca-terminal engine,
// and reports ANY visible-grid divergence with a reproducible seed + byte dump.
//
// Curated conformance cases prove what we thought of; this proves we didn't miss
// anything across the supported surface. Deterministic (seeded), so every finding
// reproduces exactly.
//
//   node fuzz-diff.mjs [trials] [seed]
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const { HeadlessTerminal } = require(`${process.cwd()}/../../native/orca-node/orca_node.node`)

const TRIALS = Number(process.argv[2] ?? 20000)
const BASE_SEED = Number(process.argv[3] ?? 1)

// Deterministic RNG (mulberry32).
function rng(seed) {
  let s = seed >>> 0
  return () => {
    s = (s + 0x6d2b79f5) >>> 0
    let t = s
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

// Generate one random byte stream from the SUPPORTED-surface grammar. We avoid
// sequences documented as out-of-scope (selective erase, GR locking shifts, C1
// 8-bit, DECSCA) and reply-only sequences (DA/DSR) which never change the grid.
function genStream(rand, cols, rows) {
  const out = []
  const push = (s) => out.push(Buffer.from(s, 'utf8'))
  const ESC = '\x1b'
  const n = () => 1 + Math.floor(rand() * 8) // small param bias
  const big = () => Math.floor(rand() * (rows + cols)) // sometimes out of range
  const ops = 8 + Math.floor(rand() * 60)
  for (let i = 0; i < ops; i++) {
    const r = rand()
    if (r < 0.34) {
      // printable run
      let s = ''
      const len = 1 + Math.floor(rand() * 12)
      for (let k = 0; k < len; k++) {
        s += String.fromCharCode(0x21 + Math.floor(rand() * 0x5d))
      }
      push(s)
    } else if (r < 0.42) {
      push('\r\n'[Math.floor(rand() * 2)] === '\r' ? (rand() < 0.5 ? '\r' : '\r\n') : '\n')
    } else if (r < 0.5) {
      // cursor move
      const verb = 'ABCDEFGHd`ae'[Math.floor(rand() * 12)]
      push(`${ESC}[${rand() < 0.3 ? big() : n()}${verb}`)
    } else if (r < 0.56) {
      push(`${ESC}[${n()};${n()}H`) // CUP
    } else if (r < 0.62) {
      push(`${ESC}[${Math.floor(rand() * 4)}J`) // ED 0-3
    } else if (r < 0.68) {
      push(`${ESC}[${Math.floor(rand() * 3)}K`) // EL 0-2
    } else if (r < 0.74) {
      // edit ops @ P L M X
      push(`${ESC}[${n()}${'@PLMX'[Math.floor(rand() * 5)]}`)
    } else if (r < 0.78) {
      push(`${ESC}[${n()}${'ST'[Math.floor(rand() * 2)]}`) // SU/SD
    } else if (r < 0.82) {
      // SGR (color/style)
      const codes = []
      const cnt = 1 + Math.floor(rand() * 3)
      for (let k = 0; k < cnt; k++) {
        const pick = Math.floor(rand() * 6)
        if (pick === 0) {
          codes.push(30 + Math.floor(rand() * 8))
        } else if (pick === 1) {
          codes.push(40 + Math.floor(rand() * 8))
        } else if (pick === 2) {
          codes.push([1, 2, 3, 4, 5, 7, 8, 9, 53][Math.floor(rand() * 9)])
        } else if (pick === 3) {
          codes.push(0)
        } else if (pick === 4) {
          codes.push([22, 23, 24, 25, 27, 28, 29, 39, 49, 55][Math.floor(rand() * 10)])
        } else {
          codes.push(90 + Math.floor(rand() * 8))
        }
      }
      push(`${ESC}[${codes.join(';')}m`)
    } else if (r < 0.85) {
      push(`${ESC}[${1 + Math.floor(rand() * rows)};${1 + Math.floor(rand() * rows)}r`) // DECSTBM
    } else if (r < 0.88) {
      push(`${ESC}[?${[6, 7, 25][Math.floor(rand() * 3)]}${rand() < 0.5 ? 'h' : 'l'}`) // DECSET/RST
    } else if (r < 0.9) {
      push(rand() < 0.5 ? `${ESC}[4h` : `${ESC}[4l`) // IRM
    } else if (r < 0.93) {
      push(rand() < 0.5 ? `${ESC}7` : `${ESC}8`) // DECSC/DECRC
    } else if (r < 0.955) {
      // charset graphics
      push(`${ESC}(0`)
      push('lqkmjxntuvw'[Math.floor(rand() * 11)])
      push(`${ESC}(B`)
    } else if (r < 0.975) {
      push('\t')
    } else if (r < 0.99) {
      // wide CJK + combining
      push(['中', '世', '界', 'é', 'á', '中́'][Math.floor(rand() * 6)])
    } else {
      push('\b') // backspace
    }
  }
  return Buffer.concat(out)
}

function rustGrid(bytes, cols, rows) {
  const t = new HeadlessTerminal(cols, rows, 200)
  t.write(bytes)
  return t
    .snapshot()
    .map((s) => s.replace(/\s+$/, ''))
    .join('\n')
}
async function xtermGrid(bytes, cols, rows) {
  const t = new Terminal({ cols, rows, scrollback: 200, allowProposedApi: true })
  await new Promise((res) => t.write(bytes, res))
  const buf = t.buffer.active
  const lines = []
  for (let r = 0; r < rows; r++) {
    const line = buf.getLine(buf.baseY + r)
    lines.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return lines.join('\n')
}

let mismatches = 0
const failures = []
for (let i = 0; i < TRIALS; i++) {
  const seed = BASE_SEED + i
  const rand = rng(seed)
  const cols = 8 + Math.floor(rand() * 60)
  const rows = 4 + Math.floor(rand() * 24)
  const bytes = genStream(rand, cols, rows)
  const rg = rustGrid(bytes, cols, rows)
  const xg = await xtermGrid(bytes, cols, rows)
  if (rg !== xg) {
    mismatches++
    if (failures.length < 12) {
      failures.push({ seed, cols, rows, bytesHex: bytes.toString('hex'), rust: rg, xterm: xg })
    }
  }
  if ((i + 1) % 5000 === 0) {
    process.stderr.write(`  ${i + 1}/${TRIALS} trials, ${mismatches} mismatches\n`)
  }
}

console.log(
  `\n=== ${TRIALS - mismatches} / ${TRIALS} random streams match xterm.js (seed base ${BASE_SEED}) ===`
)
for (const f of failures) {
  console.log(`\nMISMATCH seed=${f.seed} ${f.cols}x${f.rows}`)
  console.log(`  bytes: ${f.bytesHex}`)
  const a = f.xterm.split('\n')
  const b = f.rust.split('\n')
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) {
      console.log(`  row ${i}: xterm=${JSON.stringify(a[i])}  rust=${JSON.stringify(b[i])}`)
    }
  }
}
process.exit(mismatches > 0 ? 1 : 0)
