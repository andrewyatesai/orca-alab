import { openHttpLink } from '../../http-link-routing'
import { openTerminalHttpLink } from '../../../components/terminal-pane/terminal-url-link-hit-testing'
import { handleOscLink } from '../../../components/terminal-pane/terminal-osc-link-routing'
import type { AtermLinkOpener, AtermOscLinkOpener } from './aterm-link-input'

/** Optional pane-scoped link routing context. When supplied, terminal URL
 *  clicks honor orca's in-app/system-browser preference exactly like the default
 *  xterm path; absent, links open via openHttpLink (worktree-scoped or, with no
 *  worktree, the system browser). Kept optional so the controller signature stays
 *  backward-compatible for callers that don't thread link context. */
export type AtermLinkContext = {
  worktreeId?: string | null
  requestOpenLinksInAppPreference?: (url: string) => boolean | Promise<boolean> | null | undefined
  // OSC-8 (kind 0) scheme routing — mirrors LinkHandlerDeps (#6880):
  worktreePath?: string
  terminalHomePath?: string | null
  /** Live pane cwd — a getter so it's read per click (split panes change cwd after bind). */
  getStartupCwd?: () => string
  /** Live pane runtime (SSH/WSL) id — per click, so remote path routing stays current. */
  getRuntimeEnvironmentId?: () => string | null
}

/** Build the URL opener the link input calls for kinds 0/1 (OSC-8 / URL). Reads
 *  the link context through a getter so a late-bound context (set after the
 *  controller exists) is honored; routes through orca's in-app/system-browser
 *  preference when present, else the worktree-scoped/system-browser fallback. */
export function createAtermUrlOpener(getContext: () => AtermLinkContext | undefined): AtermLinkOpener {
  return (url: string, opts: { forceSystemBrowser: boolean }): void => {
    const context = getContext()
    if (context?.requestOpenLinksInAppPreference) {
      openTerminalHttpLink(url, {
        worktreeId: context.worktreeId ?? '',
        forceSystemBrowser: opts.forceSystemBrowser,
        requestOpenLinksInAppPreference: context.requestOpenLinksInAppPreference
      })
      return
    }
    openHttpLink(url, {
      worktreeId: context?.worktreeId ?? undefined,
      forceSystemBrowser: opts.forceSystemBrowser
    })
  }
}

/** Build the OSC-8 (kind 0) opener: routes through the same scheme-aware
 *  handleOscLink the xterm pane uses, so file:// URIs and Windows paths reach
 *  openDetectedFilePath while http(s) keeps the in-app/system-browser
 *  preference. Unroutable schemes (mailto:, unknown) are a deliberate no-op —
 *  arbitrary custom-protocol launch is the #4384 surface, not this one. Reads
 *  the context through the getter so late binding / GPU→CPU rebuilds hold. */
export function createAtermOscLinkOpener(
  getContext: () => AtermLinkContext | undefined
): AtermOscLinkOpener {
  return (url: string, event: MouseEvent): void => {
    const context = getContext()
    handleOscLink(url, event, {
      worktreeId: context?.worktreeId ?? '',
      worktreePath: context?.worktreePath ?? '',
      startupCwd: context?.getStartupCwd?.(),
      terminalHomePath: context?.terminalHomePath,
      runtimeEnvironmentId: context?.getRuntimeEnvironmentId?.() ?? null,
      requestOpenLinksInAppPreference: context?.requestOpenLinksInAppPreference
    })
  }
}
