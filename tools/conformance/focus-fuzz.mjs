// Focused short-stream differential fuzzer: few ops, tiny grids, only the
// cursor/scroll/edit/wrap ops where the remaining divergences cluster. Short
// streams print as readable repros. Usage: node focus-fuzz.mjs [trials] [seed]
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const { HeadlessTerminal } = require(`${process.cwd()}/../../native/orca-node/orca_node.node`)

const TRIALS = Number(process.argv[2] ?? 30000)
const BASE = Number(process.argv[3] ?? 1)
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
  const p = (s) => out.push(s)
  const ops = 3 + Math.floor(rand() * 7)
  for (let i = 0; i < ops; i++) {
    const r = rand()
    const N = 1 + Math.floor(rand() * (rows + 1))
    if (r < 0.16) {
      p('ABCDE'.slice(0, 1 + Math.floor(rand() * 5)))
    } else if (r < 0.24) {
      p('\n')
    } else if (r < 0.3) {
      p('\r')
    } else if (r < 0.4) {
      p(`${E}[${1 + Math.floor(rand() * rows)};${1 + Math.floor(rand() * cols)}H`)
    } else if (r < 0.48) {
      p(`${E}[${N}${'ABLM'[Math.floor(rand() * 4)]}`)
    } // CUU CUD IL DL
    else if (r < 0.53) {
      p(`${E}[${N}${'ST'[Math.floor(rand() * 2)]}`)
    } // SU SD
    else if (r < 0.59) {
      p(`${E}[${1 + Math.floor(rand() * rows)};${1 + Math.floor(rand() * rows)}r`)
    } else if (r < 0.64) {
      p(`${E}M`)
    } // RI
    else if (r < 0.69) {
      p(`${E}[${N}${'ed'[Math.floor(rand() * 2)]}`)
    } // VPR / VPA
    else if (r < 0.74) {
      p(`${E}[${Math.floor(rand() * 3)}J`)
    } else if (r < 0.78) {
      p(`${E}[${Math.floor(rand() * 3)}K`)
    } else if (r < 0.82) {
      p(`${E}[${N}${'@PXC'[Math.floor(rand() * 4)]}`)
    } // ICH DCH ECH CUF
    else if (r < 0.86) {
      p(rand() < 0.5 ? `${E}[4h` : `${E}[4l`)
    } // IRM
    else if (r < 0.9) {
      p(['中', '世', 'é'][Math.floor(rand() * 3)])
    } // wide/combining
    else if (r < 0.93) {
      p('\b')
    } // BS
    else if (r < 0.96) {
      p(`${E}(0${'lqkx'[Math.floor(rand() * 4)]}${E}(B`)
    } // charset
    else {
      p('X')
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
    .join('|')
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
  return out.join('|')
}

let mism = 0
const seen = new Set()
for (let i = 0; i < TRIALS; i++) {
  const rand = rng(BASE + i)
  const cols = 4 + Math.floor(rand() * 6)
  const rows = 4 + Math.floor(rand() * 5)
  const bytes = gen(rand, cols, rows)
  const r = rg(bytes, cols, rows)
  const x = await xg(bytes, cols, rows)
  if (r !== x) {
    mism++
    const key = bytes.toString('latin1').replace(/[0-9]+/g, '#') // dedup by shape
    if (!seen.has(key) && seen.size < 14) {
      seen.add(key)
      console.log(`\nDIFF ${cols}x${rows}  ${JSON.stringify(bytes.toString('latin1'))}`)
      console.log(`  xterm=${x}`)
      console.log(`  rust =${r}`)
    }
  }
}
console.log(`\n${TRIALS - mism}/${TRIALS} match (${seen.size} distinct shapes shown)`)
