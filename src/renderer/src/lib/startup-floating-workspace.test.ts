import { describe, expect, it } from 'vitest'
import type { Tab, TerminalTab } from '../../../shared/types'
import {
  getReusableFloatingWorkspaceTerminal,
  getStartupFloatingWorkspaceDecision
} from './startup-floating-workspace'

const eligibleStartup = {
  activeView: 'terminal' as const,
  activeWorktreeId: null,
  creationLayoutActive: false,
  floatingWorkspaceEnabled: true,
  hydrationSucceeded: true,
  onboardingLoaded: true,
  onboardingVisible: false,
  persistedUIReady: true,
  startupDecisionHandled: false
}

describe('getStartupFloatingWorkspaceDecision', () => {
  it('opens after successful hydration on the terminal view with no project', () => {
    expect(getStartupFloatingWorkspaceDecision(eligibleStartup)).toBe('open')
  })

  it.each([
    ['session hydration', { hydrationSucceeded: false }],
    ['persisted UI', { persistedUIReady: false }],
    ['onboarding state', { onboardingLoaded: false }]
  ])('waits for %s before deciding', (_label, override) => {
    expect(getStartupFloatingWorkspaceDecision({ ...eligibleStartup, ...override })).toBe('wait')
  })

  it.each([
    ['an explicit opt-out', { floatingWorkspaceEnabled: false }],
    ['an active project', { activeWorktreeId: 'worktree-1' }],
    ['an active project creation', { creationLayoutActive: true }],
    ['a secondary view', { activeView: 'tasks' as const }]
  ])('suppresses the startup open for %s', (_label, override) => {
    expect(getStartupFloatingWorkspaceDecision({ ...eligibleStartup, ...override })).toBe(
      'suppress'
    )
  })

  it('stays suppressed after the startup decision has been consumed', () => {
    expect(
      getStartupFloatingWorkspaceDecision({
        ...eligibleStartup,
        startupDecisionHandled: true
      })
    ).toBe('suppress')
  })

  it('waits through onboarding so skipping it can still open the scratch terminal', () => {
    expect(
      getStartupFloatingWorkspaceDecision({ ...eligibleStartup, onboardingVisible: true })
    ).toBe('wait')
    expect(getStartupFloatingWorkspaceDecision(eligibleStartup)).toBe('open')
  })
})

describe('getReusableFloatingWorkspaceTerminal', () => {
  const terminalTab = (id: string): Pick<TerminalTab, 'id'> => ({ id })
  const unifiedTab = (
    id: string,
    entityId: string,
    contentType: Tab['contentType']
  ): Pick<Tab, 'contentType' | 'entityId' | 'id'> => ({ contentType, entityId, id })

  it('returns a restored terminal only when its backing terminal still exists', () => {
    expect(
      getReusableFloatingWorkspaceTerminal(
        [terminalTab('terminal-1')],
        [
          unifiedTab('browser-1', 'browser-entity', 'browser'),
          unifiedTab('orphan-terminal', 'missing-terminal', 'terminal'),
          unifiedTab('unified-terminal-1', 'terminal-1', 'terminal')
        ]
      )
    ).toEqual({ terminalTabId: 'terminal-1', unifiedTabId: 'unified-terminal-1' })
  })

  it('returns null when hydration restored no usable terminal', () => {
    expect(
      getReusableFloatingWorkspaceTerminal(
        [terminalTab('terminal-1')],
        [unifiedTab('browser-1', 'browser-entity', 'browser')]
      )
    ).toBeNull()
  })
})
