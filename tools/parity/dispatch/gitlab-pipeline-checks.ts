// TS dispatch for the gitlab-pipeline-checks parity module: maps the shared
// vector function names to the real `src/shared/gitlab-pipeline-checks.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  gitLabPipelineJobsToPRChecks,
  mapGitLabPipelineJobStatusToCheckStatus,
  mapGitLabPipelineJobStatusToConclusion
} from '../../../src/shared/gitlab-pipeline-checks'
import type { GitLabPipelineJob } from '../../../src/shared/gitlab-types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'mapGitLabPipelineJobStatusToCheckStatus':
      return mapGitLabPipelineJobStatusToCheckStatus(input as string)
    case 'mapGitLabPipelineJobStatusToConclusion':
      return mapGitLabPipelineJobStatusToConclusion(input as string)
    case 'gitLabPipelineJobsToPRChecks':
      return gitLabPipelineJobsToPRChecks(input as GitLabPipelineJob[])
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
