// TS dispatch for the agent-kind parity module. The shared TS maps were DELETED
// (the Rust `orca_core::agent_kind` core is the sole impl — the renderer drives
// it via wasm), so this adapter drives that same wasm: the vectors' recorded
// goldens now pin that surface absolutely, and the harness's TS-vs-Rust diff
// degenerates to wasm-vs-binary (drift between the two Rust entry points would
// still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(gitWasmOracle().orcaDispatch('agent-kind', fn, JSON.stringify(input ?? null)))
}
