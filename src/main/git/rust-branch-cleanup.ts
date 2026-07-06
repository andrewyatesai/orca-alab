import { requireRustGitBinding } from '../daemon/rust-git-addon'
import { makeRustGitExecutor, type RunGit } from './rust-git-executor'

// Main-process branch-cleanup safe-to-delete DECISION runs through the verified
// Rust logic via the napi addon — the sole path (the addon is required). Rust
// gathers base refs, refreshes remotes (non-fatal fetch), and checks tree-equal
// merge / patch-equivalence / squash match; `runGit` still executes git (SSH/WSL-safe),
// including piping stdin for `git patch-id --stable`. The destructive `git branch -d`
// stays in TS, gated on the returned boolean. The shared TS decision stays for the
// addon-less SSH relay + parity oracle — it is NOT a fallback here.

/** True when the branch has no unmerged changes against any candidate base — i.e.
 *  safe to delete. Driven in Rust; only ever moves toward *preserve*. */
export async function branchIsSafeToDeleteNative(
  runGit: RunGit,
  branchName: string
): Promise<boolean> {
  return requireRustGitBinding().branchIsSafeToDeleteViaExecutor(
    branchName,
    makeRustGitExecutor(runGit)
  )
}
