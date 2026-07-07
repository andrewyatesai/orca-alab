import type { GitRemoteOperation } from '../../shared/git-remote-error'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

// The main process's git remote-error classification, driven by the Rust
// orca-text core via napi — the same functions the relay runs via wasm
// (src/relay/git-wasm.ts). The shared TS bodies were deleted; only the
// `unknown`→message extraction stays at this JS boundary.

/** Normalise a git remote-operation error into a user-facing message. A
 *  non-Error throw yields the fixed fallback. */
export function normalizeGitErrorMessage(error: unknown, operation?: GitRemoteOperation): string {
  const message = error instanceof Error ? error.message : undefined
  return requireRustGitBinding().normalizeGitErrorMessage(message, operation)
}

/** True only for clearly-no-upstream signals (an expected state). */
export function isNoUpstreamError(error: unknown): boolean {
  return requireRustGitBinding().isNoUpstreamError(
    error instanceof Error ? error.message : undefined
  )
}
