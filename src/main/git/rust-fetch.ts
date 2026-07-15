import type { GitPushTarget } from '../../shared/types'
import { requireRustGitBinding } from '../daemon/rust-git-addon'
import { makeRustGitExecutor, type RunGit } from './rust-git-executor'
import { assertGitPushTargetShapeNative } from './rust-push-target-validation'

// Main-process fetch runs through the verified Rust `git_fetch` via the napi addon —
// the sole path (the addon is required). Rust validates an explicit target, then
// runs `fetch --prune [<remote>]`, normalizing errors internally; `runGit` still
// executes git (SSH/WSL-safe). The shared TS fetch logic stays for the addon-less
// SSH relay + parity oracle — it is NOT a fallback here.

/**
 * Fetch (prune), driven in Rust. The JS-boundary shape guards run here (the typed
 * Rust driver can't produce the "Invalid PR push target …" messages); the caller
 * (gitFetch) normalizes on the way out, exactly as validateGitPushTarget.
 */
export async function gitFetchNative(
  runGit: RunGit,
  pushTarget: GitPushTarget | undefined
): Promise<void> {
  if (pushTarget) {
    assertGitPushTargetShapeNative(pushTarget)
  }
  await requireRustGitBinding().gitFetchViaExecutor(
    pushTarget?.remoteName ?? null,
    pushTarget?.branchName ?? null,
    pushTarget?.remoteUrl ?? null,
    makeRustGitExecutor(runGit)
  )
}
