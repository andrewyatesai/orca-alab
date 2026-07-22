import type { Tab, TerminalTab, TopLevelView } from '../../../shared/types'

export type StartupFloatingWorkspaceDecision = 'wait' | 'open' | 'suppress'

type StartupFloatingWorkspaceDecisionInput = {
  activeView: TopLevelView
  activeWorktreeId: string | null
  creationLayoutActive: boolean
  floatingWorkspaceEnabled: boolean
  hydrationSucceeded: boolean
  onboardingLoaded: boolean
  onboardingVisible: boolean
  persistedUIReady: boolean
  startupDecisionHandled: boolean
}

export function getStartupFloatingWorkspaceDecision({
  activeView,
  activeWorktreeId,
  creationLayoutActive,
  floatingWorkspaceEnabled,
  hydrationSucceeded,
  onboardingLoaded,
  onboardingVisible,
  persistedUIReady,
  startupDecisionHandled
}: StartupFloatingWorkspaceDecisionInput): StartupFloatingWorkspaceDecision {
  if (startupDecisionHandled) {
    return 'suppress'
  }
  if (!hydrationSucceeded || !onboardingLoaded || !persistedUIReady) {
    return 'wait'
  }
  if (onboardingVisible) {
    // Why: onboarding is a temporary startup surface; skipping it should still
    // reveal the default scratch terminal during this launch.
    return 'wait'
  }
  if (
    activeView !== 'terminal' ||
    activeWorktreeId !== null ||
    creationLayoutActive ||
    !floatingWorkspaceEnabled
  ) {
    return 'suppress'
  }
  return 'open'
}

type ReusableFloatingWorkspaceTerminal = {
  terminalTabId: string
  unifiedTabId: string
}

export function getReusableFloatingWorkspaceTerminal(
  terminalTabs: readonly Pick<TerminalTab, 'id'>[],
  unifiedTabs: readonly Pick<Tab, 'contentType' | 'entityId' | 'id'>[]
): ReusableFloatingWorkspaceTerminal | null {
  const terminalTabIds = new Set(terminalTabs.map((tab) => tab.id))
  const unifiedTab = unifiedTabs.find(
    (tab) => tab.contentType === 'terminal' && terminalTabIds.has(tab.entityId)
  )
  return unifiedTab ? { terminalTabId: unifiedTab.entityId, unifiedTabId: unifiedTab.id } : null
}
