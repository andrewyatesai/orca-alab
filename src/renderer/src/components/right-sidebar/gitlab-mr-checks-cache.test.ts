import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitLabMRChecks } from '../../../../shared/types'
import {
  fetchGitLabMRChecks,
  _resetGitLabMRChecksCacheForTest
} from './gitlab-mr-checks-cache'

function checksPayload(overrides: Partial<GitLabMRChecks> = {}): GitLabMRChecks {
  return {
    comments: [],
    pipelineJobs: [
      {
        id: 1,
        name: 'test',
        stage: 'test',
        status: 'success',
        webUrl: 'https://gitlab.com/g/p/-/jobs/1',
        duration: 5
      }
    ],
    headSha: 'sha-1',
    ...overrides
  }
}

const mrChecksMock = vi.fn()

// getActiveRuntimeTarget(null) resolves to { kind: 'local' } → routes through
// window.api.gl.mrChecks, so we assert against that spy directly.
const baseArgs = {
  repoPath: '/repo',
  repoId: 'repo-1',
  settings: null,
  iid: 12,
  headSha: 'sha-1'
} as const

describe('fetchGitLabMRChecks caching + dedup', () => {
  beforeEach(() => {
    _resetGitLabMRChecksCacheForTest()
    mrChecksMock.mockReset()
    mrChecksMock.mockResolvedValue(checksPayload())
    vi.stubGlobal('window', { api: { gl: { mrChecks: mrChecksMock } } })
  })

  it('routes through the lightweight gl.mrChecks path, never gl.workItemDetails', async () => {
    const result = await fetchGitLabMRChecks({ ...baseArgs })
    expect(result?.pipelineJobs).toHaveLength(1)
    expect(mrChecksMock).toHaveBeenCalledWith({
      repoPath: '/repo',
      repoId: 'repo-1',
      iid: 12
    })
  })

  it('de-duplicates concurrent polls for the same MR into one glab fan-out', async () => {
    const [a, b] = await Promise.all([
      fetchGitLabMRChecks({ ...baseArgs }),
      fetchGitLabMRChecks({ ...baseArgs })
    ])
    expect(a).toEqual(b)
    expect(mrChecksMock).toHaveBeenCalledTimes(1)
  })

  it('serves a fresh cached payload on the next poll instead of re-fetching', async () => {
    await fetchGitLabMRChecks({ ...baseArgs })
    await fetchGitLabMRChecks({ ...baseArgs })
    // Second poll inside the TTL window must not spawn another glab call.
    expect(mrChecksMock).toHaveBeenCalledTimes(1)
  })

  it('force bypasses the TTL cache but still returns the payload', async () => {
    await fetchGitLabMRChecks({ ...baseArgs })
    await fetchGitLabMRChecks({ ...baseArgs, force: true })
    expect(mrChecksMock).toHaveBeenCalledTimes(2)
  })

  it('scopes the cache by head SHA so a new push re-fetches', async () => {
    await fetchGitLabMRChecks({ ...baseArgs })
    await fetchGitLabMRChecks({ ...baseArgs, headSha: 'sha-2' })
    expect(mrChecksMock).toHaveBeenCalledTimes(2)
  })

  it('does not cache a null result (transient wasm/boot miss)', async () => {
    mrChecksMock.mockResolvedValueOnce(null)
    const first = await fetchGitLabMRChecks({ ...baseArgs })
    expect(first).toBeNull()
    await fetchGitLabMRChecks({ ...baseArgs })
    expect(mrChecksMock).toHaveBeenCalledTimes(2)
  })
})
