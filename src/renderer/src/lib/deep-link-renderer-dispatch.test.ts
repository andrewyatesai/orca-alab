import { beforeEach, describe, expect, it, vi } from 'vitest'
import { dispatchDeepLinkInRenderer, handleDeepLinkUiEvent } from './deep-link-renderer-dispatch'
import {
  getPendingRunCommandConsent,
  requestRunCommandConsent,
  resetRunCommandConsentForTest,
  settleRunCommandConsent
} from './deep-link-consent-gate'
import { activateAndRevealWorktree } from './worktree-activation'
import { activateTabAndFocusPane } from './activate-tab-and-focus-pane'
import {
  showDeepLinkPairRoutedToast,
  showDeepLinkRunIgnoredToast,
  showDeepLinkTerminalGoneToast,
  showDeepLinkUnknownWorkspaceToast,
  showDeepLinkUnrecognizedToast
} from './deep-link-ui-notices'
import type { OrcaDeepLinkOrigin } from '../../../shared/orca-deep-link'

const mockStoreState = vi.hoisted(() => ({
  state: {
    openSettingsPage: vi.fn(),
    openSettingsTarget: vi.fn(),
    getKnownWorktreeById: vi.fn(
      (): { id: string; displayName: string } | undefined => ({
        id: 'repo::wt',
        displayName: 'wt'
      })
    )
  }
}))

vi.mock('@/store', () => ({
  useAppStore: { getState: () => mockStoreState.state }
}))

vi.mock('@/lib/worktree-activation', () => ({
  activateAndRevealWorktree: vi.fn(() => ({ worktree: { id: 'repo::wt' } }))
}))

vi.mock('@/lib/activate-tab-and-focus-pane', () => ({
  activateTabAndFocusPane: vi.fn()
}))

vi.mock('@/lib/deep-link-ui-notices', () => ({
  showDeepLinkPairRoutedToast: vi.fn(),
  showDeepLinkRunIgnoredToast: vi.fn(),
  showDeepLinkTerminalGoneToast: vi.fn(),
  showDeepLinkUnknownWorkspaceToast: vi.fn(),
  showDeepLinkUnrecognizedToast: vi.fn(),
  showDeepLinkUnsupportedToast: vi.fn()
}))

const osOrigin: OrcaDeepLinkOrigin = { source: 'os' }
const terminalOrigin: OrcaDeepLinkOrigin = { source: 'terminal', worktreeId: 'repo::origin-wt' }

beforeEach(() => {
  vi.clearAllMocks()
  resetRunCommandConsentForTest()
  mockStoreState.state.getKnownWorktreeById.mockReturnValue({
    id: 'repo::wt',
    displayName: 'wt'
  })
  vi.mocked(activateAndRevealWorktree).mockReturnValue(
    { worktree: { id: 'repo::wt' } } as unknown as ReturnType<typeof activateAndRevealWorktree>
  )
})

describe('dispatchDeepLinkInRenderer worktree', () => {
  it('activates the worktree through the canonical activation path', () => {
    dispatchDeepLinkInRenderer({ kind: 'worktree', worktreeId: 'repo::wt' }, terminalOrigin)

    expect(activateAndRevealWorktree).toHaveBeenCalledWith('repo::wt')
    expect(activateTabAndFocusPane).not.toHaveBeenCalled()
  })

  it('focuses the requested tab after activation when ?tab= is present', () => {
    dispatchDeepLinkInRenderer(
      { kind: 'worktree', worktreeId: 'repo::wt', tabId: 'tab-9' },
      terminalOrigin
    )

    expect(activateTabAndFocusPane).toHaveBeenCalledWith('tab-9', null)
  })

  it('unknown worktree toasts instead of navigating', () => {
    vi.mocked(activateAndRevealWorktree).mockReturnValue(false)

    dispatchDeepLinkInRenderer({ kind: 'worktree', worktreeId: 'repo::gone' }, terminalOrigin)

    expect(showDeepLinkUnknownWorkspaceToast).toHaveBeenCalledTimes(1)
    expect(activateTabAndFocusPane).not.toHaveBeenCalled()
  })

  it('is deferred while a run-consent dialog is open and runs after settle', () => {
    requestRunCommandConsent({
      link: { kind: 'run', worktreeId: 'repo::wt', command: 'ls' },
      origin: osOrigin
    })

    dispatchDeepLinkInRenderer({ kind: 'worktree', worktreeId: 'repo::wt' }, osOrigin)
    expect(activateAndRevealWorktree).not.toHaveBeenCalled()

    settleRunCommandConsent()
    expect(activateAndRevealWorktree).toHaveBeenCalledWith('repo::wt')
  })
})

