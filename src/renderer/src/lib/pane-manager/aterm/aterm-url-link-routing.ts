import { openHttpLink } from '../../http-link-routing'
import { openTerminalHttpLink } from '../../../components/terminal-pane/terminal-url-link-hit-testing'
import type { AtermLinkOpener } from './aterm-link-input'

/** Optional pane-scoped link routing context. When supplied, terminal URL
 *  clicks honor orca's in-app/system-browser preference exactly like the default
 *  xterm path; absent, links open via openHttpLink (worktree-scoped or, with no
 *  worktree, the system browser). Kept optional so the controller signature stays
 *  backward-compatible for callers that don't thread link context. */
export type AtermLinkContext = {
  worktreeId?: string | null
  requestOpenLinksInAppPreference?: (url: string) => boolean | Promise<boolean> | null | undefined
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
