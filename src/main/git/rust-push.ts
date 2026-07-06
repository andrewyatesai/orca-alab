import type { GitPushTarget } from '../../shared/types'
import { requireRustGitBinding } from '../daemon/rust-git-addon'
import { makeRustGitExecutor, type RunGit } from './rust-git-executor'
import { assertGitPushTargetShapeNative } from './rust-push-target-validation'

// Main-process push runs through the verified Rust `git_push` via the napi addon —
// the sole path (the addon is required). Rust validates, resolves the refspec
// (pushRemote/pushDefault/branch.remote, URL-resolved, with the merge-base and
// fork guards), and runs `git push [--force-with-lease] --set-upstream …`,
// normalizing errors internally; `runGit` still executes git (SSH/WSL-safe). The
// shared TS push logic stays for the addon-less SSH relay + parity oracle — it is
// NOT a fallback here.

/**
 * Push the current branch, driven in Rust. The JS-boundary shape guards run here
 * (the typed Rust driver can't produce the "Invalid PR push target …" messages);
 * the caller (gitPush) normalizes on the way out, exactly as validateGitPushTarget.
 */
export async function gitPushNative(
  runGit: RunGit,
  pushTarget: GitPushTarget | undefined,
  forceWithLease: boolean
): Promise<void> {
  if (pushTarget) {
    assertGitPushTargetShapeNative(pushTarget)
  }
  await requireRustGitBinding().gitPushViaExecutor(
    pushTarget?.remoteName ?? null,
    pushTarget?.branchName ?? null,
    pushTarget?.remoteUrl ?? null,
    forceWithLease,
    makeRustGitExecutor(runGit)
  )
}
