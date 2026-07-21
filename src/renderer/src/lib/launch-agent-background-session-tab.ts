import type { useAppStore } from '@/store'
import type { TuiAgent } from '../../../shared/types'
import type { SleepingAgentLaunchConfig } from '../../../shared/agent-session-resume'
import { makePaneKey, type PaneKey } from '../../../shared/stable-pane-id'
import { singlePaneLayoutSnapshot } from '@/store/slices/terminal-helpers'

type AppStore = ReturnType<typeof useAppStore.getState>

/**
 * Attaches an inactive tab to an already-live background PTY in one
 * synchronous block. Why: a mounted worktree renders new tabs immediately, so
 * any await between tab creation and PTY binding lets TerminalPane's
 * fresh-spawn path bind a default shell to the run tab and orphan the agent
 * PTY (#2989). Callers must spawn the PTY first (against `reservedTabId`) and
 * only then create the tab through this helper.
 */
export function adoptAgentBackgroundSessionTab({
  store,
  worktreeId,
  reservedTabId,
  leafId,
  ptyId,
  title,
  agent,
  launchToken,
  launchConfig
}: {
  store: AppStore
  worktreeId: string
  reservedTabId: string
  leafId: string
  ptyId: string
  title: string | undefined
  agent: TuiAgent
  launchToken: string
  launchConfig: SleepingAgentLaunchConfig
}): { tab: ReturnType<AppStore['createTab']>; paneKey: PaneKey } {
  const tab = store.createTab(worktreeId, undefined, undefined, {
    id: reservedTabId,
    initialPtyId: ptyId,
    activate: false,
    recordInteraction: false
  })
  // Why: createTab mints a fresh id when the reserved id collides (and warns).
  // Store-side pane routing must key off the actual tab id; hook attribution
  // for the env-baked paneKey degrades for that terminal only.
  const paneKey = makePaneKey(tab.id, leafId)
  if (title) {
    store.setTabCustomTitle(tab.id, title, { recordInteraction: false })
  }
  store.registerAgentLaunchConfig(paneKey, launchConfig, {
    agentType: agent,
    launchToken,
    tabId: tab.id,
    leafId
  })
  store.updateTabPtyId(tab.id, ptyId)
  // Why: `title` labels the tab/worktree entry. Pane titles render as an
  // in-terminal title row, so background sessions must not persist it there.
  store.setTabLayout(tab.id, singlePaneLayoutSnapshot(leafId, ptyId))
  return { tab, paneKey }
}
