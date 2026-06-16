import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const sc = JSON.parse(process.argv[2])
const addon = join(here, '..', '..', 'native', 'orca-node', 'orca_node.node')
const nm = join(here, 'node_modules')

function runEngine(engine) {
  const out = `/tmp/orca-bench/live/${sc.name}-${engine}.json`
  const env = {
    ...process.env,
    NODE_PATH: nm,
    ORCA_RUST_TERMINAL: engine === 'rust' ? '1' : '0',
    ORCA_RUST_TERMINAL_ADDON: addon,
    SCENARIO_CMD: sc.cmd,
    SCENARIO_ARGS: JSON.stringify(sc.args),
    COLS: String(sc.cols),
    ROWS: String(sc.rows),
    NEWCOLS: String(sc.newCols),
    NEWROWS: String(sc.newRows),
    RESIZE_AT_MS: String(sc.resizeAtMs),
    DURATION_MS: String(sc.durationMs),
    OUT: out
  }
  execFileSync(join(nm, '.bin', 'tsx'), [join(here, 'resize-harness.ts')], {
    env,
    stdio: ['ignore', 'ignore', 'pipe'],
    timeout: sc.durationMs + 8000
  })
  let ok = true
  let vout = ''
  try {
    vout = execFileSync('node', [join(here, 'verify-resize.mjs'), out], { encoding: 'utf8' })
  } catch (e) {
    ok = false
    vout = (e.stdout || '') + (e.stderr || '')
  }
  const cap = JSON.parse(readFileSync(out, 'utf8'))
  return { engine, ok, bytes: cap.rawBytes, vout: vout.trim() }
}

const rust = runEngine('rust')
const ts = runEngine('ts')
const pass = rust.ok && ts.ok
console.error(
  `${pass ? 'PASS' : 'FAIL'} ${sc.name}  bytes=${rust.bytes} rust=${rust.ok ? 'ok' : 'FAIL'} ts=${ts.ok ? 'ok' : 'FAIL'}`
)
if (!rust.ok) {
  console.error(rust.vout)
}
if (!ts.ok) {
  console.error(`TS leg:\n${ts.vout}`)
}
process.exit(pass ? 0 : 1)
