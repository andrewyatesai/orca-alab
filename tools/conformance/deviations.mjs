// Registry of confirmed places where the reference (xterm.js 6.1.0-beta.220)
// deviates from the controlling spec (ECMA-48 / DEC STD 070 / VT5xx). The
// differential fuzzer surfaces divergences; the ones where *xterm* is wrong land
// here so they are documented EXPLICITLY rather than silently tolerated.
//
// `node deviations.mjs` re-verifies each entry against the live xterm.js and the
// engine, and regenerates XTERM-DEVIATIONS.md. A deviation is only valid if:
//   (1) xterm still exhibits the non-spec behaviour (probe matches `xterm`)
//   (2) the engine follows the spec (probe matches `correct`)
// If either fails, the entry is stale and the run exits non-zero.
import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { createRequire } from 'node:module'
import xpkg from '@xterm/headless'

const { Terminal } = xpkg
const require = createRequire(import.meta.url)
const here = dirname(fileURLToPath(import.meta.url))
const { HeadlessTerminal } = require(
  join(here, '..', '..', 'native', 'orca-node', 'orca_node.node')
)
const E = '\x1b'

// Each deviation: a minimal repro + a probe reading some scalar (cursor row, …)
// so xterm's wrong value and the spec-correct value are concrete.
export const DEVIATIONS = [
  {
    id: 'cuu-down-from-top-margin-under-origin',
    title: 'CUU moves the cursor DOWN, away from the top margin (origin mode)',
    bytes: `${E}[?6h${E}[4;17r${E}[8A`,
    cols: 17,
    rows: 19,
    spec: 'ECMA-48 §8.3.22 (CUU): the active position moves UP by n lines, stopping at the top margin.',
    probe: 'cursor row after the sequence',
    xterm: 6,
    correct: 3,
    note: 'With origin mode + a scroll region, xterm moves the cursor downward instead of clamping it to the top margin. No real program relies on this; the engine clamps per spec.'
  }
]

function rustCursorRow(bytes, cols, rows) {
  const t = new HeadlessTerminal(cols, rows, 40)
  t.write(Buffer.from(bytes, 'latin1'))
  return t.cursor()[0]
}
async function xtermCursorRow(bytes, cols, rows) {
  const t = new Terminal({ cols, rows, scrollback: 40, allowProposedApi: true })
  await new Promise((r) => t.write(Buffer.from(bytes, 'latin1'), r))
  return t.buffer.active.cursorY
}

let stale = 0
let md = `# xterm.js spec deviations\n\n`
md += `Places where the reference implementation (**xterm.js 6.1.0-beta.220**) deviates\n`
md += `from ECMA-48 / DEC specs, found by the differential fuzzer. The engine follows\n`
md += `the spec in each case. Re-verify with \`node deviations.mjs\` — entries are rejected\n`
md += `if xterm no longer deviates or the engine no longer matches the spec.\n\n`

for (const d of DEVIATIONS) {
  const xv = await xtermCursorRow(d.bytes, d.cols, d.rows)
  const rv = rustCursorRow(d.bytes, d.cols, d.rows)
  const xtermOk = xv === d.xterm
  const rustOk = rv === d.correct
  if (!xtermOk || !rustOk) {
    stale++
    console.error(
      `STALE ${d.id}: xterm probe=${xv} (expected ${d.xterm}, ${xtermOk ? 'ok' : 'CHANGED'}); ` +
        `engine probe=${rv} (expected ${d.correct}, ${rustOk ? 'ok' : 'CHANGED'})`
    )
  } else {
    console.log(`✓ ${d.id}: xterm=${xv} (non-spec), engine=${rv} (spec-correct)`)
  }
  md += `## ${d.title}\n\n`
  md += `- **Repro** (${d.cols}×${d.rows}): \`${Buffer.from(d.bytes, 'latin1').toString('hex')}\`\n`
  md += `- **Spec**: ${d.spec}\n`
  md += `- **Probe**: ${d.probe}\n`
  md += `- **xterm.js**: ${xv}${xtermOk ? '' : ' ⚠ CHANGED'} (deviates)\n`
  md += `- **Spec-correct / engine**: ${rv}${rustOk ? '' : ' ⚠ CHANGED'}\n`
  md += `- ${d.note}\n\n`
}

writeFileSync(join(here, 'XTERM-DEVIATIONS.md'), md)
console.log(`\n${DEVIATIONS.length} deviation(s), ${stale} stale. Wrote XTERM-DEVIATIONS.md`)
process.exit(stale > 0 ? 1 : 0)
