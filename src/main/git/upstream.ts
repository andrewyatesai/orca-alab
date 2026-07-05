import type { GitPushTarget, GitUpstreamStatus } from '../../shared/types'
import { upstreamOnlyCommitsArePatchEquivalent } from '../../shared/git-upstream-status'
import { isNoUpstreamError, normalizeGitErrorMessage } from '../../shared/git-remote-error'
import { getEffectiveGitUpstreamStatus } from '../../shared/git-effective-upstream'
import { getPublishTargetStatus } from '../../shared/git-publish-target-status'
import { gitExecFileAsync } from './runner'
import { validateGitPushTarget } from './push-target-validation'
import { assertGitPushTargetShapePreferRust } from './rust-push-target-validation'
import { loadRustGitBinding, type RustGitExecutor } from '../daemon/rust-git-addon'

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
      return { stdout, stderr, exitCode: 0 }
    } catch (error) {
      const err = error as { code?: unknown; stdout?: unknown; stderr?: unknown }
      if (typeof err.code !== 'number') {
        throw error
      }
      return {
        stdout: typeof err.stdout === 'string' ? err.stdout : '',
        stderr: typeof err.stderr === 'string' ? err.stderr : '',
        exitCode: err.code
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

async function getBehindCommitsArePatchEquivalent(
  worktreePath: string,
  upstreamName: string,
  options: GitExecOptions = {}
): Promise<boolean> {
  try {
    const { stdout } = await gitExecFileAsync(
      ['log', '--oneline', '--cherry-mark', '--right-only', `HEAD...${upstreamName}`, '--'],
      gitExecOptions(worktreePath, options)
    )
    return upstreamOnlyCommitsArePatchEquivalent(stdout)
  } catch {
    // Why: patch-equivalence is an optimization for the rebase case. If the
    // probe fails, keep the conservative pull-first behavior.
    return false
  }
}

export async function getUpstreamStatus(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitExecOptions = {}
): Promise<GitUpstreamStatus> {
  try {
    if (pushTarget) {
      const binding = loadRustGitBinding()
      if (binding?.getUpstreamStatusViaExecutor) {
        // Rust drives the multi-round status (validate → rev-parse → rev-list →
        // log) and applies the no-upstream swallow + error normalization
        // in-process; runner.ts still executes git so SSH/WSL/env routing is
        // preserved. The JS-boundary shape guards run here — the typed Rust driver
        // can't produce the "Invalid PR push target …" messages — and are
        // normalized by the outer catch, exactly as validateGitPushTarget's assert.
        assertGitPushTargetShapePreferRust(pushTarget)
        const json = await binding.getUpstreamStatusViaExecutor(
          pushTarget.remoteName,
          pushTarget.branchName,
          pushTarget.remoteUrl ?? null,
          makeRustGitExecutor(worktreePath, options)
        )
        return JSON.parse(json) as GitUpstreamStatus
      }
      const target = await validateGitPushTarget(worktreePath, pushTarget, options)
      return await getPublishTargetStatus(
        (args) => gitExecFileAsync(args, gitExecOptions(worktreePath, options)),
        target,
        (upstreamName) => getBehindCommitsArePatchEquivalent(worktreePath, upstreamName, options)
      )
    }
    return await getEffectiveGitUpstreamStatus(
      (args) => gitExecFileAsync(args, gitExecOptions(worktreePath, options)),
      (upstreamName) => getBehindCommitsArePatchEquivalent(worktreePath, upstreamName, options)
    )
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
