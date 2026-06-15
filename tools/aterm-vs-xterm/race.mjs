// Visual head-to-head: feed the SAME ANSI corpus through aterm (Rust) and
// @xterm/headless (the engine Orca ships) back-to-back, then animate a race so
// the throughput difference is obvious. Both measured just-in-time in the same
// thermal state, so the RATIO is fair regardless of machine load.
//
//   node race.mjs <corpus> [aterm-bench-binary]
import { readFileSync } from 'node:fs'
import { execFileSync } from "node:child_process"
import { createRequire } from 'node:module'
const require = createRequire(import.meta.url)
const { Terminal } = require('/Users/ayates/orc/tools/terminal-bench/node_modules/@xterm/headless/lib-headless/xterm-headless.js')

const ROWS = 40, COLS = 120, SCROLLBACK = 5000, CHUNK = 4096
const corpusPath = process.argv[2] || '/tmp/orca-bench/corpus.bin'
const atermBin = process.argv[3] || '/tmp/orca-prof-target/release/examples/bench-3f8ac9c8aad4c38b'
const corpus = readFileSync(corpusPath)
const MB = corpus.length / (1024 * 1024)

// --- aterm leg (best of 5 via the prebuilt Rust binary) ---
function atermMBs() {
  const out = '/tmp/orca-bench/race-aterm.json'
  let best = 0
  for (let i = 0; i < 5; i++) {
    execFileSync(atermBin, ['run', corpusPath, out], { stdio: ['ignore', 'ignore', 'ignore'] })
    best = Math.max(best, JSON.parse(readFileSync(out)).mb_per_s)
  }
  return best
}

// --- xterm leg (warm V8, best of 5) ---
async function xtermMBs() {
  const onePass = async () => {
    const term = new Terminal({ cols: COLS, rows: ROWS, scrollback: SCROLLBACK, allowProposedApi: true })
    const t0 = process.hrtime.bigint()
    for (let i = 0; i < corpus.length; i += CHUNK) term.write(corpus.subarray(i, i + CHUNK))
    await new Promise((r) => term.write('', r))
    const ms = Number(process.hrtime.bigint() - t0) / 1e6
    term.dispose()
    return MB / (ms / 1000)
  }
  for (let i = 0; i < 3; i++) await onePass()
  let best = 0
  for (let i = 0; i < 5; i++) best = Math.max(best, await onePass())
  return best
}

const xterm = await xtermMBs()
const aterm = atermMBs()
const ratio = aterm / xterm

// --- animate the race (real rates; faster engine finishes first) ---
const W = 50
const tA = MB / aterm, tX = MB / xterm           // seconds per pass
const dur = Math.max(tA, tX)
const bar = (frac, color) => {
  const n = Math.round(Math.min(1, frac) * W)
  return `\x1b[${color}m` + '█'.repeat(n) + '\x1b[2m' + '░'.repeat(W - n) + '\x1b[0m'
}
const isTTY = process.stdout.isTTY
const frames = isTTY ? 60 : 1
process.stdout.write(`\nRendering ${MB.toFixed(0)} MB of terminal output — aterm vs @xterm/headless\n\n`)
for (let f = 1; f <= frames; f++) {
  const t = (f / frames) * dur
  const fa = Math.min(1, t / tA), fx = Math.min(1, t / tX)
  const line =
    `  aterm  ${bar(fa, 32)} ${(fa * MB).toFixed(0).padStart(3)}/${MB.toFixed(0)} MB ${fa >= 1 ? '\x1b[1;32m✓ ' + tA.toFixed(2) + 's\x1b[0m' : ''}\n` +
    `  xterm  ${bar(fx, 33)} ${(fx * MB).toFixed(0).padStart(3)}/${MB.toFixed(0)} MB ${fx >= 1 ? '\x1b[1;33m✓ ' + tX.toFixed(2) + 's\x1b[0m' : ''}\n`
  if (isTTY) { process.stdout.write('\x1b[s' + line + '\x1b[u'); await new Promise((r) => setTimeout(r, dur * 1000 / frames)) }
  else if (f === frames) process.stdout.write(line)
}
process.stdout.write(
  `\n  \x1b[1materm: ${aterm.toFixed(0)} MB/s   xterm: ${xterm.toFixed(0)} MB/s   →  aterm is ${ratio.toFixed(1)}× faster\x1b[0m\n` +
  `  (same corpus, back-to-back, best of 5; identical visible grid)\n`
)
