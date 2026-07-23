// Production pane discovery for the federated live adapter: the tab pane-manager
// registry supplies live panes (leafId + controller); the app store supplies tab
// identity (worktree, title, active tab). Best-effort by design — a pane torn
// down mid-enumeration is simply absent from this generation's fan-out, and the
// stale-paneRef degradation (§1 Navigation) owns any result→click gap.

import {
  getLivePaneManagersForTab,
  getRegisteredTabPaneManagerTabIds
} from '../pane-manager/pane-manager-registry'
import { isTerminalLeafId, makePaneKey } from '../../../../shared/stable-pane-id'
import { useAppStore } from '@/store'
import type { AtermFederatedSearchTarget } from '../pane-manager/aterm/aterm-federated-search-target'
import type { DiscoveredLivePane } from './live-pane-search-adapter'

type StoreTabIndexEntry = { worktreeId: string; title: string | null }

function indexTabs(): Map<string, StoreTabIndexEntry> {
  const state = useAppStore.getState()
  const byTabId = new Map<string, StoreTabIndexEntry>()
  for (const [worktreeId, tabs] of Object.entries(state.tabsByWorktree)) {
    for (const tab of tabs) {
      byTabId.set(tab.id, { worktreeId, title: tab.title ?? null })
    }
  }
  return byTabId
}

/** Enumerate every live/hidden pane with a federated search target. */
export function discoverLiveFederatedPanes(): DiscoveredLivePane[] {
  const state = useAppStore.getState()
  const tabIndex = indexTabs()
  const activeTabId = state.activeTabId
  const panes: DiscoveredLivePane[] = []
  for (const tabId of getRegisteredTabPaneManagerTabIds()) {
    const tabMeta = tabIndex.get(tabId)
    for (const manager of getLivePaneManagersForTab(tabId)) {
      let managerPanes: ReturnType<(typeof manager)['getPanes']>
      try {
        managerPanes = manager.getPanes()
      } catch {
        continue // manager tearing down — skip it, siblings still enumerate
      }
      for (const pane of managerPanes) {
        const target = pane.atermController?.federatedSearchTarget?.() as
          | AtermFederatedSearchTarget
          | null
          | undefined
        if (!target || !isTerminalLeafId(pane.leafId)) {
          continue
        }
        const focused =
          pane.container instanceof HTMLElement &&
          document.activeElement instanceof Element &&
          pane.container.contains(document.activeElement)
        panes.push({
          paneRef: {
            tabId,
            leafId: pane.leafId,
            paneKey: makePaneKey(tabId, pane.leafId),
            worktreeId: tabMeta?.worktreeId ?? null,
            title: tabMeta?.title ?? null
          },
          visible: tabId === activeTabId,
          focused,
          // Daemon session identity is wired by the daemon adapter track (5C);
          // until then live panes dedup by paneKey only.
          sessionId: null,
          lastOutputAt: 0,
          target
        })
      }
    }
  }
  return panes
}
