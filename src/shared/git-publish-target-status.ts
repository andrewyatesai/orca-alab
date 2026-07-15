import type { GitPushTarget } from './types'

// The publish-target STATUS resolution now lives in Rust
// (`rust/crates/orca-git/src/publish_target_status.rs`), driven by both A-bridges
// (main via napi, relay via wasm) — one source of truth. What remains here is the
// pure `remote/branch` display-name formatter the renderer still uses to compare
// a resolved upstream name against a configured push target.
export function getPublishTargetDisplayName(target: GitPushTarget): string {
  return `${target.remoteName}/${target.branchName}`
}
