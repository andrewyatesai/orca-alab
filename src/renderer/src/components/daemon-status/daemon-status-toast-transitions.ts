import type { DaemonRuntimeState } from '../../../../preload/api-types'

// Pure transition logic for the sticky daemon-status toast, kept separate from
// the React bridge so the show/dismiss/celebrate decisions are unit-testable.

export type DaemonStatusToastAction =
  /** Show (or update in place — the toast id is stable) the sticky toast. */
  | 'show-sticky'
  /** Dismiss the sticky toast; recovery is confirmed with a success toast. */
  | 'dismiss-and-celebrate'
  /** Dismiss the sticky toast without celebrating (e.g. a retry is starting). */
  | 'dismiss'
  | 'none'

export function isDaemonUnavailableState(state: DaemonRuntimeState): boolean {
  return state === 'failed' || state === 'degraded-fallback'
}

export function nextDaemonStatusToastAction(opts: {
  stickyShown: boolean
  state: DaemonRuntimeState
}): DaemonStatusToastAction {
  if (isDaemonUnavailableState(opts.state)) {
    return 'show-sticky'
  }
  if (!opts.stickyShown) {
    return 'none'
  }
  return opts.state === 'running' ? 'dismiss-and-celebrate' : 'dismiss'
}
