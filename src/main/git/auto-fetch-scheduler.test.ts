import { describe, expect, it, vi } from 'vitest'
import type { Repo } from '../../shared/types'
import { GitAutoFetchScheduler } from './auto-fetch-scheduler'

function makeRepo(id: string): Repo {
  return {
    id,
    path: `/repos/${id}`,
    displayName: id,
    badgeColor: '#000000',
    addedAt: 0,
    kind: 'git'
  } as Repo
}

type Harness = {
  scheduler: GitAutoFetchScheduler
  fetchRepo: ReturnType<typeof vi.fn>
  onRepoFetched: ReturnType<typeof vi.fn>
  advance: (ms: number) => void
}

function makeHarness(repos: Repo[], intervalMinutes = 5): Harness {
  let nowMs = 1_000_000
  const fetchRepo = vi.fn(async () => {})
  const onRepoFetched = vi.fn()
  const scheduler = new GitAutoFetchScheduler({
    listRepos: () => repos,
    fetchRepo,
    onRepoFetched,
    now: () => nowMs,
    // Why: the timer is driven manually via runDueFetches in tests.
    setIntervalFn: (() => ({ unref: () => {} })) as unknown as typeof setInterval,
    clearIntervalFn: (() => {}) as unknown as typeof clearInterval
  })
  scheduler.configure({ enabled: true, intervalMinutes })
  return { scheduler, fetchRepo, onRepoFetched, advance: (ms) => (nowMs += ms) }
}

describe('GitAutoFetchScheduler', () => {
  it('fetches after the initial delay, then waits a full interval between fetches', async () => {
    const { scheduler, fetchRepo, onRepoFetched, advance } = makeHarness([makeRepo('a')])

    await scheduler.runDueFetches()
    expect(fetchRepo).not.toHaveBeenCalled()

    advance(30_000)
    await scheduler.runDueFetches()
    expect(fetchRepo).toHaveBeenCalledTimes(1)
    expect(onRepoFetched).toHaveBeenCalledTimes(1)

    advance(4 * 60_000)
    await scheduler.runDueFetches()
    expect(fetchRepo).toHaveBeenCalledTimes(1)

    advance(60_000)
    await scheduler.runDueFetches()
    expect(fetchRepo).toHaveBeenCalledTimes(2)
  })

  it('backs off exponentially per repo after failures and recovers on success', async () => {
    const { scheduler, fetchRepo, advance } = makeHarness([makeRepo('a')])
    fetchRepo.mockRejectedValueOnce(new Error('offline'))
    fetchRepo.mockRejectedValueOnce(new Error('offline'))

    await scheduler.runDueFetches() // first sight seeds the initial delay
    advance(30_000)
    await scheduler.runDueFetches() // failure #1 → next in 2x interval
    expect(fetchRepo).toHaveBeenCalledTimes(1)

    advance(5 * 60_000)
    await scheduler.runDueFetches() // 5min < 10min backoff: not eligible
    expect(fetchRepo).toHaveBeenCalledTimes(1)

    advance(5 * 60_000)
    await scheduler.runDueFetches() // failure #2 → next in 4x interval
    expect(fetchRepo).toHaveBeenCalledTimes(2)

    advance(10 * 60_000)
    await scheduler.runDueFetches() // 10min < 20min backoff: not eligible
    expect(fetchRepo).toHaveBeenCalledTimes(2)

    advance(10 * 60_000)
    await scheduler.runDueFetches() // success resets the backoff
    expect(fetchRepo).toHaveBeenCalledTimes(3)

    advance(5 * 60_000)
    await scheduler.runDueFetches()
    expect(fetchRepo).toHaveBeenCalledTimes(4)
  })

  it('caps the failure backoff multiplier', async () => {
    const { scheduler, fetchRepo, advance } = makeHarness([makeRepo('a')])
    fetchRepo.mockRejectedValue(new Error('offline'))

    await scheduler.runDueFetches() // first sight seeds the initial delay
    advance(30_000)
    for (let i = 0; i < 6; i++) {
      await scheduler.runDueFetches()
      advance(8 * 5 * 60_000) // always advance by the capped 8x interval
    }
    // Every retry stays reachable at the 8x cap instead of growing unbounded.
    expect(fetchRepo).toHaveBeenCalledTimes(6)
  })

  it('tracks eligibility independently per repo', async () => {
    const repoA = makeRepo('a')
    const repoB = makeRepo('b')
    const { scheduler, fetchRepo, advance } = makeHarness([repoA, repoB])
    fetchRepo.mockImplementation(async (repo: Repo) => {
      if (repo.id === 'a') {
        throw new Error('offline')
      }
    })

    await scheduler.runDueFetches() // first sight seeds the initial delay
    advance(30_000)
    await scheduler.runDueFetches()
    expect(fetchRepo).toHaveBeenCalledTimes(2)

    advance(5 * 60_000)
    await scheduler.runDueFetches() // b eligible again; a still backing off
    const fetchedIds = fetchRepo.mock.calls.map(([repo]) => (repo as Repo).id)
    expect(fetchedIds).toEqual(['a', 'b', 'b'])
  })

  it('stops fetching when disabled', async () => {
    const { scheduler, fetchRepo, advance } = makeHarness([makeRepo('a')])
    scheduler.configure({ enabled: false, intervalMinutes: 5 })

    advance(60 * 60_000)
    await scheduler.runDueFetches()
    expect(fetchRepo).not.toHaveBeenCalled()
  })
})
