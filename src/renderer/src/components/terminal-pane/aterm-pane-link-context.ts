import type { AtermLinkContext } from '@/lib/pane-manager/aterm/aterm-url-link-routing'
import type { TerminalLinkRoutingPreferenceRequester } from './terminal-url-link-hit-testing'

export type AtermPaneLinkContextDeps = {
  worktreeId: string
  worktreePath: string
  terminalHomePath?: string | null
  requestOpenLinksInAppPreference?: TerminalLinkRoutingPreferenceRequester
  /** Live pane cwd resolver (paneCwd cache → lifecycle startupCwd fallback). */
  getPaneLinkCwd: (paneId: number) => string
  getRuntimeEnvironmentIdForPane?: (paneId: number) => string | null
}

/** The pane-scoped link context the lifecycle binds onto the aterm controller:
 *  worktree/home identity for scheme-aware OSC-8 routing plus per-click getters
 *  for cwd and runtime — getters, not values, because split panes change cwd
 *  (OSC 7) and runtimes attach after the context is bound (#6880). */
export function buildAtermPaneLinkContext(
  deps: AtermPaneLinkContextDeps,
  paneId: number
): AtermLinkContext {
  return {
    worktreeId: deps.worktreeId,
    worktreePath: deps.worktreePath,
    terminalHomePath: deps.terminalHomePath,
    requestOpenLinksInAppPreference: deps.requestOpenLinksInAppPreference,
    getStartupCwd: () => deps.getPaneLinkCwd(paneId),
    getRuntimeEnvironmentId: () => deps.getRuntimeEnvironmentIdForPane?.(paneId) ?? null
  }
}
