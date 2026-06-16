// Focused reducer for vertical cursor moves inside a DECSTBM scroll region.
// Exhaustively sweeps (region, start row, origin mode, count, op) for the
// vertical relative-move ops and reports the distinct (op, origin) classes where
// the engine diverges from xterm.js. Isolates region-clamping bugs that the
// general fuzzer (hunt.mjs) only surfaces buried in noisy multi-op streams.
//   node focus-region.mjs
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const { HeadlessTerminal } = require(`${process.cwd()}/../../native/orca-node/orca_node.node`)
const E = '\x1b'

function rg(bytes, c, r) {
  const t = new HeadlessTerminal(c, r, 40)
  t.write(Buffer.from(bytes, 'latin1'))
  return {
    grid: t
      .snapshot()
      .map((s) => s.replace(/\s+$/, ''))
      .join('|'),
    cur: t.cursor()
  }
}

async function xg(bytes, c, r) {
  const t = new Terminal({ cols: c, rows: r, scrollback: 40, allowProposedApi: true })
  await new Promise((res) => t.write(Buffer.from(bytes, 'latin1'), res))
  const b = t.buffer.active
  const out = []
  for (let i = 0; i < r; i++) {
    const l = b.getLine(b.baseY + i)
    out.push((l ? l.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return { grid: out.join('|'), cur: [b.cursorY, b.cursorX] }
}

// Single vertical relative move with a small count.
const ops = (n) => [
  [`CUU ${n}`, `${E}[${n}A`],
  [`CUD ${n}`, `${E}[${n}B`],
  [`CNL ${n}`, `${E}[${n}E`],
  [`CPL ${n}`, `${E}[${n}F`],
  [`VPA ${n}`, `${E}[${n}d`],
  [`VPR ${n}`, `${E}[${n}e`],
  [`SU ${n}`, `${E}[${n}S`],
  [`SD ${n}`, `${E}[${n}T`],
  ['RI', `${E}M`],
  ['IND', `${E}D`],
  ['NEL', `${E}E`]
]

const rows = 8
const cols = 6
const seen = new Map()

for (let top = 1; top <= rows; top++) {
  for (let bot = top + 1; bot <= rows; bot++) {
    for (let startRow = 1; startRow <= rows; startRow++) {
      for (const origin of [false, true]) {
        for (let n = 1; n <= rows; n++) {
          for (const [name, op] of ops(n)) {
            const pre = `${origin ? `${E}[?6h` : ''}${E}[${top};${bot}r${E}[${startRow};1H`
            const bytes = `${pre}X${op}Y`
            const a = rg(bytes, cols, rows)
            const x = await xg(bytes, cols, rows)
            if (a.grid === x.grid && a.cur[0] === x.cur[0]) {
              continue
            }
            const key = `${name}|o${origin ? 1 : 0}`
            if (seen.has(key)) {
              continue
            }
            seen.set(key, { bytes, a, x, top, bot, startRow, origin, n })
          }
        }
      }
    }
  }
}

console.log(`${seen.size} distinct (op,origin) divergence classes:\n`)
for (const [k, v] of seen) {
  console.log(`[${k}] region ${v.top}-${v.bot} start@${v.startRow} n=${v.n} origin=${v.origin}`)
  console.log(`   bytes ${JSON.stringify(v.bytes)}`)
  console.log(`   xterm grid=${JSON.stringify(v.x.grid)} cur=${v.x.cur}`)
  console.log(`   aterm grid=${JSON.stringify(v.a.grid)} cur=${v.a.cur}\n`)
}