describe('dispatchDeepLinkInRenderer pair', () => {
  it('routes to the Mobile settings pane with an origin-labeled toast — never auto-pairs', () => {
    dispatchDeepLinkInRenderer({ kind: 'pair', code: 'secret' }, osOrigin)

    expect(mockStoreState.state.openSettingsPage).toHaveBeenCalledTimes(1)
    expect(mockStoreState.state.openSettingsTarget).toHaveBeenCalledWith({
      pane: 'mobile',
      repoId: null
    })
    expect(showDeepLinkPairRoutedToast).toHaveBeenCalledWith(osOrigin)
  })
})

describe('dispatchDeepLinkInRenderer run', () => {
  it('opens consent with the transport-stamped origin', () => {
    dispatchDeepLinkInRenderer(
      { kind: 'run', worktreeId: 'repo::wt', command: 'npm test' },
      terminalOrigin
    )

    const pending = getPendingRunCommandConsent()
    expect(pending?.link.command).toBe('npm test')
    expect(pending?.origin).toEqual(terminalOrigin)
  })

  it('never executes anything at dispatch time', () => {
    dispatchDeepLinkInRenderer({ kind: 'run', worktreeId: 'repo::wt', command: 'rm -rf' }, osOrigin)

    expect(activateAndRevealWorktree).not.toHaveBeenCalled()
    expect(mockStoreState.state.openSettingsPage).not.toHaveBeenCalled()
  })

  it('whitespace-only command is malformed, not consented', () => {
    dispatchDeepLinkInRenderer({ kind: 'run', worktreeId: 'repo::wt', command: '   ' }, osOrigin)

    expect(getPendingRunCommandConsent()).toBeNull()
    expect(showDeepLinkUnrecognizedToast).toHaveBeenCalledTimes(1)
  })

  it('unknown target worktree toasts instead of opening consent', () => {
    mockStoreState.state.getKnownWorktreeById.mockReturnValue(undefined)

    dispatchDeepLinkInRenderer({ kind: 'run', worktreeId: 'repo::gone', command: 'ls' }, osOrigin)

    expect(getPendingRunCommandConsent()).toBeNull()
    expect(showDeepLinkUnknownWorkspaceToast).toHaveBeenCalledTimes(1)
  })

  it('a second run link while consent is open is dropped with a toast', () => {
    dispatchDeepLinkInRenderer({ kind: 'run', worktreeId: 'repo::wt', command: 'first' }, osOrigin)
    dispatchDeepLinkInRenderer(
      { kind: 'run', worktreeId: 'repo::wt', command: 'second' },
      terminalOrigin
    )

    expect(getPendingRunCommandConsent()?.link.command).toBe('first')
    expect(showDeepLinkRunIgnoredToast).toHaveBeenCalledTimes(1)
  })
})

describe('handleDeepLinkUiEvent', () => {
  it('link events dispatch with the main-stamped origin', () => {
    handleDeepLinkUiEvent({
      type: 'link',
      link: { kind: 'run', worktreeId: 'repo::wt', command: 'ls' },
      origin: osOrigin
    })

    expect(getPendingRunCommandConsent()?.origin).toEqual(osOrigin)
  })

  it('notice events still toast', () => {
    handleDeepLinkUiEvent({ type: 'notice', notice: 'terminal-gone' })

    expect(showDeepLinkTerminalGoneToast).toHaveBeenCalledTimes(1)
  })
})
