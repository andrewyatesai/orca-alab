import { requireRustGitBinding } from '../daemon/rust-git-addon'

/** True when a git fetch/pull error means the remote ref does not exist (an
 *  expected state, not a failure). The matching runs in the Rust `orca-git`
 *  core (fetch_error_classification.rs — the TS body was deleted); only the
 *  `unknown`→message extraction stays at this JS boundary. */
export function isMissingRemoteRefGitError(error: unknown): boolean {
  // Why: execFile rejections carry `Command failed: git fetch …` in `.message`
  // while git's real `fatal: couldn't find remote ref …` diagnostic lives in
  // `.stderr`; feed both to the Rust matcher or multi-remote PR resolution
  // treats a missing ref as a hard failure and never walks the next remote.
  const message = error instanceof Error ? error.message : String(error)
  const stderr =
    error && typeof error === 'object' && typeof (error as { stderr?: unknown }).stderr === 'string'
      ? (error as { stderr: string }).stderr
      : ''
  return requireRustGitBinding().isMissingRemoteRefGitError(`${message}\n${stderr}`)
}
