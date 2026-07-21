import type { GitRemoteOperation } from '../../shared/git-remote-error'
import { formatGitRemoteOperationTimeoutMessage } from '../../shared/git-remote-error'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

// The main process's git remote-error classification, driven by the Rust
// orca-text core via napi — the same functions the relay runs via wasm
// (src/relay/git-wasm.ts). The shared TS bodies were deleted; only the
// `unknown`→message extraction stays at this JS boundary.

/** Normalise a git remote-operation error into a user-facing message. A
 *  non-Error throw yields the fixed fallback. */
export function normalizeGitErrorMessage(error: unknown, operation?: GitRemoteOperation): string {
  // Why: the runner's timeout text is a runner artifact, so it is matched at
  // this boundary rather than inside the Rust core (see shared helper).
  const timeoutMessage = formatGitRemoteOperationTimeoutMessage(error, operation)
  if (timeoutMessage !== null) {
    return timeoutMessage
  }
  const message = error instanceof Error ? error.message : undefined
  return requireRustGitBinding().normalizeGitErrorMessage(message, operation)
}

/** True only for clearly-no-upstream signals (an expected state). */
export function isNoUpstreamError(error: unknown): boolean {
  return requireRustGitBinding().isNoUpstreamError(
    error instanceof Error ? error.message : undefined
  )
}

/** Scrub credentials embedded in a git URL within `message` — the same Rust
 *  core the relay runs via wasm. */
export function stripCredentialsFromMessage(message: string): string {
  return requireRustGitBinding().stripCredentialsFromMessage(message)
}
