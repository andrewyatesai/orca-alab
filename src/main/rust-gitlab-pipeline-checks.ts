// The GitLab pipeline-status mappers now live in the Rust orca-core
// `gitlab_pipeline_checks` module (parity-proven), reached from the main process
// via napi orcaDispatch (the shared TS impl was deleted). Same status →
// (check status, conclusion) split src/main/gitlab/mappers.ts relied on — one
// source of truth with the Rust port.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { PRCheckDetail } from '../shared/types'

// The Rust dispatch reads the status as a bare string (input.as_str()), so the
// input JSON is the status literal, not a keyed object.
function mapStatus(fn: string, status: string): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('gitlab-pipeline-checks', fn, JSON.stringify(status))
  )
}

export function mapGitLabPipelineJobStatusToCheckStatus(status: string): PRCheckDetail['status'] {
  return mapStatus('mapGitLabPipelineJobStatusToCheckStatus', status) as PRCheckDetail['status']
}

export function mapGitLabPipelineJobStatusToConclusion(
  status: string
): PRCheckDetail['conclusion'] {
  return mapStatus('mapGitLabPipelineJobStatusToConclusion', status) as PRCheckDetail['conclusion']
}
