// The GitLab pipeline job → check-row mapping now lives in the Rust orca-core
// `gitlab_pipeline_checks` module (parity-proven), driven in the renderer through
// the orca-git wasm (the shared TS impl was deleted). Returns null during the
// ~tens-of-ms wasm boot window; the ChecksPanel caller skips that poll's update
// (the next poll repopulates) rather than clobbering the panel with an empty list.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { GitLabPipelineJob } from '../../../../shared/gitlab-types'
import type { PRCheckDetail } from '../../../../shared/types'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(orcaDispatch('gitlab-pipeline-checks', fn, JSON.stringify(input ?? null)))
}

export function gitLabPipelineJobsToPRChecks(jobs: GitLabPipelineJob[]): PRCheckDetail[] | null {
  return op('gitLabPipelineJobsToPRChecks', jobs) as PRCheckDetail[] | null
}
