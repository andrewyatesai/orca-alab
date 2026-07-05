import { useEffect, useRef } from 'react'
import { toast } from 'sonner'
import { useDaemonRuntimeStatus } from '@/lib/daemon-runtime-status-store'
import {
  getDaemonStatusRestoredMessage,
  getDaemonStatusRetryFailedTitle,
  getDaemonStatusToastCopy
} from './daemon-status-copy'
import { nextDaemonStatusToastAction } from './daemon-status-toast-transitions'

// Why: a stable id makes repeated degraded/failed transitions update one sticky
// toast in place instead of stacking duplicates, and lets recovery dismiss it.
export const DAEMON_STATUS_TOAST_ID = 'daemon-runtime-status'

function retryDaemonRelaunch(): void {
  void window.api.daemonStatus.relaunch().then((result) => {
    // Success surfaces through the registry transitions the bridge watches:
    // degraded→running celebrates with the restored toast, while the total-
    // failure path (failed→starting→running) just dismisses the sticky toast
    // at 'starting'. Only failure needs a direct reply here.
    if (!result.success) {
      toast.error(getDaemonStatusRetryFailedTitle(), {
        description: result.error ?? undefined
      })
    }
  })
}

/**
 * Mounts once at App root and mirrors the daemon-status registry into a sticky
 * toast — the "loud" half of docs/reference/daemon-staleness-ux.md §Phase 2. Mirrors the
 * "Session restore failed" pattern: duration Infinity, dismissible, one action.
 */
export function DaemonStatusToastBridge(): null {
  const status = useDaemonRuntimeStatus()
  const stickyShownRef = useRef(false)

  useEffect(() => {
    const action = nextDaemonStatusToastAction({
      stickyShown: stickyShownRef.current,
      state: status.state
    })
    if (action === 'show-sticky') {
      const copy = getDaemonStatusToastCopy(status)
      if (!copy) {
        return
      }
      stickyShownRef.current = true
      // Why: deliberately one-click (no DaemonActionDialog confirm like the
      // settings flow) — the toast only appears when persistence is already
      // lost, and the description states that restarting closes open panes.
      toast.error(copy.title, {
        id: DAEMON_STATUS_TOAST_ID,
        description: copy.description,
        duration: Infinity,
        dismissible: true,
        action: {
          label: copy.actionLabel,
          onClick: retryDaemonRelaunch
        }
      })
      return
    }
    if (action === 'dismiss-and-celebrate' || action === 'dismiss') {
      stickyShownRef.current = false
      toast.dismiss(DAEMON_STATUS_TOAST_ID)
      if (action === 'dismiss-and-celebrate') {
        toast.success(getDaemonStatusRestoredMessage())
      }
    }
  }, [status])

  return null
}
