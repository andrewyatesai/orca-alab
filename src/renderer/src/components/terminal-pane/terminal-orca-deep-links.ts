import { parseOrcaDeepLink } from '../../../../shared/orca-deep-link'
import {
  showDeepLinkTerminalGoneToast,
  showDeepLinkUnrecognizedToast
} from '@/lib/deep-link-ui-notices'
import { dispatchDeepLinkInRenderer } from '@/lib/deep-link-renderer-dispatch'
import { focusRendererTerminalHandle, focusRuntimeTerminalHandle } from './terminal-handle-links'

export type TerminalOrcaDeepLinkContext = {
  /** Worktree of the pane the link was CLICKED in — the origin, not the target. */
  worktreeId: string
  /** Runtime (SSH/WSL) of the clicked pane; handles are runtime-scoped. */
  runtimeEnvironmentId?: string | null
}

/** Terminal-minted orca:// links route IN-APP: never through shell.openExternal /
 *  the OS handler — that would bounce through a hijackable registration and
 *  erase the origin (design #4384 §5.3). Always returns true: the click is
 *  consumed so a malformed link can't fall through to file-path detection. */
export function routeTerminalOrcaDeepLink(
  raw: string,
  context: TerminalOrcaDeepLinkContext
): boolean {
  const link = parseOrcaDeepLink(raw)
  if (!link) {
    showDeepLinkUnrecognizedToast()
    return true
  }
  if (link.kind === 'focus') {
    const runtimeEnvironmentId = context.runtimeEnvironmentId ?? null
    if (!focusRendererTerminalHandle(link.handle, runtimeEnvironmentId)) {
      // Why: sleeping/other-window sessions aren't in renderer state; the
      // terminal.focus RPC resolves them on whichever runtime issued the handle.
      focusRuntimeTerminalHandle(link.handle, runtimeEnvironmentId).catch(() => {
        showDeepLinkTerminalGoneToast()
      })
    }
    return true
  }
  // Why: the origin worktree is the pane the link was CLICKED in — stamped by this
  // transport, never taken from the URL — so consent can label untrusted output (§6.2).
  dispatchDeepLinkInRenderer(link, { source: 'terminal', worktreeId: context.worktreeId })
  return true
}
