// Main-process repo-icon sanitizer/builders, driven by the Rust repo-icon core
// via napi (the shared TS impl was gutted to types/data).
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { RepoIcon } from '../shared/repo-icon'

// `sanitizeRepoIcon` is tri-state; the Rust side encodes JS `undefined` as this
// sentinel string (JSON can't carry undefined), so map it back on the way out.
const SANITIZE_UNDEFINED = '__undefined__'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('repo-icon', fn, JSON.stringify(input ?? null))
  )
}

export function sanitizeRepoIcon(value: unknown): RepoIcon | null | undefined {
  // `undefined` means "leave as-is"; it isn't JSON-representable, so short-
  // circuit rather than let it collapse to null (which Rust reads as reset).
  if (value === undefined) return undefined
  const result = dispatch('sanitizeRepoIcon', value)
  return result === SANITIZE_UNDEFINED ? undefined : (result as RepoIcon | null)
}

export function faviconUrlFromWebsite(rawUrl: string): string | null {
  return dispatch('faviconUrlFromWebsite', rawUrl) as string | null
}

export function githubAvatarIcon(slug: { owner: string; repo: string }): RepoIcon {
  return dispatch('githubAvatarIcon', slug) as RepoIcon
}
