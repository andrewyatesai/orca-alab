import { beforeEach, describe, expect, it, vi } from 'vitest'
import { attachAutoCloseAfterMergeController } from './auto-close-after-merge-controller'
import type { AppState } from '@/store'

const mocks = vi.hoisted(() => ({
  runWorktreeDeleteWithToast: vi.fn().mockResolvedValue(true)
}))

vi.mock('@/store', () => ({
  useAppStore: { getState: () => ({}), subscribe: () => () => {} }
}))

vi.mock('./delete-worktree-flow', () => ({
  runWorktreeDeleteWithToast: mocks.runWorktreeDeleteWithToast
}))

type FakeState = Pick<
  AppState,
  'settings' | 'worktreesByRepo' | 'repos' | 'prCache' | 'hostedReviewCache'
>

type FakeListener = (state: AppState, prevState: AppState) => void

function createFakeStore(initial: FakeState): {
  getState: () => AppState
  subscribe: (listener: FakeListener) => () => void
  setState: (updates: Partial<FakeState>) => void
} {
  let state = initial
  const listeners = new Set<FakeListener>()
  return {
    getState: () => state as AppState,
    subscribe: (listener: FakeListener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    setState: (updates: Partial<FakeState>) => {
      const prev = state
      state = { ...state, ...updates }
      for (const listener of listeners) {
        listener(state as AppState, prev as AppState)
      }
    }
  }
}

function makeWorktree(overrides: Partial<Record<string, unknown>> = {}): Record<string, unknown> {
  return {
    id: 'repo-1::/tmp/wt-feat',
    repoId: 'repo-1',
    branch: 'refs/heads/feat',
    displayName: 'feat',
    isMainWorktree: false,
    isBare: false,
    ...overrides
  }
}

function makeState(overrides: Partial<FakeState> = {}): FakeState {
  return {
    settings: { autoCloseAfterMerge: true } as FakeState['settings'],
    repos: [{ id: 'repo-1', path: '/tmp/repo' }] as FakeState['repos'],
    worktreesByRepo: { 'repo-1': [makeWorktree()] } as unknown as FakeState['worktreesByRepo'],
    prCache: {},
    hostedReviewCache: {},
    ...overrides
  }
}

// Local repo (no connectionId/executionHostId): PR key `${repoId}::${branch}`,
// hosted review key `local::${repoId}::${branch}`.
const PR_KEY = 'repo-1::feat'
const HOSTED_KEY = 'local::repo-1::feat'

describe('attachAutoCloseAfterMergeController', () => {
  beforeEach(() => {
    mocks.runWorktreeDeleteWithToast.mockClear()
  })

  it('deletes a worktree when its PR merges live in-session', () => {
    const store = createFakeStore(makeState())
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledTimes(1)
    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledWith('repo-1::/tmp/wt-feat', 'feat')
    detach()
  })

  it('does not re-fire for the same worktree on later state changes', () => {
    const store = createFakeStore(makeState())
    const detach = attachAutoCloseAfterMergeController(store)

    const prCache = { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    store.setState({ prCache } as unknown as Partial<FakeState>)
    store.setState({ prCache: { ...prCache } } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledTimes(1)
    detach()
  })

  it('never deletes for merges hydrated from the persisted cache, even after a live refresh', () => {
    const staleFetchedAt = Date.now() - 60_000
    const store = createFakeStore(
      makeState({
        prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: staleFetchedAt } }
      } as unknown as FakeState)
    )
    const detach = attachAutoCloseAfterMergeController(store)
    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()

    // A live refresh re-observes the same merged PR with a fresh timestamp.
    store.setState({
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()
    detach()
  })

  it('does not sweep merges observed while the setting was off after a later toggle-on', () => {
    const store = createFakeStore(
      makeState({ settings: { autoCloseAfterMerge: false } as FakeState['settings'] })
    )
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)
    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()

    store.setState({ settings: { autoCloseAfterMerge: true } as FakeState['settings'] })
    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()
    detach()
  })

  it('never deletes main or bare worktrees', () => {
    const store = createFakeStore(
      makeState({
        worktreesByRepo: {
          'repo-1': [
            makeWorktree({ id: 'repo-1::/tmp/repo', isMainWorktree: true, branch: 'feat' })
          ]
        } as unknown as FakeState['worktreesByRepo']
      })
    )
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()
    detach()
  })

  it('honors merged reviews from non-GitHub providers via hostedReviewCache', () => {
    const store = createFakeStore(makeState())
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      hostedReviewCache: {
        [HOSTED_KEY]: {
          data: { provider: 'gitlab', state: 'merged' },
          fetchedAt: Date.now() + 1000
        }
      }
    } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledTimes(1)
    detach()
  })

  it('prefers the provider-generic hosted review over a stale GitHub prCache mirror', () => {
    // hostedReviewCache says open; a leftover prCache mirror says merged.
    const store = createFakeStore(makeState())
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      hostedReviewCache: {
        [HOSTED_KEY]: { data: { provider: 'gitlab', state: 'open' }, fetchedAt: Date.now() + 1000 }
      },
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()
    detach()
  })

  it('scopes cache lookups to the worktree execution host', () => {
    // Same repo id on an SSH host: local merged entries must not close it.
    const store = createFakeStore(
      makeState({
        repos: [
          { id: 'repo-1', path: '/tmp/repo' },
          { id: 'repo-1', path: '/remote/repo', executionHostId: 'ssh:box' }
        ] as FakeState['repos'],
        worktreesByRepo: {
          'repo-1': [makeWorktree({ id: 'repo-1::/remote/wt-feat', hostId: 'ssh:box' })]
        } as unknown as FakeState['worktreesByRepo']
      })
    )
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)
    expect(mocks.runWorktreeDeleteWithToast).not.toHaveBeenCalled()

    store.setState({
      prCache: {
        [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 },
        ['ssh:box::repo-1::feat']: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 }
      }
    } as unknown as Partial<FakeState>)
    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledTimes(1)
    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledWith('repo-1::/remote/wt-feat', 'feat')
    detach()
  })

  it('allows a recreated worktree at the same path to auto-close again', () => {
    const store = createFakeStore(makeState())
    const detach = attachAutoCloseAfterMergeController(store)

    store.setState({
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 1000 } }
    } as unknown as Partial<FakeState>)
    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledTimes(1)

    // Worktree removed → handled entry evicted; recreate at the same path with
    // a fresh merged observation for its next PR.
    store.setState({
      worktreesByRepo: {} as unknown as FakeState['worktreesByRepo'],
      prCache: {}
    } as unknown as Partial<FakeState>)
    store.setState({
      worktreesByRepo: { 'repo-1': [makeWorktree()] } as unknown as FakeState['worktreesByRepo'],
      prCache: { [PR_KEY]: { data: { state: 'merged' }, fetchedAt: Date.now() + 2000 } }
    } as unknown as Partial<FakeState>)

    expect(mocks.runWorktreeDeleteWithToast).toHaveBeenCalledTimes(2)
    detach()
  })
})
