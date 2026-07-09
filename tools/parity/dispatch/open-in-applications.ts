// TS dispatch for the open-in-applications parity module: the shared TS body was
// DELETED (the Rust orca-config core is the sole impl), so this adapter drives
// the SAME wasm the renderer runs via orcaDispatch — the vectors' recorded
// goldens keep pinning that surface, and the harness's TS-vs-Rust diff
// degenerates to wasm-vs-binary (drift between the two Rust entry points would
// still surface here). The vector input already carries `value`/`seedDefaults`/
// `createIds`, which the Rust dispatch reifies internally.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('open-in-applications', fn, JSON.stringify(input ?? null))
  )
}
