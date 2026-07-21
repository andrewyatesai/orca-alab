import { beforeEach, describe, expect, it, vi } from 'vitest'
import { adoptAgentBackgroundSessionTab } from './launch-agent-background-session-tab'

const mockCreateTab = vi.fn()
const mockSetTabCustomTitle = vi.fn()
const mockUpdateTabPtyId = vi.fn()
const mockSetTabLayout = vi.fn()
const mockRegisterAgentLaunchConfig = vi.fn()

// Why: makePaneKey rejects non-UUID leaf ids, mirroring the durable layout leaf.
const LEAF_ID = '11111111-1111-4111-8111-111111111111'

const store = {
  createTab: mockCreateTab,
  setTabCustomTitle: mockSetTabCustomTitle,
  updateTabPtyId: mockUpdateTabPtyId,
  setTabLayout: mockSetTabLayout,
  registerAgentLaunchConfig: mockRegisterAgentLaunchConfig
}

const launchConfig = {
  agentCommand: "claude '--dangerously-skip-permissions'",
  agentArgs: '--dangerously-skip-permissions',
  agentEnv: {}
}

function adopt(overrides?: { title?: string }): ReturnType<typeof adoptAgentBackgroundSessionTab> {
  return adoptAgentBackgroundSessionTab({
    store: store as never,
    worktreeId: 'wt-1',
    reservedTabId: 'reserved-tab',
    leafId: LEAF_ID,
    ptyId: 'pty-1',
    title: overrides?.title,
    agent: 'claude',
    launchToken: 'token-1',
    launchConfig
  })
}

describe('adoptAgentBackgroundSessionTab', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockCreateTab.mockImplementation((_worktreeId, _groupId, _shell, options) => ({
      id: options?.id ?? 'tab-fallback',
      title: 'Terminal 1'
    }))
  })

  it('creates the inactive tab against the reserved id with its PTY already bound', () => {
    const { tab, paneKey } = adopt()

    expect(mockCreateTab).toHaveBeenCalledWith('wt-1', undefined, undefined, {
      id: 'reserved-tab',
      initialPtyId: 'pty-1',
      activate: false,
      recordInteraction: false
    })
    expect(tab.id).toBe('reserved-tab')
    expect(paneKey).toBe(`reserved-tab:${LEAF_ID}`)
    expect(mockUpdateTabPtyId).toHaveBeenCalledWith('reserved-tab', 'pty-1')
    expect(mockRegisterAgentLaunchConfig).toHaveBeenCalledWith(
      `reserved-tab:${LEAF_ID}`,
      launchConfig,
      { agentType: 'claude', launchToken: 'token-1', tabId: 'reserved-tab', leafId: LEAF_ID }
    )
  })

  it('binds the layout to the live PTY without persisting a pane title row', () => {
    adopt({ title: 'Nightly audit' })

    expect(mockSetTabLayout).toHaveBeenCalledWith(
      'reserved-tab',
      expect.objectContaining({
        root: { type: 'leaf', leafId: LEAF_ID },
        activeLeafId: LEAF_ID,
        ptyIdsByLeafId: { [LEAF_ID]: 'pty-1' }
      })
    )
    // Why: `title` labels the tab, not the pane — a pane title renders as an
    // in-terminal title row that background sessions must not persist.
    expect(mockSetTabLayout.mock.calls.at(-1)?.[1]).not.toHaveProperty('titlesByLeafId')
    expect(mockSetTabCustomTitle).toHaveBeenCalledWith('reserved-tab', 'Nightly audit', {
      recordInteraction: false
    })
  })

  it('leaves the tab untitled when the launch has no title', () => {
    adopt()

    expect(mockSetTabCustomTitle).not.toHaveBeenCalled()
  })

  it('keys pane routing off the real tab id when the reserved id collides', () => {
    mockCreateTab.mockReturnValueOnce({ id: 'tab-minted', title: 'Terminal 1' })

    const { tab, paneKey } = adopt()

    // Why: createTab mints a fresh id when the reserved one is taken, so store
    // writes must follow the tab that exists rather than the reservation.
    expect(tab.id).toBe('tab-minted')
    expect(paneKey).toBe(`tab-minted:${LEAF_ID}`)
    expect(mockUpdateTabPtyId).toHaveBeenCalledWith('tab-minted', 'pty-1')
    expect(mockSetTabLayout.mock.calls.at(-1)?.[0]).toBe('tab-minted')
    expect(mockRegisterAgentLaunchConfig.mock.calls.at(-1)?.[0]).toBe(`tab-minted:${LEAF_ID}`)
  })
})
