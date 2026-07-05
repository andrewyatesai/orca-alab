import { useSyncExternalStore } from 'react'
import type { DaemonRuntimeStatus } from '../../../preload/api-types'

// Why: the daemon status has two independent consumers (sticky toast bridge,
// status-bar segment). A tiny module-level external store gives them one shared
// IPC subscription without widening the app store's already-large surface.

const INITIAL_STATUS: DaemonRuntimeStatus = {
  state: 'starting',
  cause: null,
  detail: null,
  updatedAt: 0
}

let currentStatus: DaemonRuntimeStatus = INITIAL_STATUS
let started = false
let unsubscribeIpc: (() => void) | null = null
const listeners = new Set<() => void>()

function emit(): void {
  // Why: snapshot first — a listener may unsubscribe (or subscribe) during
  // notification, and mutating the live Set mid-iteration would skip peers.
  for (const listener of Array.from(listeners)) {
    listener()
  }
}

function startIpcSubscription(): void {
  if (started) {
    return
  }
  // Why: tolerate non-preload contexts (unit tests) instead of crashing the
  // first component that renders the hook.
  const api = window.api?.daemonStatus
  if (!api) {
    return
  }
  started = true
  unsubscribeIpc = api.onChanged((status) => {
    currentStatus = status
    emit()
  })
  void api
    .get()
    .then((status) => {
      // Why: a change event can land while get() is in flight — keep whichever
      // status the main process stamped most recently.
      if (status.updatedAt >= currentStatus.updatedAt) {
        currentStatus = status
        emit()
      }
    })
    .catch(() => {
      // Handler not registered yet (window still attaching) — the change
      // subscription above still delivers the first real transition.
    })
}

export function subscribeDaemonRuntimeStatus(listener: () => void): () => void {
  startIpcSubscription()
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function getDaemonRuntimeStatusSnapshot(): DaemonRuntimeStatus {
  return currentStatus
}

export function useDaemonRuntimeStatus(): DaemonRuntimeStatus {
  return useSyncExternalStore(subscribeDaemonRuntimeStatus, getDaemonRuntimeStatusSnapshot)
}

export function resetDaemonRuntimeStatusStoreForTest(): void {
  unsubscribeIpc?.()
  unsubscribeIpc = null
  started = false
  currentStatus = INITIAL_STATUS
  listeners.clear()
}
