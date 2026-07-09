// Main-process repo-badge-color normalizers, driven by the Rust
// repo-badge-color core via napi (the shared TS impl was gutted to types/data).
// One source of truth with the parity-proven Rust port.
import { requireRustGitBinding } from './daemon/rust-git-addon'

// route to the Rust repo-badge-color core via the orcaDispatch aggregate op
function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('repo-badge-color', fn, JSON.stringify(input ?? null))
  )
}

export function normalizeRepoBadgeColor(value: unknown): string | null {
  return dispatch('normalizeRepoBadgeColor', { value }) as string | null
}

export function resolveRepoBadgeColor(value: unknown): string {
  return dispatch('resolveRepoBadgeColor', { value }) as string
}
