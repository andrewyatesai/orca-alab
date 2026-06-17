// Focused reducer for DECSC/DECRC (save/restore cursor) interacting with scroll
// regions, scrolling, and origin mode. Save the cursor at a known spot, perform
// one intervening op, restore, print a marker, and diff cursor row + grid vs
// xterm.js. Isolates finding #4.
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

// intervening ops between save and restore
const ops = (n) => [
  [`SU ${n}`, `${E}[${n}S`],
  [`SD ${n}`, `${E}[${n}T`],
  [`IND`, `${E}D`],
  [`RI`, `${E}M`],
  [`NEL`, `${E}E`],
  [`LF*${n}`, '\n'.repeat(n)],
  [`setregion`, `${E}[2;4r`],
  [`CUP-home`, `${E}[1;1H`]
]

const rows = 8
const cols = 6
const seen = new Map()
for (let saveRow = 1; saveRow <= rows; saveRow++) {
  for (const origin of [false, true]) {
    for (let n = 1; n <= 4; n++) {
      for (const [name, op] of ops(n)) {
        // optional pre-region so save happens inside a region
        for (const [rname, region] of [
          ['noreg', ''],
          ['reg2-5', `${E}[2;5r`]
        ]) {
          const pre = `${origin ? `${E}[?6h` : ''}${region}${E}[${saveRow};2H`
          const bytes = `${pre}${E}7${op}${E}8X`
          const a = rg(bytes, cols, rows)
          const x = await xg(bytes, cols, rows)
          if (a.grid === x.grid && a.cur[0] === x.cur[0]) {
            continue
          }
          const key = `${name}|${rname}|o${origin ? 1 : 0}`
          if (seen.has(key)) {
            continue
          }
          seen.set(key, { bytes, a, x, saveRow, n })
        }
      }
    }
  }
}
console.log(`${seen.size} save/restore divergence classes:\n`)
let i = 0
for (const [k, v] of seen) {
  if (i++ >= 20) {
    console.log(`... (${seen.size - 20} more)`)
    break
  }
  console.log(`[${k}] save@${v.saveRow} n=${v.n}`)
  console.log(`   bytes ${JSON.stringify(v.bytes)}`)
  console.log(`   xterm grid=${JSON.stringify(v.x.grid)} cur=${v.x.cur}`)
  console.log(`   aterm grid=${JSON.stringify(v.a.grid)} cur=${v.a.cur}\n`)
}
