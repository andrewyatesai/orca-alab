import { beforeEach, describe, expect, it, vi } from 'vitest'
import { runDeepLinkCommandInNewTab } from './deep-link-run-command'
import { activateAndRevealWorktree } from './worktree-activation'

type MockStoreState = {
  createTab: ReturnType<typeof vi.fn>
  queueTabStartupCommand: ReturnType<typeof vi.fn>
  setTabCustomTitle: ReturnType<typeof vi.fn>
  setActiveTabType: ReturnType<typeof vi.fn>
  setTabBarOrder: ReturnType<typeof vi.fn>
  tabsByWorktree: Record<string, { id: string }[]>
  openFiles: { id: string; worktreeId: string }[]
  browserTabsByWorktree: Record<string, { id: string }[]>
  tabBarOrderByWorktree: Record<string, string[]>
}

let mockState: MockStoreState

vi.mock('@/store', () => ({
  useAppStore: { getState: () => mockState }
}))

vi.mock('@/lib/worktree-activation', () => ({
  activateAndRevealWorktree: vi.fn(() => ({ worktree: { id: 'repo::wt' } }))
}))

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(activateAndRevealWorktree).mockReturnValue(
    { worktree: { id: 'repo::wt' } } as unknown as ReturnType<typeof activateAndRevealWorktree>
  )
  mockState = {
    createTab: vi.fn(() => ({ id: 'tab-new' })),
    queueTabStartupCommand: vi.fn(),
    setTabCustomTitle: vi.fn(),
    setActiveTabType: vi.fn(),
    setTabBarOrder: vi.fn(),
    tabsByWorktree: { 'repo::wt': [{ id: 'tab-existing' }, { id: 'tab-new' }] },
    openFiles: [],
    browserTabsByWorktree: {},
    tabBarOrderByWorktree: {}
  }
})

describe('runDeepLinkCommandInNewTab', () => {
  it('spawns a fresh tab and queues the command as its startup command', () => {
    const result = runDeepLinkCommandInNewTab({ worktreeId: 'repo::wt', command: 'npm test' })

    expect(result).toEqual({ tabId: 'tab-new' })
    expect(activateAndRevealWorktree).toHaveBeenCalledWith('repo::wt')
    expect(mockState.createTab).toHaveBeenCalledWith('repo::wt')
    // Why asserted: the command must go through the startup queue (a NEW pty), never an existing pane's stdin (§6.1).
    expect(mockState.queueTabStartupCommand).toHaveBeenCalledWith('tab-new', {
      command: 'npm test'
    })
    expect(mockState.setActiveTabType).toHaveBeenCalledWith('terminal')
  })

  it('applies the optional title to the new tab', () => {
    runDeepLinkCommandInNewTab({ worktreeId: 'repo::wt', command: 'ls', title: ' Build ' })

    expect(mockState.setTabCustomTitle).toHaveBeenCalledWith('tab-new', 'Build')
  })

  it('appends the new tab to the persisted tab-bar order', () => {
    mockState.tabBarOrderByWorktree = { 'repo::wt': ['tab-existing'] }

    runDeepLinkCommandInNewTab({ worktreeId: 'repo::wt', command: 'ls' })

    expect(mockState.setTabBarOrder).toHaveBeenCalledWith('repo::wt', ['tab-existing', 'tab-new'])
  })

  it('refuses when the worktree is unknown (deleted since consent opened)', () => {
    vi.mocked(activateAndRevealWorktree).mockReturnValue(false)

    expect(runDeepLinkCommandInNewTab({ worktreeId: 'repo::gone', command: 'ls' })).toBeNull()
    expect(mockState.createTab).not.toHaveBeenCalled()
  })

  it('refuses whitespace-only commands', () => {
    expect(runDeepLinkCommandInNewTab({ worktreeId: 'repo::wt', command: '  \n ' })).toBeNull()
    expect(activateAndRevealWorktree).not.toHaveBeenCalled()
    expect(mockState.createTab).not.toHaveBeenCalled()
  })
})
