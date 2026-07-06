import type { RustGitExecutor } from '../daemon/rust-git-addon'

/** A git command runner: executes git and resolves its captured output, or
 *  rejects (non-zero exit or spawn failure) exactly like `gitExecFileAsync`. */
export type RunGit = (args: string[]) => Promise<{ stdout: string; stderr: string }>

/**
 * Adapt a `runGit` (over `gitExecFileAsync`) into the {@link RustGitExecutor} the
 * "A bridge" calls back into — the SSH-safe seam: Rust drives the multi-round
 * logic, but git is still spawned here with all WSL/SSH/env routing intact.
 *
 * `runGit` REJECTS on a non-zero exit; map that back to a RESOLVED result carrying
 * the exit code (default 1) and stderr (falling back to the error message) so the
 * Rust runner classifies it — `BridgeGitOutput` must never reject for a git that
 * spawned and exited. A true spawn failure (a STRING errno like `ENOENT`) is
 * re-thrown so the bridge reports a spawn error, not a git exit.
 */
export function makeRustGitExecutor(runGit: RunGit): RustGitExecutor {
  return async (args) => {
    try {
      const { stdout, stderr } = await runGit(args)
      return { stdout: stdout ?? '', stderr: stderr ?? '', exitCode: 0 }
    } catch (error) {
      const err = error as { code?: unknown; stdout?: unknown; stderr?: unknown; message?: unknown }
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
