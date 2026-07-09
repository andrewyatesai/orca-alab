// Main-process hosted-review ref normalizers, driven by the Rust
// hosted-review-refs core via napi (the shared TS impl was gutted). One source
// of truth with the parity-proven Rust port.
import { requireRustGitBinding } from './daemon/rust-git-addon'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('hosted-review-refs', fn, JSON.stringify(input ?? null))
  )
}

export function normalizeHostedReviewHeadRef(ref: string): string {
  return dispatch('normalizeHostedReviewHeadRef', ref) as string
}

export function normalizeHostedReviewBaseRef(ref: string): string {
  return dispatch('normalizeHostedReviewBaseRef', ref) as string
}
