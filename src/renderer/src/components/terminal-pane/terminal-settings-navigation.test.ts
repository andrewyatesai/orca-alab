// CM-A5: the context menu's Terminal Settings jump — deepest existing target is
// the Terminal settings pane scoped to the pane's repo (no pane-scoped model).

import { describe, expect, it, vi } from 'vitest'
import { openTerminalSettingsForRepo } from './terminal-settings-navigation'

const store = vi.hoisted(() => ({
  openSettingsTarget: vi.fn(),
  openSettingsPage: vi.fn()
}))
vi.mock('@/store', () => ({ useAppStore: { getState: () => store } }))

describe('openTerminalSettingsForRepo', () => {
  it('sets the settings navigation target and opens the settings page', () => {
    openTerminalSettingsForRepo('repo-42')
    expect(store.openSettingsTarget).toHaveBeenCalledWith({ pane: 'terminal', repoId: 'repo-42' })
    expect(store.openSettingsPage).toHaveBeenCalledTimes(1)
  })

  it('targets the global terminal settings when the pane has no repo', () => {
    openTerminalSettingsForRepo(null)
    expect(store.openSettingsTarget).toHaveBeenCalledWith({ pane: 'terminal', repoId: null })
  })
})
