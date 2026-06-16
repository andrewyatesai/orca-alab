// Orchestrates the head-to-head: generates the corpus (via the Rust example),
// runs xterm.js and the Rust napi addon N times each, and prints medians + the
// parity verdict. Usage: node run.mjs [trials] [corpusMB]
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join, resolve } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '..', '..')
const trials = Number(process.argv[2] ?? 5)
const corpusMB = Number(process.argv[3] ?? 16)
const corpus = '/tmp/orca-bench/corpus.bin'
const addon = join(repo, 'native', 'orca-node', 'orca_node.node')

const run = (cmd, args) =>
  execFileSync(cmd, args, { cwd: here, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
const median = (xs) => xs.slice().sort((a, b) => a - b)[Math.floor(xs.length / 2)]

console.log(
  `corpus: ${corpusMB} MB · trials: ${trials} · chunk: 4096 B · grid: 120x40 · scrollback: 5000\n`
)

// xterm.js (the engine Orca ships today)
const xtermRates = []
let xtermSha
for (let i = 0; i < trials; i++) {
  run('node', ['xterm-bench.mjs', corpus, '/tmp/orca-bench/ts.json'])
  const j = JSON.parse(readFileSync('/tmp/orca-bench/ts.json'))
  xtermRates.push(j.mb_per_s)
  xtermSha = j.visible_sha
}

// Rust orca-terminal via the napi addon (what the app loads behind the flag)
const addonRates = []
let addonSha
for (let i = 0; i < trials; i++) {
  run('node', ['addon-bench.mjs', addon, corpus, '/tmp/orca-bench/addon.json'])
  const j = JSON.parse(readFileSync('/tmp/orca-bench/addon.json'))
  addonRates.push(j.mb_per_s)
  addonSha = j.visible_sha
}

const xMed = median(xtermRates)
const aMed = median(addonRates)
const pad = (s, n) => String(s).padEnd(n)
console.log(`${pad('engine', 26) + pad('median MB/s', 14)}visible grid`)
console.log('-'.repeat(60))
console.log(pad('@xterm/headless (shipped)', 26) + pad(xMed.toFixed(1), 14) + xtermSha)
console.log(pad('rust orca-terminal (napi)', 26) + pad(aMed.toFixed(1), 14) + addonSha)
console.log('-'.repeat(60))
console.log(
  `speedup: ${(aMed / xMed).toFixed(2)}x   parity: ${xtermSha === addonSha ? '✅ identical' : '❌ DIFFER'}`
)
