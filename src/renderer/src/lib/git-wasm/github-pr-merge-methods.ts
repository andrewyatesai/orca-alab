// Renderer GitHub PR merge-method resolution, driven by the Rust
// github-pr-merge-methods core in the orca-git wasm module (the shared TS impl
// now holds types + label data only). The render-time callsites build the merge
// dropdown from the result, so a wasm-load FAILURE must still yield a populated
// presentation — the fallback offers every method rather than an empty menu.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import {
  GITHUB_PR_MERGE_METHODS,
  GITHUB_PR_MERGE_METHOD_LABELS,
  type GitHubPRMergeMethodPresentation
} from '../../../../shared/github-pr-merge-methods'
import type { GitHubPRMergeMethodSettings } from '../../../../shared/types'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {return null}
  return JSON.parse(orcaDispatch('github-pr-merge-methods', fn, JSON.stringify(input ?? null)))
}

// Mirrors the Rust no-settings result (all methods allowed, squash default) so a
// wasm-load failure still renders a full merge dropdown.
function defaultPresentation(): GitHubPRMergeMethodPresentation {
  const [defaultMethod] = GITHUB_PR_MERGE_METHODS
  return {
    defaultMethod,
    defaultLabel: GITHUB_PR_MERGE_METHOD_LABELS[defaultMethod],
    methods: GITHUB_PR_MERGE_METHODS.map((method) => ({
      method,
      label: GITHUB_PR_MERGE_METHOD_LABELS[method]
    }))
  }
}

export function resolveGitHubPRMergeMethods(
  settings?: GitHubPRMergeMethodSettings | null
): GitHubPRMergeMethodPresentation {
  const r = op('resolveGitHubPRMergeMethods', settings ?? null) as
    | GitHubPRMergeMethodPresentation
    | null
  return r ?? defaultPresentation()
}
