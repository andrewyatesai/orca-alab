type RegisteredPaneManager = {
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
