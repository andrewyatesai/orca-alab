// The rebase-source result shape shared by both A-bridge drivers of orca-git's
// resolver: the main process (napi, `rust-rebase-source.ts`) and the SSH relay
// (wasm, `git-wasm.ts`). The resolution logic itself lives in Rust
// (`rust/crates/orca-git/src/rebase_source.rs`) — one source of truth — so this
// module is now just the crossing-the-boundary type.
export type GitRemoteRebaseSource = {
  remoteName: string
  branchName: string
  displayName: string
}
