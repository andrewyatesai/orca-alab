// Main-process GitHub PR merge-method normalization, driven by the Rust
// github-pr-merge-methods core via napi (the shared TS impl now holds types +
// label data only). One source of truth with the parity-proven Rust port.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { GitHubPRMergeMethodSettings } from '../shared/types'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'github-pr-merge-methods',
      fn,
      JSON.stringify(input ?? null)
    )
  )
}

export function normalizeGitHubPRMergeMethodSettings(args: {
  defaultMethod: unknown
  mergeCommitAllowed: unknown
  rebaseMergeAllowed: unknown
  squashMergeAllowed: unknown
}): GitHubPRMergeMethodSettings | undefined {
  // Rust emits JSON `null` when no method is allowed; the TS contract is
  // `undefined`, so coerce it back.
  return (
    (dispatch('normalizeGitHubPRMergeMethodSettings', args) as GitHubPRMergeMethodSettings | null) ??
    undefined
  )
}
