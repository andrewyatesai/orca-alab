// Main-process GitLab "recent projects" computation, driven by the Rust
// orca-core gitlab_projects core via the aggregate napi orcaDispatch (the shared
// TS impl was deleted). One source of truth with the parity-proven Rust port —
// same most-recent-first / dedupe / cap behavior the IPC handler relied on.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { GitLabProjectSettings } from '../shared/types'

// Mirrors orca-core::gitlab_projects::GITLAB_RECENTS_MAX (the authoritative cap):
// the dispatch reads an explicit `max`, so the value has to cross the boundary.
const GITLAB_RECENTS_MAX = 10

export function computeNextGitLabRecents(
  existing: GitLabProjectSettings['recent'],
  host: string,
  path: string,
  now: Date = new Date(),
  max: number = GITLAB_RECENTS_MAX
): GitLabProjectSettings['recent'] {
  // The dispatch rehydrates a Date from an ISO string — pass nowIso, not a Date.
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'gitlab-projects',
      'computeNextGitLabRecents',
      JSON.stringify({ existing, host, path, nowIso: now.toISOString(), max })
    )
  ) as GitLabProjectSettings['recent']
}
