import {
  describeOrcaDeepLinkForLog,
  type OrcaDeepLink,
  type OrcaDeepLinkOrigin,
  type OrcaDeepLinkUiEvent
} from '../../shared/orca-deep-link'
import { getRepoIdFromWorktreeId, WORKTREE_ID_SEPARATOR } from '../../shared/worktree-id'

export const DEEP_LINK_UI_CHANNEL = 'ui:deepLink'

export type DeepLinkDispatchDeps = {
  /** Runtime accessor — index.ts holds the singleton; null before ready. */
  getRuntime: () => { focusTerminal: (handle: string) => Promise<unknown> } | null
  /** Send to the main window's webContents; returns false when no window exists. */
  sendDeepLinkUiEvent: (event: OrcaDeepLinkUiEvent) => boolean
  /** `ui:activateWorktree` — the notification-click navigation channel (notifications.ts precedent). */
  sendActivateWorktree: (payload: { repoId: string; worktreeId: string }) => boolean
  /** `ui:focusTerminal` follow-up for `?tab=` worktree links. */
  sendFocusTerminal: (payload: {
    tabId: string
    worktreeId: string
    leafId: string | null
  }) => boolean
  focusMainWindow: () => void
  log?: (message: string) => void
}

export type MainDeepLinkDispatcher = {
  dispatch: (link: OrcaDeepLink, origin: OrcaDeepLinkOrigin) => void
  /** Toast for OS-routed URLs that failed the shared parser. */
  notifyUnrecognized: () => void
}

/** Main-process dispatch: `focus` resolves through the SAME canonical action as
 *  the `terminal.focus` RPC (runtime.focusTerminal); `worktree` mirrors the
 *  notification-click path over `ui:activateWorktree`/`ui:focusTerminal`;
 *  `pair`/`run` are forwarded to the renderer's consent surface over
 *  `ui:deepLink` with the transport-stamped origin (#4384 PR2). */
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
      if (link.kind === 'worktree') {
        // Why: ids are `repoId::worktreePath`; without the separator no repoId exists (notifications.ts precedent).
        if (!link.worktreeId.includes(WORKTREE_ID_SEPARATOR)) {
          deps.sendDeepLinkUiEvent({ type: 'notice', notice: 'unrecognized' })
          return
        }
        deps.sendActivateWorktree({
          repoId: getRepoIdFromWorktreeId(link.worktreeId),
          worktreeId: link.worktreeId
        })
        if (link.tabId) {
          deps.sendFocusTerminal({
            tabId: link.tabId,
            worktreeId: link.worktreeId,
            leafId: null
          })
        }
        return
      }
      deps.sendDeepLinkUiEvent({ type: 'link', link, origin })
    },
    notifyUnrecognized: () => {
      deps.sendDeepLinkUiEvent({ type: 'notice', notice: 'unrecognized' })
    }
  }
}
