// Runs ONE scenario through the real daemon Session under both engines and emits
// a structured verdict. For each engine the Session consumes a live PTY, then we
// check its snapshot renders identically to xterm parsing the exact same bytes
// (self-consistent ground truth, robust to nondeterministic programs).
//
// Usage: node run-scenario.mjs <scenarioName>
// Emits one JSON line to stdout (and human-readable lines to stderr).
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { SCENARIOS } from './scenarios.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const name = process.argv[2]
// Ad-hoc mode: `run-scenario.mjs --adhoc '<json>'` where json is
// {name?,cmd,args,inputs?,durationMs?,cols?,rows?}. Lets the swarm throw
// arbitrary scenarios without editing the catalog.
let sc
let scenarioLabel = name
if (name === '--adhoc') {
  sc = JSON.parse(process.argv[3])
  scenarioLabel = sc.name ?? 'adhoc'
} else {
  sc = SCENARIOS[name]
}
if (!sc) {
  console.error(`unknown scenario: ${name}`)
  process.exit(2)
}
const addon = join(here, '..', '..', 'native', 'orca-node', 'orca_node.node')
const nm = join(here, 'node_modules')
const cols = sc.cols ?? 100
const rows = sc.rows ?? 30

function runEngine(engine) {
  const out = `/tmp/orca-bench/live/${scenarioLabel}-${engine}.json`
  const env = {
    ...process.env,
    NODE_PATH: nm,
    ORCA_RUST_TERMINAL: engine === 'rust' ? '1' : '0',
    ORCA_RUST_TERMINAL_ADDON: addon,
    SCENARIO_CMD: sc.cmd,
    SCENARIO_ARGS: JSON.stringify(sc.args),
    INPUTS: JSON.stringify(sc.inputs ?? []),
    COLS: String(cols),
    ROWS: String(rows),
    DURATION_MS: String(sc.durationMs ?? 1500),
    OUT: out
  }
  execFileSync(join(nm, '.bin', 'tsx'), [join(here, 'session-live-harness.ts')], {
    env,
    stdio: ['ignore', 'ignore', 'pipe'],
    timeout: (sc.durationMs ?? 1500) + 8000
  })
  // verify-live exits non-zero on mismatch
  let verifyOk = true
  let verifyOut = ''
  try {
    verifyOut = execFileSync('node', [join(here, 'verify-live.mjs'), out], { encoding: 'utf8' })
  } catch (e) {
    verifyOk = false
    verifyOut = (e.stdout || '') + (e.stderr || '')
  }
  const cap = JSON.parse(readFileSync(out, 'utf8'))
  return {
    engine,
    verifyOk,
    bytes: cap.rawBytes,
    alt: cap.modes?.alternateScreen ?? false,
    scrollback: cap.scrollbackLines,
    verifyOut: verifyOut.trim()
  }
}

const rust = runEngine('rust')
const ts = runEngine('ts')
const pass = rust.verifyOk && ts.verifyOk
const verdict = {
  scenario: scenarioLabel,
  pass,
  altScreen: rust.alt,
  bytes: rust.bytes,
  rustVerify: rust.verifyOk,
  tsVerify: ts.verifyOk,
  expectedAlt: sc.alt ?? null
}
console.error(
  `${pass ? '✅' : '❌'} ${scenarioLabel}  bytes=${rust.bytes} alt=${rust.alt} rust=${rust.verifyOk ? 'ok' : 'FAIL'} ts=${ts.verifyOk ? 'ok' : 'FAIL'}`
)
if (!rust.verifyOk) {
  console.error(rust.verifyOut)
}
console.log(JSON.stringify(verdict))
process.exit(pass ? 0 : 1)
