// TS dispatch for the gitlab-pipeline-checks parity module. The shared TS impl
// was DELETED (the Rust orca-core `gitlab_pipeline_checks` port is the sole
// implementation — main drives the status mappers via napi, the renderer drives
// the job → check-row mapping via the orca-git wasm), so this adapter drives that
// same wasm: the vectors' recorded goldens now pin the production surface, and
// the harness's TS-vs-Rust diff degenerates to wasm-vs-binary (drift between the
// two Rust entry points would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('gitlab-pipeline-checks', fn, JSON.stringify(input ?? null))
  )
}
