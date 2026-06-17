// Focused reducer for editing/erase ops interacting with double-width glyphs.
// Places a wide char at a known column, runs one edit/erase op with the cursor
// on/before/after the wide pair, and diffs the row vs xterm.js. Isolates the
// "wide pair split leaves a stray cell" class (finding #2).
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'
const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const { HeadlessTerminal } = require(`${process.cwd()}/../../native/orca-node/orca_node.node`)
const E = '\x1b'

function rrow(bytes, c, r) {
  const t = new HeadlessTerminal(c, r, 20)
  t.write(Buffer.from(bytes, 'utf8'))
  return t
    .snapshot()
    .map((s) => s.replace(/\s+$/, ''))
    .join('|')
}
async function xrow(bytes, c, r) {
  const t = new Terminal({ cols: c, rows: r, scrollback: 20, allowProposedApi: true })
  await new Promise((z) => t.write(Buffer.from(bytes, 'utf8'), z))
  const b = t.buffer.active,
    out = []
  for (let i = 0; i < r; i++) {
    const l = b.getLine(b.baseY + i)
    out.push((l ? l.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return out.join('|')
}

// edit/erase ops to try at the cursor
const OPS = [
  ['ICH1', `${E}[1@`],
  ['ICH2', `${E}[2@`],
  ['DCH1', `${E}[1P`],
  ['DCH2', `${E}[2P`],
  ['ECH1', `${E}[1X`],
  ['ECH2', `${E}[2X`],
  ['EL0', `${E}[0K`],
  ['EL1', `${E}[1K`],
  ['BS+print', `\bZ`],
  ['IRM', `${E}[4hZ`]
]
const cols = 8,
  rows = 2
const seen = new Map()
// layout: some ascii, a wide glyph, more ascii; cursor parked at column k via CHA
const prefixes = [`AB中CD`, `中CD`, `AB中`, `A中中B`, `中`]
for (const layout of prefixes) {
  for (let k = 1; k <= cols; k++) {
    for (const [name, op] of OPS) {
      const bytes = `${layout}${E}[${k}G${op}`
      const a = rrow(bytes, cols, rows)
      const x = await xrow(bytes, cols, rows)
      if (a === x) {
        continue
      }
      const key = `${name}@col${k}|${layout}`
      if (seen.has(key)) {
        continue
      }
      seen.set(key, { bytes, a, x })
    }
  }
}
console.log(`${seen.size} wide-edit divergences:\n`)
let i = 0
for (const [k, v] of seen) {
  if (i++ >= 25) {
    console.log(`... (${seen.size - 25} more)`)
    break
  }
  console.log(
    `[${k}]\n   bytes ${JSON.stringify(v.bytes)}\n   xterm ${JSON.stringify(v.x)}\n   aterm ${JSON.stringify(v.a)}\n`
  )
}
