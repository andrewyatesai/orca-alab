import { requireRustGitBinding } from '../daemon/rust-git-addon'

/** True when a git fetch/pull error means the remote ref does not exist (an
 *  expected state, not a failure). The matching runs in the Rust `orca-git`
 *  core (fetch_error_classification.rs — the TS body was deleted); only the
 *  `unknown`→message extraction stays at this JS boundary. */
export function isMissingRemoteRefGitError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return requireRustGitBinding().isMissingRemoteRefGitError(message)
}
