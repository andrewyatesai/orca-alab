// TS dispatch for the feature-education-telemetry parity module. The shared TS
// normalizers were DELETED (the Rust orca-config core is the sole impl — the
// renderer drives them via the orca-git wasm), so this adapter drives the SAME
// wasm: the vectors' recorded goldens now pin that surface, and the harness's
// TS-vs-Rust diff degenerates to wasm-vs-binary (drift between the two Rust
// entry points would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('feature-education-telemetry', fn, JSON.stringify(input ?? null))
  )
}
