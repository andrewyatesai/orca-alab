import { beforeEach, describe, expect, it, vi } from 'vitest'

type RateLimitGuardResult =
  | { blocked: false }
  | { blocked: true; remaining: number; limit: number; resetAt: number }

const {
  ghExecFileAsyncMock,
  getOwnerRepoMock,
  getIssueOwnerRepoMock,
  getEnterpriseGitHubRepoSlugMock,
  getWorkItemMock,
  getPRChecksMock,
  getPRCommentsMock,
  rateLimitGuardMock,
  noteRateLimitSpendMock,
  ghRepoExecOptionsMock,
  githubRepoContextMock,
  acquireMock,
  releaseMock
} = vi.hoisted(() => ({
  ghExecFileAsyncMock: vi.fn(),
  getOwnerRepoMock: vi.fn(),
  getIssueOwnerRepoMock: vi.fn(),
  getEnterpriseGitHubRepoSlugMock: vi.fn(),
  getWorkItemMock: vi.fn(),
  getPRChecksMock: vi.fn(),
  getPRCommentsMock: vi.fn(),
  rateLimitGuardMock: vi.fn<() => RateLimitGuardResult>(() => ({ blocked: false })),
  noteRateLimitSpendMock: vi.fn(),
  ghRepoExecOptionsMock: vi.fn((context) =>
    context.connectionId
      ? {}
      : { cwd: context.repoPath, ...(context.wslDistro ? { wslDistro: context.wslDistro } : {}) }
  ),
  githubRepoContextMock: vi.fn((repoPath, connectionId, localGitOptions) => ({
    repoPath,
    connectionId: connectionId ?? null,
    ...localGitOptions
  })),
  acquireMock: vi.fn(),
  releaseMock: vi.fn()
}))

vi.mock('./gh-utils', () => ({
  ghExecFileAsync: ghExecFileAsyncMock,
  getOwnerRepo: getOwnerRepoMock,
  getIssueOwnerRepo: getIssueOwnerRepoMock,
  ghRepoExecOptions: ghRepoExecOptionsMock,
  githubRepoContext: githubRepoContextMock,
  acquire: acquireMock,
  release: releaseMock
}))

vi.mock('./client', () => ({
  getWorkItem: getWorkItemMock,
  getPRChecks: getPRChecksMock,
  getPRComments: getPRCommentsMock
}))

vi.mock('./github-enterprise-repository', () => ({
  getEnterpriseGitHubRepoSlug: getEnterpriseGitHubRepoSlugMock
}))

vi.mock('./rate-limit', () => ({
  rateLimitGuard: rateLimitGuardMock,
  noteRateLimitSpend: noteRateLimitSpendMock
}))

import { getPRFileContents, getWorkItemDetails } from './work-item-details'

