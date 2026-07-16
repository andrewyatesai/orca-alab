import { recordTerminalWebglDiagnostic } from '../../../../shared/terminal-webgl-diagnostics'
import type { AtermRainPulse } from '../../../../shared/aterm-rain-signal'
import type { PaneRenderingDiagnostics } from './pane-manager-types'

type RegisteredPaneManager = {
  resetWebglTextureAtlases?: () => void
  fitAllPanes?: () => void
  refreshAllPanes?: () => void
  getRenderingDiagnostics?: () => PaneRenderingDiagnostics[]
}

type TabPaneManager = RegisteredPaneManager & {
  getPanes: () => {
    leafId: string
    /** The .pane element; window-level overlays (spill geometry) measure it. */
    container?: HTMLElement
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

/** Read-only snapshot of every tabId with a live pane manager. Window-level
 * overlays (the spill geometry tracker) enumerate ALL panes through this plus
 * getLivePaneManagersForTab; it grants no mutation access to the registry. */
export function getRegisteredTabPaneManagerTabIds(): readonly string[] {
  return [...managersByTabId.keys()]
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
  // Why: the atlas wipe is the heavy recovery path; recording it lets a freeze
  // report show whether a post-wake repaint actually ran. Silent breadcrumb.
  recordTerminalWebglDiagnostic('webgl-atlas-reset', { managers: liveManagers.size })
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

/**
 * Per-pane WebGL renderer state across all live managers, for the one-paste
 * freeze report. Lets a post-wake garble report show, per pane, whether it
 * held a live WebGL addon or had fallen back after a context loss — the state
 * that distinguishes "missed repaint" from "atlas corrupted".
 */
export function getAllPaneRenderingDiagnostics(): PaneRenderingDiagnostics[] {
  const all: PaneRenderingDiagnostics[] = []
  for (const manager of liveManagers) {
    try {
      const diagnostics = manager.getRenderingDiagnostics?.()
      if (diagnostics) {
        all.push(...diagnostics)
      }
    } catch {
      // Why: best-effort during teardown; one manager must not sink the report.
    }
  }
  return all
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
