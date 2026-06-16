// Minimize a failing fuzz stream: binary-search the shortest byte prefix that
// still diverges, then print it + the two grids. Usage: node bisect.mjs <hex> <cols> <rows>
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const { HeadlessTerminal } = require(`${process.cwd()}/../../native/orca-node/orca_node.node`)

const hex = process.argv[2]
const cols = Number(process.argv[3])
const rows = Number(process.argv[4])
const full = Buffer.from(hex, 'hex')

function rg(bytes) {
  const t = new HeadlessTerminal(cols, rows, 200)
  t.write(bytes)
  return t
    .snapshot()
    .map((s) => s.replace(/\s+$/, ''))
    .join('\n')
}
async function xg(bytes) {
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
const diverges = async (bytes) => rg(bytes) !== (await xg(bytes))

// shortest diverging prefix
let lo = 1
let hi = full.length
while (lo < hi) {
  const mid = (lo + hi) >> 1
  if (await diverges(full.subarray(0, mid))) {
    hi = mid
  } else {
    lo = mid + 1
  }
}
const prefix = full.subarray(0, lo)
console.log(`shortest diverging prefix: ${lo}/${full.length} bytes`)
console.log(`hex: ${prefix.toString('hex')}`)
console.log(`repr: ${JSON.stringify(prefix.toString('latin1'))}`)
const a = (await xg(prefix)).split('\n')
const b = rg(prefix).split('\n')
for (let i = 0; i < Math.max(a.length, b.length); i++) {
  if (a[i] !== b[i]) {
    console.log(`  row ${i}: xterm=${JSON.stringify(a[i])}  rust=${JSON.stringify(b[i])}`)
  }
}