// Why: split from work-item-details.test.ts (max-lines): PR file listing /
// Enterprise-host pinning scenarios live here; the rest stays there.
describe('getWorkItemDetails', () => {
  beforeEach(() => {
    ghExecFileAsyncMock.mockReset()
    getOwnerRepoMock.mockReset()
    getIssueOwnerRepoMock.mockReset()
    getEnterpriseGitHubRepoSlugMock.mockReset()
    getEnterpriseGitHubRepoSlugMock.mockResolvedValue(null)
    getWorkItemMock.mockReset()
    getPRChecksMock.mockReset()
    getPRCommentsMock.mockReset()
    rateLimitGuardMock.mockReset()
    rateLimitGuardMock.mockReturnValue({ blocked: false })
    noteRateLimitSpendMock.mockReset()
    ghRepoExecOptionsMock.mockClear()
    githubRepoContextMock.mockClear()
    acquireMock.mockReset()
    releaseMock.mockReset()
    acquireMock.mockResolvedValue(undefined)
  })

  // Why: #8935 — GHES remotes resolve via getEnterpriseGitHubRepoSlug and must
  // pin every gh api call with --hostname so diffs do not hit github.com.
  it('uses the GitHub Enterprise host for PR files in work item details', async () => {
    getWorkItemMock.mockResolvedValueOnce({
      id: 'pr:7',
      type: 'pr',
      number: 7,
      title: 'Enterprise PR files',
      state: 'open',
      url: 'https://github.acme-corp.com/team/orca/pull/7',
      labels: [],
      updatedAt: '2026-07-16T00:00:00Z',
      author: 'pr-author'
    })
    getOwnerRepoMock.mockResolvedValue(null)
    getEnterpriseGitHubRepoSlugMock.mockResolvedValue({
      owner: 'team',
      repo: 'orca',
      host: 'github.acme-corp.com'
    })
    getPRCommentsMock.mockResolvedValue([])
    getPRChecksMock.mockResolvedValue([])
    ghExecFileAsyncMock.mockImplementation(async (args: string[]) => {
      const endpoint = args.find((arg) => arg.startsWith('repos/')) ?? ''
      if (endpoint === 'repos/team/orca/pulls/7') {
        return {
          stdout: JSON.stringify({
            body: 'Enterprise PR body',
            head: { sha: 'head-sha' },
            base: { sha: 'base-sha' }
          })
        }
      }
      if (endpoint === 'repos/team/orca/pulls/7/files?per_page=100') {
        return {
          stdout: JSON.stringify([
            {
              filename: 'src/enterprise.ts',
              status: 'modified',
              additions: 2,
              deletions: 1,
              changes: 3,
              patch: '@@ -1 +1 @@'
            }
          ])
        }
      }
      const query = args.find((arg) => arg.startsWith('query=')) ?? ''
      if (query.includes('viewerViewedState')) {
        return {
          stdout: JSON.stringify({
            data: {
              repository: {
                pullRequest: {
                  id: 'PR_enterprise',
                  files: {
                    pageInfo: { hasNextPage: false, endCursor: null },
                    nodes: [{ path: 'src/enterprise.ts', viewerViewedState: 'VIEWED' }]
                  }
                }
              }
            }
          })
        }
      }
      if (query.includes('participants(first: 100)')) {
        return {
          stdout: JSON.stringify({
            data: { repository: { pullRequest: { participants: { nodes: [] } } } }
          })
        }
      }
      throw new Error(`unexpected gh call: ${args.join(' ')}`)
    })

    const details = await getWorkItemDetails('/repo-root', 7, 'pr')

    expect(details?.body).toBe('Enterprise PR body')
    expect(details?.headSha).toBe('head-sha')
    expect(details?.baseSha).toBe('base-sha')
    expect(details?.filesUnavailable).toBe(false)
    expect(details?.files).toEqual([
      {
        path: 'src/enterprise.ts',
        oldPath: undefined,
        status: 'modified',
        additions: 2,
        deletions: 1,
        isBinary: false,
        reviewCommentLineNumbers: [],
        viewerViewedState: 'VIEWED'
      }
    ])
    const apiCalls = ghExecFileAsyncMock.mock.calls
      .map(([args]) => args as string[])
      .filter((args) => args[0] === 'api')
    expect(apiCalls.length).toBeGreaterThan(0)
    expect(apiCalls.every((args) => args.includes('--hostname'))).toBe(true)
    expect(
      apiCalls.every((args) => args[args.indexOf('--hostname') + 1] === 'github.acme-corp.com')
    ).toBe(true)
  })

  it('uses the GitHub Enterprise host when fetching PR file contents with special path chars', async () => {
    getOwnerRepoMock.mockResolvedValue(null)
    getEnterpriseGitHubRepoSlugMock.mockResolvedValue({
      owner: 'team',
      repo: 'orca',
      host: 'github.acme-corp.com'
    })
    ghExecFileAsyncMock.mockImplementation(async (args: string[]) => {
      const endpoint = args.find((arg) => arg.startsWith('repos/')) ?? ''
      if (endpoint === 'repos/team/orca/contents/src/path%23with%3Fchars.ts?ref=base-sha') {
        return { stdout: 'base content' }
      }
      if (endpoint === 'repos/team/orca/contents/src/path%23with%3Fchars.ts?ref=head-sha') {
        return { stdout: 'head content' }
      }
      throw new Error(`unexpected gh call: ${args.join(' ')}`)
    })

    const contents = await getPRFileContents({
      repoPath: '/repo-root',
      prNumber: 7,
      path: 'src/path#with?chars.ts',
      status: 'modified',
      headSha: 'head-sha',
      baseSha: 'base-sha'
    })

    expect(contents).toMatchObject({
      original: 'base content',
      modified: 'head content',
      originalIsBinary: false,
      modifiedIsBinary: false
    })
    const apiCalls = ghExecFileAsyncMock.mock.calls.map(([args]) => args as string[])
    expect(apiCalls).toHaveLength(2)
    expect(apiCalls.every((args) => args.includes('--hostname'))).toBe(true)
    expect(
      apiCalls.every((args) => args[args.indexOf('--hostname') + 1] === 'github.acme-corp.com')
    ).toBe(true)
  })

  // Why: github.com remotes must keep the pre-fix argv shape — no --hostname —
  // so process-level GH_HOST overrides still only apply where gh already did.
  it('does not pin --hostname for github.com PR detail API calls', async () => {
    getWorkItemMock.mockResolvedValueOnce({
      id: 'pr:9',
      type: 'pr',
      number: 9,
      title: 'Dotcom PR',
      state: 'open',
      url: 'https://github.com/acme/widgets/pull/9',
      labels: [],
      updatedAt: '2026-07-16T00:00:00Z',
      author: 'pr-author'
    })
    getOwnerRepoMock.mockResolvedValue({ owner: 'acme', repo: 'widgets' })
    getPRCommentsMock.mockResolvedValue([])
    getPRChecksMock.mockResolvedValue([])
    ghExecFileAsyncMock.mockImplementation(async (args: string[]) => {
      const endpoint = args.find((arg) => arg.startsWith('repos/')) ?? ''
      if (endpoint === 'repos/acme/widgets/pulls/9') {
        return {
          stdout: JSON.stringify({
            body: 'dotcom body',
            head: { sha: 'head-sha' },
            base: { sha: 'base-sha' }
          })
        }
      }
      if (endpoint === 'repos/acme/widgets/pulls/9/files?per_page=100') {
        return { stdout: '[]' }
      }
      return { stdout: JSON.stringify({ data: {} }) }
    })

    const details = await getWorkItemDetails('/repo-root', 9, 'pr')

    expect(details?.filesUnavailable).toBe(false)
    expect(getEnterpriseGitHubRepoSlugMock).not.toHaveBeenCalled()
    const apiCalls = ghExecFileAsyncMock.mock.calls
      .map(([args]) => args as string[])
      .filter((args) => args[0] === 'api')
    expect(apiCalls.every((args) => !args.includes('--hostname'))).toBe(true)
  })

  it('keeps filesUnavailable when neither github.com nor Enterprise owner/repo resolve', async () => {
    getWorkItemMock.mockResolvedValueOnce({
      id: 'pr:11',
      type: 'pr',
      number: 11,
      title: 'Unresolved remote PR',
      state: 'open',
      url: 'https://example.invalid/team/repo/pull/11',
      labels: [],
      updatedAt: '2026-07-16T00:00:00Z',
      author: 'pr-author'
    })
    getOwnerRepoMock.mockResolvedValue(null)
    getEnterpriseGitHubRepoSlugMock.mockResolvedValue(null)
    getPRCommentsMock.mockResolvedValue([])
    getPRChecksMock.mockResolvedValue([])
    ghExecFileAsyncMock.mockResolvedValue({
      stdout: JSON.stringify({ body: 'meta only', headRefOid: 'h', baseRefOid: 'b' })
    })

    const details = await getWorkItemDetails('/repo-root', 11, 'pr')

    expect(details?.filesUnavailable).toBe(true)
    expect(details?.files).toBeUndefined()
  })

  // Why: a rate-limited/auth-failed file fetch must not render as an empty PR;
  // the Files tab keys its retry state off details.filesUnavailable.
  it('flags filesUnavailable when the PR file fetch fails but leaves the PR empty otherwise intact', async () => {
    getWorkItemMock.mockResolvedValueOnce({
      id: 'pr:8305',
      type: 'pr',
      number: 8305,
      title: 'Files fetch fails',
      state: 'open',
      url: 'https://github.com/acme/widgets/pull/8305',
      labels: [],
      updatedAt: '2026-07-11T00:00:00Z',
      author: 'pr-author'
    })
    getOwnerRepoMock.mockResolvedValue({ owner: 'acme', repo: 'widgets' })
    getPRCommentsMock.mockResolvedValue([])
    getPRChecksMock.mockResolvedValue([])
    ghExecFileAsyncMock.mockImplementation(async (args: string[]) => {
      const target = args.at(-1)
      if (target === 'repos/acme/widgets/pulls/8305') {
        return {
          stdout: JSON.stringify({ head: { sha: 'head-sha' }, base: { sha: 'base-sha' } })
        }
      }
      if (target === 'repos/acme/widgets/pulls/8305/files?per_page=100') {
        throw new Error('gh: API rate limit exceeded (403)')
      }
      return { stdout: JSON.stringify({ data: {} }) }
    })

    const details = await getWorkItemDetails('/repo-root', 8305, 'pr')

    expect(details?.filesUnavailable).toBe(true)
    expect(details?.files).toBeUndefined()
  })

  it('treats an empty file list as a genuinely empty PR, not an unavailable one', async () => {
    getWorkItemMock.mockResolvedValueOnce({
      id: 'pr:8306',
      type: 'pr',
      number: 8306,
      title: 'Empty PR',
      state: 'open',
      url: 'https://github.com/acme/widgets/pull/8306',
      labels: [],
      updatedAt: '2026-07-11T00:00:00Z',
      author: 'pr-author'
    })
    getOwnerRepoMock.mockResolvedValue({ owner: 'acme', repo: 'widgets' })
    getPRCommentsMock.mockResolvedValue([])
    getPRChecksMock.mockResolvedValue([])
    ghExecFileAsyncMock.mockImplementation(async (args: string[]) => {
      const target = args.at(-1)
      if (target === 'repos/acme/widgets/pulls/8306') {
        return {
          stdout: JSON.stringify({ head: { sha: 'head-sha' }, base: { sha: 'base-sha' } })
        }
      }
      if (target === 'repos/acme/widgets/pulls/8306/files?per_page=100') {
        return { stdout: '[]' }
      }
      return { stdout: JSON.stringify({ data: {} }) }
    })

    const details = await getWorkItemDetails('/repo-root', 8306, 'pr')

    expect(details?.filesUnavailable).toBe(false)
    expect(details?.files).toEqual([])
  })
})
