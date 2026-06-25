type RegisteredPaneManager = {
  resetWebglTextureAtlases?: () => void
  fitAllPanes?: () => void
  refreshAllPanes?: () => void
}

const liveManagers = new Set<RegisteredPaneManager>()

export function registerLivePaneManager(manager: RegisteredPaneManager): void {
  liveManagers.add(manager)
}

export function unregisterLivePaneManager(manager: RegisteredPaneManager): void {
  liveManagers.delete(manager)
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
