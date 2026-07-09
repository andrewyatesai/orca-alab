// TS dispatch for the setup-script-telemetry parity module. The shared TS
// builders were DELETED (the Rust orca_core::setup_script_telemetry port is the
// sole impl — the renderer drives it via the orca-git wasm), so this adapter
// drives the same wasm: the vectors' recorded goldens now pin that surface, and
// the harness's TS-vs-Rust diff degenerates to wasm-vs-binary (drift between the
// two Rust entry points would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  const json = gitWasmOracle().orcaDispatch(
    'setup-script-telemetry',
    fn,
    JSON.stringify(input ?? null)
  )
  return JSON.parse(json)
}
