import { useEffect } from 'react'
import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import { dispatchZoomLevelChanged } from '@/lib/zoom-events'
import { safeFit } from '@/lib/pane-manager/pane-tree-ops'
import { getPaneOwnedActiveHelperTextarea } from './regular-terminal-focus-ownership'
import {
  TERMINAL_PANE_MAX_FONT_SIZE,
  TERMINAL_PANE_MIN_FONT_SIZE
} from './layout-serialization'

type FontZoomDeps = {
  isActive: boolean
  containerRef: React.RefObject<HTMLElement | null>
  managerRef: React.RefObject<PaneManager | null>
  paneFontSizesRef: React.RefObject<Map<number, number>>
  settingsRef: React.RefObject<{ terminalFontSize?: number } | null>
  /** Persists the pane layout so zoom deltas survive restart (#8516). */
  persistLayoutSnapshot?: () => void
}

export function useTerminalFontZoom({
  isActive,
  containerRef,
  managerRef,
  paneFontSizesRef,
  settingsRef,
  persistLayoutSnapshot
}: FontZoomDeps): void {
  useEffect(() => {
    if (!isActive) {
      return
    }
    const FONT_SIZE_STEP = 1

    return window.api.ui.onTerminalZoom((direction) => {
      const container = containerRef.current
      if (!container || !getPaneOwnedActiveHelperTextarea(container, document.activeElement)) {
        return
      }
      const manager = managerRef.current
      if (!manager) {
        return
      }
      const pane = manager.getActivePane()
      if (!pane) {
        return
      }

      const globalSize = settingsRef.current?.terminalFontSize ?? 14
      const currentSize = paneFontSizesRef.current.get(pane.id) ?? globalSize

      let nextSize: number
      if (direction === 'reset') {
        nextSize = globalSize
        paneFontSizesRef.current.delete(pane.id)
      } else if (direction === 'in') {
        nextSize = Math.min(TERMINAL_PANE_MAX_FONT_SIZE, currentSize + FONT_SIZE_STEP)
        paneFontSizesRef.current.set(pane.id, nextSize)
      } else {
        nextSize = Math.max(TERMINAL_PANE_MIN_FONT_SIZE, currentSize - FONT_SIZE_STEP)
        paneFontSizesRef.current.set(pane.id, nextSize)
      }

      pane.terminal.options.fontSize = nextSize
      safeFit(pane)
      // Why: the zoom map is a ref, so nothing else re-persists the layout; snapshot now or the delta dies with the mount.
      persistLayoutSnapshot?.()

      const percent = Math.round((nextSize / globalSize) * 100)
      dispatchZoomLevelChanged('terminal', percent)
    })
  }, [containerRef, isActive, managerRef, paneFontSizesRef, persistLayoutSnapshot, settingsRef])
}
