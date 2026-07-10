import type { AtermRainPulse } from '../../../../shared/aterm-rain-signal'

type RegisteredPaneManager = {
  resetWebglTextureAtlases?: () => void
  fitAllPanes?: () => void
  refreshAllPanes?: () => void
}

type TabPaneManager = RegisteredPaneManager & {
  getPanes: () => {
    leafId: string
    atermController?: {
      noteMatrixRainPulse?: (pulse: AtermRainPulse) => void
    } | null
  }[]
}

const liveManagers = new Set<RegisteredPaneManager>()
// React can briefly overlap the old and replacement TerminalPane lifecycle for
// one durable tab. Retain both identities until their own cleanup runs so a
// hook pulse cannot disappear into whichever mount happened to register last.
const managersByTabId = new Map<string, Set<TabPaneManager>>()

type TabPaneManagerLifecycleObserver = {
  managerRegistered: (tabId: string) => void
}

let tabLifecycleObserver: TabPaneManagerLifecycleObserver | null = null

/** Install the single semantic-effects observer. Kept callback-shaped so this
 * registry stays independent of the aterm delivery module (no import cycle). */
export function setTabPaneManagerLifecycleObserver(
  observer: TabPaneManagerLifecycleObserver
): void {
  tabLifecycleObserver = observer
}

export function registerLivePaneManager(manager: RegisteredPaneManager): void {
  liveManagers.add(manager)
}

export function unregisterLivePaneManager(manager: RegisteredPaneManager): void {
  liveManagers.delete(manager)
}

/** Register the stable tab identity separately from the global repaint set.
 * PaneManager itself does not own a tab id; the TerminalPane lifecycle does. */
export function registerTabPaneManager(tabId: string, manager: TabPaneManager): void {
  let managers = managersByTabId.get(tabId)
  if (!managers) {
    managers = new Set()
    managersByTabId.set(tabId, managers)
  }
  managers.add(manager)
  try {
    tabLifecycleObserver?.managerRegistered(tabId)
  } catch {
    // Registration owns terminal lifecycle; a best-effort visual effect must
    // never prevent the manager from becoming reachable.
  }
}

export function unregisterTabPaneManager(tabId: string, manager: TabPaneManager): void {
  const managers = managersByTabId.get(tabId)
  managers?.delete(manager)
  if (managers?.size === 0) {
    managersByTabId.delete(tabId)
  }
}

export function getLivePaneManagersForTab(tabId: string): readonly TabPaneManager[] {
  return [...(managersByTabId.get(tabId) ?? [])]
}

/** Resolve the durable tab identity at the exact async controller-attach edge.
 * This bounded live-manager scan is cold-path only (once per controller build). */
export function getRegisteredTabIdsForController(
  leafId: string,
  controller: object
): readonly string[] {
  const tabIds: string[] = []
  for (const [tabId, managers] of managersByTabId) {
    let matched = false
    for (const manager of managers) {
      try {
        matched = manager
          .getPanes()
          .some((pane) => pane.leafId === leafId && (pane.atermController as object) === controller)
      } catch {
        // An overlapping manager may already be tearing down; inspect siblings.
      }
      if (matched) {
        tabIds.push(tabId)
        break
      }
    }
  }
  return tabIds
}

/** Force a fresh full repaint of every pane across all live managers. The aterm
 *  GPU drawer re-presents the engine grid each frame (no shared glyph atlas to
 *  invalidate), so this re-rasterizes the current frame — the honest aterm
 *  equivalent of the old cross-manager xterm-WebGL atlas reset. */
export function resetAllTerminalWebglAtlases(): void {
  for (const manager of liveManagers) {
    try {
      manager.resetWebglTextureAtlases?.()
    } catch {
      // Why: best-effort during pane teardown; one disposed manager should not
      // prevent sibling terminals from repainting.
    }
  }
}

export function resetAndRefreshAllTerminalWebglAtlases(): void {
  const resetManagers: RegisteredPaneManager[] = []
  for (const manager of liveManagers) {
    try {
      manager.resetWebglTextureAtlases?.()
      resetManagers.push(manager)
    } catch {
      // Why: recovery is best-effort during pane teardown; a disposed manager
      // should not block sibling terminals from rebuilding and repainting.
    }
  }
  for (const manager of resetManagers) {
    try {
      manager.refreshAllPanes?.()
    } catch {
      // Why: a pane can unmount between atlas reset and repaint; later
      // managers still need to repaint from their xterm buffers.
    }
  }
}

export function refitAndRefreshAllTerminalPanes(): void {
  for (const manager of liveManagers) {
    try {
      // Why: after bulk desktop restore, background panes may have correct
      // cols/rows but a stale xterm renderer until focus forces a repaint.
      manager.fitAllPanes?.()
      manager.refreshAllPanes?.()
    } catch {
      // Why: restore-all is best-effort across live managers during teardown.
    }
  }
}
