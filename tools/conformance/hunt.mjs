// Fuzz + auto-minimize: generates random VT streams, and on each engine/xterm
// divergence binary-searches the shortest diverging prefix, dedups by "shape",
// and prints clean minimal repros of DISTINCT root causes. Origin mode (?6h) is
// excluded — those are the documented xterm-deviation class, tracked separately.
//   node hunt.mjs [trials] [seed] [maxShapes]
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const { HeadlessTerminal } = require(`${process.cwd()}/../../native/orca-node/orca_node.node`)

const TRIALS = Number(process.argv[2] ?? 4000)
const BASE = Number(process.argv[3] ?? 1)
const MAX_SHAPES = Number(process.argv[4] ?? 10)
const E = '\x1b'

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

function gen(rand, cols, rows) {
  const out = []
  const push = (s) => out.push(s)
  const n = () => 1 + Math.floor(rand() * 8)
  const ops = 6 + Math.floor(rand() * 40)
  for (let i = 0; i < ops; i++) {
    const r = rand()
    if (r < 0.32) {
      let s = ''
      for (let k = 0, len = 1 + Math.floor(rand() * 10); k < len; k++) {
        s += String.fromCharCode(0x21 + Math.floor(rand() * 0x5d))
      }
      push(s)
    } else if (r < 0.4) {
      push(rand() < 0.5 ? '\r\n' : '\n')
    } else if (r < 0.48) {
      push(
        `${E}[${rand() < 0.3 ? Math.floor(rand() * (rows + cols)) : n()}${'ABCDEFGHdae'[Math.floor(rand() * 11)]}`
      )
    } else if (r < 0.54) {
      push(`${E}[${n()};${n()}H`)
    } else if (r < 0.6) {
      push(`${E}[${Math.floor(rand() * 4)}J`)
    } else if (r < 0.66) {
      push(`${E}[${Math.floor(rand() * 3)}K`)
    } else if (r < 0.72) {
      push(`${E}[${n()}${'@PLMX'[Math.floor(rand() * 5)]}`)
    } else if (r < 0.76) {
      push(`${E}[${n()}${'ST'[Math.floor(rand() * 2)]}`)
    } else if (r < 0.8) {
      push(`${E}[${1 + Math.floor(rand() * rows)};${1 + Math.floor(rand() * rows)}r`)
    } else if (r < 0.84) {
      push(`${E}[${[1, 2, 3, 4, 7, 9].map(() => 30 + Math.floor(rand() * 10)).join(';')}m`)
    } else if (r < 0.87) {
      push(rand() < 0.5 ? `${E}[4h` : `${E}[4l`)
    } else if (r < 0.9) {
      push(rand() < 0.5 ? `${E}7` : `${E}8`)
    } else if (r < 0.94) {
      push(`${E}(0${'lqkmjxntuvw'[Math.floor(rand() * 11)]}${E}(B`)
    } else if (r < 0.97) {
      push(['中', '世', 'é'][Math.floor(rand() * 3)])
    } else {
      push(rand() < 0.5 ? '\t' : '\b')
    }
  }
  return Buffer.from(out.join(''), 'utf8')
}

function rg(bytes, cols, rows) {
  const t = new HeadlessTerminal(cols, rows, 100)
  t.write(bytes)
  return t
    .snapshot()
    .map((s) => s.replace(/\s+$/, ''))
    .join('\n')
}
async function xg(bytes, cols, rows) {
  const t = new Terminal({ cols, rows, scrollback: 100, allowProposedApi: true })
  await new Promise((res) => t.write(bytes, res))
  const b = t.buffer.active
  const out = []
  for (let r = 0; r < rows; r++) {
    const l = b.getLine(b.baseY + r)
    out.push((l ? l.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return out.join('\n')
}
const diff = async (b, c, r) => rg(b, c, r) !== (await xg(b, c, r))

async function minimize(bytes, cols, rows) {
  let lo = 1
  let hi = bytes.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (await diff(bytes.subarray(0, mid), cols, rows)) {
      hi = mid
    } else {
      lo = mid + 1
    }
  }
  return bytes.subarray(0, lo)
}

const seen = new Set()
let mism = 0
for (let i = 0; i < TRIALS && seen.size < MAX_SHAPES; i++) {
  const rand = rng(BASE + i)
  const cols = 6 + Math.floor(rand() * 30)
  const rows = 4 + Math.floor(rand() * 16)
  const bytes = gen(rand, cols, rows)
  if (!(await diff(bytes, cols, rows))) {
    continue
  }
  mism++
  const min = await minimize(bytes, cols, rows)
  const repr = min.toString('latin1')
  const shape = repr.replace(/[0-9]+/g, '#').replace(/[A-Za-z]{2,}/g, 'T') // crude dedup key
  if (seen.has(shape)) {
    continue
  }
  seen.add(shape)
  const x = (await xg(min, cols, rows)).split('\n')
  const r = rg(min, cols, rows).split('\n')
  console.log(`\n[${seen.size}] ${cols}x${rows}  ${JSON.stringify(repr)}`)
  for (let k = 0; k < Math.max(x.length, r.length); k++) {
    if (x[k] !== r[k]) {
      console.log(`    row ${k}: xterm=${JSON.stringify(x[k])}  aterm=${JSON.stringify(r[k])}`)
    }
  }
}
console.log(`\n${seen.size} distinct shapes from ${mism} mismatches in ${TRIALS} trials`)
