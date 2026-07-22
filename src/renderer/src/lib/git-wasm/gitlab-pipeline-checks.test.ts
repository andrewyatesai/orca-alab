import { describe, expect, it } from 'vitest'
import './init-git-wasm-for-test'
import { gitLabPipelineJobsToPRChecks } from './gitlab-pipeline-checks'
import type { GitLabPipelineJob } from '../../../../shared/gitlab-types'

function job(overrides: Partial<GitLabPipelineJob>): GitLabPipelineJob {
  return {
    id: 1,
    name: 'unit',
    stage: 'test',
    status: 'success',
    webUrl: 'https://gitlab.com/g/p/-/jobs/1',
    duration: null,
    ...overrides
  }
}

describe('gitLabPipelineJobsToPRChecks (orca-git wasm)', () => {
  it('maps a blocking manual gate to action_required so the Checks panel counts it as failing', () => {
    // Pins parity with the pipeline-level manual/blocked→failure mapping in
    // src/main/gitlab/mappers.ts: the MR badge and the job rows must agree.
    const checks = gitLabPipelineJobsToPRChecks([
      job({ id: 2, name: 'release gate', stage: 'deploy', status: 'manual', allowFailure: false })
    ])
    expect(checks?.[0]).toMatchObject({
      name: 'deploy: release gate',
      status: 'completed',
      conclusion: 'action_required'
    })
  })

  it('keeps an optional (allow_failure) manual job neutral — it never blocks the pipeline', () => {
    const checks = gitLabPipelineJobsToPRChecks([
      job({ id: 3, name: 'optional deploy', stage: 'deploy', status: 'manual', allowFailure: true })
    ])
    expect(checks?.[0]).toMatchObject({ conclusion: 'neutral' })
  })

  it('fails closed to action_required when allowFailure is absent from a manual job', () => {
    const checks = gitLabPipelineJobsToPRChecks([
      job({ id: 4, name: 'legacy manual', stage: 'deploy', status: 'manual' })
    ])
    expect(checks?.[0]).toMatchObject({ conclusion: 'action_required' })
  })

  it('leaves non-manual statuses unchanged', () => {
    const checks = gitLabPipelineJobsToPRChecks([
      job({ id: 5, status: 'failed' }),
      job({ id: 6, status: 'running' })
    ])
    expect(checks?.map((c) => c.conclusion)).toEqual(['failure', 'pending'])
  })
})
