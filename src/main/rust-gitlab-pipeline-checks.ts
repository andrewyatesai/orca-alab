// The GitLab pipeline-status mappers now live in the Rust orca-core
// `gitlab_pipeline_checks` module (parity-proven), reached from the main process
// via napi orcaDispatch (the shared TS impl was deleted). Same status →
// (check status, conclusion) split src/main/gitlab/mappers.ts relied on — one
// source of truth with the Rust port.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { PRCheckDetail } from '../shared/types'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('gitlab-pipeline-checks', fn, JSON.stringify(input))
  )
}

// The Rust dispatch reads the status as a bare string (input.as_str()), so the
// input JSON is the status literal, not a keyed object.
export function mapGitLabPipelineJobStatusToCheckStatus(status: string): PRCheckDetail['status'] {
  return dispatch('mapGitLabPipelineJobStatusToCheckStatus', status) as PRCheckDetail['status']
}

// Why: 'manual' splits on allowFailure — a blocking gate is action_required
// (parity with the pipeline-level manual→failure mapping), an optional job is
// neutral — so the conclusion mapper needs both fields.
export function mapGitLabPipelineJobStatusToConclusion(
  status: string,
  allowFailure: boolean
): PRCheckDetail['conclusion'] {
  return dispatch('mapGitLabPipelineJobStatusToConclusion', {
    status,
    allowFailure
  }) as PRCheckDetail['conclusion']
}
