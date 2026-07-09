// TS dispatch for the task-query parity module. The shared TS impl was DELETED
// (the Rust orca-core task_query port is the sole impl — napi in main, wasm in
// the renderer), so this adapter drives the SAME wasm: the vectors' recorded
// goldens now pin that surface absolutely, and the harness's TS-vs-Rust diff
// degenerates to wasm-vs-binary (drift between the two Rust entry points would
// still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(gitWasmOracle().orcaDispatch('task-query', fn, JSON.stringify(input ?? null)))
}
