// Renderer-side git remote-error text helpers, driven by the Rust orca-text
// core in the same orca-git wasm module as the line stats (the shared TS
// bodies were deleted; main runs the identical functions via napi, the relay
// via its embedded wasm copy).
import {
  formatSubmodulePushFailureDetail as wasmFormatSubmodulePushFailureDetail,
  stripCredentialsFromMessage as wasmStripCredentialsFromMessage
} from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'

/**
 * Scrub credentials embedded in a git URL within `message`. Returns null while
 * the wasm is still initialising — callers must SKIP the detail rather than
 * show the unscrubbed message (never leak credentials on the not-ready path).
 */
export function stripCredentialsFromMessage(message: string): string | null {
  if (!isGitWasmReady()) {
    return null
  }
  return wasmStripCredentialsFromMessage(message)
}

/** The actionable nested-submodule rejection hidden behind a recursive-push
 *  failure, or null (also null while the wasm is still initialising). */
export function formatSubmodulePushFailureDetail(message: string): string | null {
  if (!isGitWasmReady()) {
    return null
  }
  return wasmFormatSubmodulePushFailureDetail(message) ?? null
}
