import type { GitPushTarget, GitUpstreamStatus } from '../../shared/types'
import { isNoUpstreamError, normalizeGitErrorMessage } from '../../shared/git-remote-error'
import { gitExecFileAsync } from './runner'
import { assertGitPushTargetShapeNative } from './rust-push-target-validation'
import { makeRustGitExecutor } from './rust-git-executor'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

type GitExecOptions = {
  wslDistro?: string
}

function gitExecOptions(
  cwd: string,
  options: GitExecOptions = {}
): { cwd: string; wslDistro?: string } {
  return options.wslDistro ? { cwd, wslDistro: options.wslDistro } : { cwd }
}

export async function getUpstreamStatus(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitExecOptions = {}
): Promise<GitUpstreamStatus> {
  // Rust drives the multi-round status (resolve upstream → rev-list → cherry-mark
  // log) and applies the no-upstream swallow + error normalization in-process;
  // runner.ts still executes git so SSH/WSL/env routing is preserved.
  const binding = requireRustGitBinding()
  const executor = makeRustGitExecutor((args) =>
    gitExecFileAsync(args, gitExecOptions(worktreePath, options))
  )
  try {
    if (pushTarget) {
      // The JS-boundary shape guards run here — the typed Rust driver can't produce
      // the "Invalid PR push target …" messages — and are normalized by the outer
      // catch, exactly as validateGitPushTarget's assert.
      assertGitPushTargetShapeNative(pushTarget)
      const json = await binding.getUpstreamStatusViaExecutor(
        pushTarget.remoteName,
        pushTarget.branchName,
        pushTarget.remoteUrl ?? null,
        executor
      )
      return JSON.parse(json) as GitUpstreamStatus
    }
    const json = await binding.getEffectiveUpstreamStatusViaExecutor(executor)
    return JSON.parse(json) as GitUpstreamStatus
  } catch (error) {
    // Why: we only swallow clearly-no-upstream signals — that's an expected
    // state, not a failure. Other errors (auth, corruption, "not a git
    // repository", sparse-checkout) should surface to the user so they can
    // act on them. The shared isNoUpstreamError helper intentionally omits
    // broad phrases like "no such branch" to avoid masking real errors.
    if (isNoUpstreamError(error)) {
      return {
        hasUpstream: false,
        ahead: 0,
        behind: 0
      }
    }
    // Why: parity with gitPush/gitPull/gitFetch — normalize before crossing
    // the IPC boundary so renderers don't see execFile stderr preambles or local paths.
    throw new Error(normalizeGitErrorMessage(error, 'upstream'))
  }
}
