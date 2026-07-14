// Renderer repo-icon sanitizer/builders, driven by the Rust repo-icon core in
// the orca-git wasm module (the shared TS impl was gutted to types/data).
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { RepoIcon } from '../../../../shared/repo-icon'

// `sanitizeRepoIcon` is tri-state; Rust encodes JS `undefined` as this sentinel
// string (JSON can't carry undefined), so map it back on the way out.
const SANITIZE_UNDEFINED = '__undefined__'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(orcaDispatch('repo-icon', fn, JSON.stringify(input ?? null)))
}

export function sanitizeRepoIcon(value: unknown): RepoIcon | null | undefined {
  // On a wasm-load failure pass the icon through unchanged so the sync
  // sanitizeRepoUpdate reducer can't silently delete the user's saved icon.
  if (!isGitWasmReady()) {return value as RepoIcon | null | undefined}
  // `undefined` means "leave as-is"; it isn't JSON-representable, so short-circuit.
  if (value === undefined) {return undefined}
  const result = dispatch('sanitizeRepoIcon', value)
  return result === SANITIZE_UNDEFINED ? undefined : (result as RepoIcon | null)
}

export function faviconUrlFromWebsite(rawUrl: string): string | null {
  // The original legitimately returns null, so null is a valid not-ready fallback.
  if (!isGitWasmReady()) {return null}
  return dispatch('faviconUrlFromWebsite', rawUrl) as string | null
}

export function githubAvatarIcon(slug: { owner: string; repo: string }): RepoIcon {
  // Wasm-load-failure fallback: build the same GitHub avatar icon inline
  // (mirrors the Rust core) so the async caller still gets a valid non-null icon.
  if (!isGitWasmReady()) {
    return {
      type: 'image',
      src: `https://github.com/${encodeURIComponent(slug.owner)}.png?size=64`,
      source: 'github',
      label: `${slug.owner}/${slug.repo}`
    }
  }
  return dispatch('githubAvatarIcon', slug) as RepoIcon
}
