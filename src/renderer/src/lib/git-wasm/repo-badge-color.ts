// Renderer repo-badge-color normalizers, driven by the Rust repo-badge-color
// core in the orca-git wasm module (the shared TS impl was gutted to
// types/data). Every consumer feeds the result into a synchronous color prop or
// reducer, so on a wasm-load failure both fns fall back to the default badge
// color (never null) which degrades gracefully; a null would break rendering.
import { DEFAULT_REPO_BADGE_COLOR } from '../../../../shared/constants'
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(orcaDispatch('repo-badge-color', fn, JSON.stringify(input ?? null)))
}

export function normalizeRepoBadgeColor(value: unknown): string | null {
  // Only the wasm-not-ready path defaults; a ready null is a legitimate
  // "invalid color" that sync consumers (e.g. the picker) rely on.
  if (!isGitWasmReady()) {return DEFAULT_REPO_BADGE_COLOR}
  return dispatch('normalizeRepoBadgeColor', { value }) as string | null
}

export function resolveRepoBadgeColor(value: unknown): string {
  if (!isGitWasmReady()) {return DEFAULT_REPO_BADGE_COLOR}
  return dispatch('resolveRepoBadgeColor', { value }) as string
}
