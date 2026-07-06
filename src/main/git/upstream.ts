import type { GitPushTarget, GitUpstreamStatus } from '../../shared/types'
import { isNoUpstreamError, normalizeGitErrorMessage } from '../../shared/git-remote-error'
import { gitExecFileAsync } from './runner'
import { assertGitPushTargetShapeNative } from './rust-push-target-validation'
import { requireRustGitBinding, type RustGitExecutor } from '../daemon/rust-git-addon'

type GitExecOptions = {
  wslDistro?: string
}

/**
 * A {@link RustGitExecutor} over `gitExecFileAsync` — the SSH-safe seam for the
 * "A bridge": Rust drives the multi-round status logic, but git is still spawned
 * here with all WSL/SSH/env routing intact. gitExecFileAsync REJECTS on a
 * non-zero exit; map that back to a RESOLVED result carrying the exit code so the
 * Rust runner classifies it (BridgeGitOutput's resolve-never-reject contract). A
 * spawn failure (non-numeric code, e.g. ENOENT) is re-thrown so the bridge treats
 * it as a spawn error, not a git exit.
 */
function makeRustGitExecutor(worktreePath: string, options: GitExecOptions): RustGitExecutor {
  return async (args) => {
    try {
      const { stdout, stderr } = await gitExecFileAsync(args, gitExecOptions(worktreePath, options))
      // Default to '' — the bridge's BridgeGitOutput requires string stdout/stderr.
      return { stdout: stdout ?? '', stderr: stderr ?? '', exitCode: 0 }
    } catch (error) {
      const err = error as { code?: unknown; stdout?: unknown; stderr?: unknown; message?: unknown }
      // A true spawn failure (git binary missing) carries a STRING errno like
      // 'ENOENT'; re-throw so the bridge reports a spawn error. Anything else is a
      // git process that spawned and exited non-zero — map it to a resolved result
      // carrying the exit code (default 1) and stderr (falling back to the error
      // message) so the Rust runner classifies it (BridgeGitOutput never rejects).
      if (typeof err.code === 'string') {
        throw error
      }
      const stderr =
        typeof err.stderr === 'string'
          ? err.stderr
          : typeof err.message === 'string'
            ? err.message
            : ''
      return {
        stdout: typeof err.stdout === 'string' ? err.stdout : '',
        stderr,
        exitCode: typeof err.code === 'number' ? err.code : 1
      }
    }
  }
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
  const executor = makeRustGitExecutor(worktreePath, options)
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
