// TS dispatch for the gitlab-projects parity module. The shared TS twin
// (computeNextGitLabRecents) was DELETED (the Rust orca-core gitlab_projects
// core is the sole impl — napi in main, the only process that persists the
// recents list), so this adapter drives the napi binding's aggregate
// orcaDispatch: the vectors' recorded goldens now pin that surface absolutely,
// and the harness's TS-vs-Rust diff degenerates to napi-vs-binary. Requires the
// built addon, like the napi-parity suite.
import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('gitlab-projects', fn, JSON.stringify(input ?? null))
  )
}
