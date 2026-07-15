import { requireRustGitBinding } from '../daemon/rust-git-addon'
import { makeRustGitExecutor, type RunGit } from './rust-git-executor'

// Main-process rebase-from-base runs through the verified Rust
// `git_pull_rebase_from_base` via the napi addon — the sole path (the addon is
// required). Rust resolves the base's remote/branch (read-only: git remote +
// check-ref-format) AND runs the mutating `pull --rebase` in one call; `runGit`
// still executes git (SSH/WSL-safe). The shared TS logic stays for the addon-less
// SSH relay + parity oracle — it is NOT a fallback here.

/**
 * Pull-rebase the current branch onto a base ref, driven in Rust. Rejects with
 * the error already normalized as 'pull' (the raw "Choose a remote base branch to
 * rebase from." resolver message tails identically); the caller
 * (gitPullRebaseFromBase) re-normalizes on the way out, which is idempotent.
 */
export async function gitPullRebaseFromBaseNative(runGit: RunGit, baseRef: string): Promise<void> {
  await requireRustGitBinding().gitPullRebaseFromBaseViaExecutor(
    baseRef,
    makeRustGitExecutor(runGit)
  )
}
