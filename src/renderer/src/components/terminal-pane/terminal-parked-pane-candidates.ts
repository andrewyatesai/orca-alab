/**
 * Pane-candidate resolution for parked-tab watchers: which {ptyId, paneId,
 * leafId} tuples a parked tab's watchers must cover, from the park-time capture
 * or the persisted layout fallback.
 */
import type { useAppStore } from '@/store'
import { collectLeafIdsInOrder } from './terminal-layout-leaf-ids'
import {
  capturedPanesByTabId,
  type ParkedTerminalPaneCapture
} from './terminal-parked-watcher-registry'

export type ParkableTerminalTabIdentity = { id: string; ptyId: string | null }

type ParkedPaneFallbackState = {
  terminalLayoutsByTabId: ReturnType<typeof useAppStore.getState>['terminalLayoutsByTabId']
  runtimePaneTitlesByTabId: ReturnType<typeof useAppStore.getState>['runtimePaneTitlesByTabId']
}

// Why: pane ids are unknown in this layout fallback; reuse the sole runtime-title slot when unambiguous to overwrite a stale title, else negative slots that can't collide with real PaneManager ids.
export function fallbackParkedPaneCandidates(
  tab: ParkableTerminalTabIdentity,
  state: ParkedPaneFallbackState
): ParkedTerminalPaneCapture[] {
  const layout = state.terminalLayoutsByTabId[tab.id]
  const leafIds = collectLeafIdsInOrder(layout?.root)
  if (leafIds.length === 0) {
    return []
  }
  const ptyIdsByLeafId = layout?.ptyIdsByLeafId ?? {}
  const titleSlots = Object.keys(state.runtimePaneTitlesByTabId[tab.id] ?? {})
  const reusableSlot =
    leafIds.length === 1 && titleSlots.length === 1 ? Number(titleSlots[0]) : null
  return leafIds.map((leafId, index) => ({
    ptyId: ptyIdsByLeafId[leafId] ?? (leafIds.length === 1 ? tab.ptyId : null),
    paneId: reusableSlot ?? -(index + 1),
    leafId,
    drivesTabTitle: layout?.activeLeafId ? leafId === layout.activeLeafId : index === 0
  }))
}

// Why: start path and eligibility check must resolve identical candidates, or a tab passes the check then starts uncoverable.
export function resolveParkedTerminalPaneCandidates(
  tab: ParkableTerminalTabIdentity,
  state: ParkedPaneFallbackState
): ParkedTerminalPaneCapture[] {
  const captured = capturedPanesByTabId.get(tab.id)
  // Why: a capture missing the tab's current PTY is stale (PTY re-minted since unmount); fall back to the layout.
  const capturedIsCurrent =
    captured !== undefined &&
    captured.panes.length > 0 &&
    (tab.ptyId === null || captured.panes.some((pane) => pane.ptyId === tab.ptyId))
  return capturedIsCurrent ? captured.panes : fallbackParkedPaneCandidates(tab, state)
}
