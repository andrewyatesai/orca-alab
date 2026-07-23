import {
  describeOrcaDeepLinkForLog,
  type OrcaDeepLink,
  type OrcaDeepLinkOrigin,
  type OrcaDeepLinkUiEvent
} from '../../shared/orca-deep-link'

export const DEEP_LINK_UI_CHANNEL = 'ui:deepLink'

export type DeepLinkDispatchDeps = {
  /** Runtime accessor — index.ts holds the singleton; null before ready. */
  getRuntime: () => { focusTerminal: (handle: string) => Promise<unknown> } | null
  /** Send to the main window's webContents; returns false when no window exists. */
  sendDeepLinkUiEvent: (event: OrcaDeepLinkUiEvent) => boolean
  focusMainWindow: () => void
  log?: (message: string) => void
}

export type MainDeepLinkDispatcher = {
  dispatch: (link: OrcaDeepLink, origin: OrcaDeepLinkOrigin) => void
  /** Toast for OS-routed URLs that failed the shared parser. */
  notifyUnrecognized: () => void
}

/** PR1 dispatch: `focus` resolves through the SAME canonical action as the
 *  `terminal.focus` RPC (runtime.focusTerminal); `worktree`/`pair`/`run` are
 *  forwarded to the renderer, which shows an "unsupported yet" notice until PR2
 *  lands navigation + consent. */
export function createMainDeepLinkDispatcher(deps: DeepLinkDispatchDeps): MainDeepLinkDispatcher {
  const log = deps.log ?? console.log
  return {
    dispatch: (link, origin) => {
      // Why: every OS dispatch surfaces the window first (open-url has no
      // second-instance activation to ride on).
      deps.focusMainWindow()
      // describeOrcaDeepLinkForLog redacts pair codes and run commands.
      log(`[deep-links] dispatch ${describeOrcaDeepLinkForLog(link)} (origin: ${origin.source})`)
      if (link.kind === 'focus') {
        const runtime = deps.getRuntime()
        if (!runtime) {
          deps.sendDeepLinkUiEvent({ type: 'notice', notice: 'terminal-gone' })
          return
        }
        runtime.focusTerminal(link.handle).catch(() => {
          // terminal_handle_stale / terminal_exited → "Terminal is no longer running".
          deps.sendDeepLinkUiEvent({ type: 'notice', notice: 'terminal-gone' })
        })
        return
      }
      deps.sendDeepLinkUiEvent({ type: 'link', link, origin })
    },
    notifyUnrecognized: () => {
      deps.sendDeepLinkUiEvent({ type: 'notice', notice: 'unrecognized' })
    }
  }
}
