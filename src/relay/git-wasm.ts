// The relay's git-output parsing, driven by the orca-git Rust core compiled to
// wasm (rust/orca-git-wasm) instead of a hand-maintained TS reimplementation.
//
// The relay runs on the remote host as pure JS with NO native addon, so it used
// to re-implement git parsing in TS — code that could (and did) drift from the
// Rust port the main process runs via napi. These wrappers call the SAME pure
// orca-git/orca-text functions through wasm, so relay output is byte-identical to
// the main process. The wasm bytes are embedded (base64) so the relay stays a
// single self-contained bundle; initSync is idempotent and lazy so callers and
// tests need no explicit setup.
import {
  initSync,
  normalizeGitErrorMessage as wasmNormalizeGitErrorMessage,
  isNoUpstreamError as wasmIsNoUpstreamError,
  parseStatusPorcelain as wasmParseStatusPorcelain
} from './wasm/orca_git_wasm.js'
import { ORCA_GIT_WASM_BASE64 } from './wasm/orca_git_wasm_bg.wasm.base64'
import type { GitRemoteOperation } from '../shared/git-remote-error'
import type { GitStatusEntry } from '../shared/types'

let inited = false
function ensureGitWasm(): void {
  if (inited) {
    return
  }
  // Buffer is a Uint8Array (BufferSource), which initSync accepts. Node/relay
  // only — the relay never runs in a browser.
  initSync({ module: Buffer.from(ORCA_GIT_WASM_BASE64, 'base64') })
  inited = true
}

/**
 * Normalise a git remote-operation error into a user-facing message. Same
 * signature as the shared TS `normalizeGitErrorMessage`; the `error.message`
 * extraction happens at this JS boundary (the wasm fn takes the message string),
 * mirroring the parity dispatch. A non-Error throw yields the fixed fallback.
 */
export function normalizeGitErrorMessage(error: unknown, operation?: GitRemoteOperation): string {
  ensureGitWasm()
  const message = error instanceof Error ? error.message : undefined
  return wasmNormalizeGitErrorMessage(message, operation)
}

/** True only for clearly-no-upstream signals (an expected state). */
export function isNoUpstreamError(error: unknown): boolean {
  ensureGitWasm()
  return wasmIsNoUpstreamError(error instanceof Error ? error.message : undefined)
}

/**
 * Parse `git status --porcelain=v2 --branch` output into the relay's structured
 * shape. Passes limit 0 (cap disabled) so the parser returns ALL entries, exactly
 * like the old relay-local parser — the caller (git-handler-status-ops) applies
 * DEFAULT_GIT_STATUS_LIMIT itself and relies on the full count to detect the
 * over-limit state. Adapts the wasm's flat upstream fields into the nested
 * `upstreamStatus` the relay consumers expect.
 */
export function parseStatusOutput(stdout: string): {
  entries: GitStatusEntry[]
  unmergedLines: string[]
  ignoredPaths: string[]
  head?: string
  branch?: string
  upstreamStatus: {
    hasUpstream: boolean
    upstreamName?: string
    ahead: number
    behind: number
  }
} {
  ensureGitWasm()
  const r = JSON.parse(wasmParseStatusPorcelain(Buffer.from(stdout, 'utf8'), 0)) as {
    entries: GitStatusEntry[]
    unmergedLines: string[]
    ignoredPaths: string[]
    head?: string
    branch?: string
    upstreamName?: string
    ahead?: number
    behind?: number
  }
  return {
    entries: r.entries,
    unmergedLines: r.unmergedLines,
    ignoredPaths: r.ignoredPaths,
    head: r.head,
    branch: r.branch,
    upstreamStatus:
      r.upstreamName !== undefined
        ? {
            hasUpstream: true,
            upstreamName: r.upstreamName,
            ahead: r.ahead ?? 0,
            behind: r.behind ?? 0
          }
        : { hasUpstream: false, ahead: 0, behind: 0 }
  }
}
