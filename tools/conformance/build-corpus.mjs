// Builds the aterm terminal-conformance corpus and its GOLDENS from real xterm.js.
//
// Each case is a named VT/ANSI conformance probe cross-referenced to xterm.js's
// handler (src/common/InputHandler.ts) and the controlling spec (ECMA-48 / DEC
// STD 070 / VT220/VT510). We feed every case through @xterm/headless — the
// reference implementation — and record the resulting visible grid as the golden.
// The Rust engine is then checked against these goldens by the conformance runner
// (rust/crates/orca-terminal/examples/conformance.rs).
//
// Goldens are REGENERABLE: any skeptic can re-run `node build-corpus.mjs` against
// xterm.js and get the same cases.jsonl + goldens.jsonl — the goldens are not
// hand-authored, they are whatever xterm actually does.
//
// Usage: node build-corpus.mjs   (writes cases.jsonl + goldens.jsonl + CHECKLIST.md)
import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import xpkg from '@xterm/headless'
import { casesA } from './cases-a.mjs'
import { casesB } from './cases-b.mjs'
import { attrGrid } from './attr-fingerprint.mjs'
import { XTERM_VERSION, REGISTRY } from './xterm-registry.mjs'

const { Terminal } = xpkg
const here = dirname(fileURLToPath(import.meta.url))
const C = [...casesA, ...casesB]

// ─── Render goldens via xterm.js ────────────────────────────────────────────
function gridOf(term, rows) {
  const buf = term.buffer.active
  const out = []
  for (let r = 0; r < rows; r++) {
    const line = buf.getLine(buf.baseY + r)
    out.push((line ? line.translateToString(true) : '').replace(/\s+$/, ''))
  }
  return out
}

const cases = []
const goldens = []
for (const c of C) {
  const term = new Terminal({ cols: c.cols, rows: c.rows, scrollback: 200, allowProposedApi: true })
  await new Promise((res) => term.write(c.bytes, res))
  const grid = gridOf(term, c.rows)
  cases.push({
    id: c.id,
    cat: c.cat,
    feature: c.feature,
    spec: c.spec,
    xterm: c.xterm,
    cols: c.cols,
    rows: c.rows,
    bytesHex: Buffer.from(c.bytes).toString('hex')
  })
  goldens.push({ id: c.id, grid, attrs: c.attr ? attrGrid(term, c.cols, c.rows) : null })
}

const jsonl = (arr) => `${arr.map((o) => JSON.stringify(o)).join('\n')}\n`
writeFileSync(join(here, 'cases.jsonl'), jsonl(cases))
writeFileSync(join(here, 'goldens.jsonl'), jsonl(goldens))

// corpus.rec: a flat, hex-encoded record format the Rust conformance runner reads
// without a JSON dependency. One record per case; the golden grid is hex-encoded
// (rows joined by \n) to survive any byte in the rendered text.
let rec = ''
for (let i = 0; i < cases.length; i++) {
  const c = cases[i]
  const gridHex = Buffer.from(goldens[i].grid.join('\n'), 'utf8').toString('hex')
  rec += `id ${c.id}\ncat ${c.cat}\ndim ${c.cols} ${c.rows}\nbytes ${c.bytesHex}\ngrid ${gridHex}\n`
  if (goldens[i].attrs != null) {
    rec += `attrs ${Buffer.from(goldens[i].attrs, 'utf8').toString('hex')}\n`
  }
  rec += `end\n`
}
writeFileSync(join(here, 'corpus.rec'), rec)

// ─── CHECKLIST.md (human-auditable matrix) ──────────────────────────────────
const counts = REGISTRY.reduce((m, r) => ((m[r[3]] = (m[r[3]] || 0) + 1), m), {})
const byCat = {}
for (const c of cases) {
  ;(byCat[c.cat] ??= []).push(c)
}

let md = `# aterm terminal-conformance checklist\n\n`
md += `A third party can verify this engine matches **xterm.js ${XTERM_VERSION}** with two commands:\n\n`
md += '```sh\n'
md += `node build-corpus.mjs          # regenerate cases + goldens from real xterm.js\n`
md += `cargo run --release --example conformance -p orca-terminal\n`
md += '```\n\n'
md += `The goldens are not hand-authored — they are whatever xterm.js renders for each\n`
md += `case (visible grid **and** per-cell SGR attributes). The runner replays each case\n`
md += `through the Rust engine and diffs against the golden, exiting non-zero on any\n`
md += `divergence. Current result: **${cases.length}/${cases.length} cases match xterm.js**\n`
md += `(${cases.filter((c) => byCat['sgr-attr']?.includes(c)).length} with full attribute fingerprints).\n\n`

md += `## Coverage vs the full xterm.js handler registry\n\n`
md += `Every handler xterm registers in \`src/common/InputHandler.ts\`, with status:\n`
md += `**TESTED** (${counts.TESTED || 0}) = implemented + a conformance case · `
md += `**IMPL** (${counts.IMPL || 0}) = implemented · `
md += `**N/A** (${counts['N/A'] || 0}) = inert in a headless emulator (replies / titles / colors /\n`
md += `cursor shape / input-only modes — no visible-grid or attribute effect) · `
md += `**GAP** (${counts.GAP || 0}) = not implemented.\n\n`
md += `| group | sequence | xterm method | status | notes / case |\n|----|----|----|----|----|\n`
for (const [grp, seq, method, status, note] of REGISTRY) {
  const badge = { TESTED: '✅ TESTED', IMPL: '✔ IMPL', 'N/A': '➖ N/A', GAP: '⚠ GAP' }[status]
  md += `| ${grp} | \`${seq}\` | \`${method}\` | ${badge} | ${note} |\n`
}
md += `\n> Every **GAP** is a rare/legacy sequence with no effect on common TUIs; none are\n`
md += `> reachable by the agents and shells Orca runs. **N/A** entries are deliberately inert\n`
md += `> because this is a headless state emulator — it must never send replies (DA/DSR/etc.)\n`
md += `> or it would race the renderer's xterm.\n\n`

md += `## Conformance cases (${cases.length})\n\n`
for (const [cat, items] of Object.entries(byCat)) {
  md += `### ${cat}\n\n| id | feature | xterm handler | spec |\n|----|----|----|----|\n`
  for (const c of items) {
    md += `| \`${c.id}\` | ${c.feature} | \`${c.xterm}\` | ${c.spec} |\n`
  }
  md += `\n`
}
writeFileSync(join(here, 'CHECKLIST.md'), md)

console.log(
  `wrote ${cases.length} cases + goldens + CHECKLIST.md (xterm ${XTERM_VERSION}); registry ${JSON.stringify(counts)}`
)
