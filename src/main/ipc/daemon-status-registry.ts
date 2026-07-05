import { BrowserWindow } from 'electron'

/**
 * Single source of truth for terminal-daemon availability
 * (docs/reference/daemon-staleness-ux.md §Phase 2). Fed by daemon-init's launch/restart
 * paths; read over `daemon:status:get` and pushed to every window on change.
 *
 * Kept separate from ./daemon-status (the IPC handlers) so daemon-init can
 * import this registry without creating a daemon-init ↔ handler module cycle.
 */

export type DaemonRuntimeState = 'starting' | 'running' | 'degraded-fallback' | 'failed'

export type DaemonRuntimeStatusCause =
  /** Daemon binary/process failed to launch — init threw. */
  | 'launch-failed'
  /** Startup fail-open aborted init; fresh spawns run locally without persistence. */
  | 'startup-timeout'
  /** Preserved daemon serves its old sessions but cannot spawn new PTYs. */
  | 'spawn-unhealthy'
  /** An explicit daemon restart attempt threw. */
  | 'restart-failed'

export type DaemonRuntimeStatus = {
  state: DaemonRuntimeState
  cause: DaemonRuntimeStatusCause | null
  /** Raw error detail (e.g. the launch error message) for diagnostics surfaces. */
  detail: string | null
  updatedAt: number
}

export const DAEMON_STATUS_CHANGED_CHANNEL = 'daemon:status:changed'

function makeInitialStatus(): DaemonRuntimeStatus {
  return { state: 'starting', cause: null, detail: null, updatedAt: Date.now() }
}

let currentStatus: DaemonRuntimeStatus = makeInitialStatus()

export function getDaemonRuntimeStatus(): DaemonRuntimeStatus {
  return currentStatus
}

export function setDaemonRuntimeStatus(
  state: DaemonRuntimeState,
  options?: { cause?: DaemonRuntimeStatusCause; detail?: string }
): void {
  const cause = options?.cause ?? null
  const detail = options?.detail ?? null
  const previous = currentStatus
  if (previous.state === state && previous.cause === cause && previous.detail === detail) {
    return
  }
  currentStatus = { state, cause, detail, updatedAt: Date.now() }
  broadcastDaemonRuntimeStatus(currentStatus)
}

function broadcastDaemonRuntimeStatus(status: DaemonRuntimeStatus): void {
  // Why: status writes happen from daemon init, which also runs under unit
  // tests and headless `orca serve` where no window system exists — treat
  // "no BrowserWindow" as "no listeners" instead of crashing the write.
  if (typeof BrowserWindow?.getAllWindows !== 'function') {
    return
  }
  for (const window of BrowserWindow.getAllWindows()) {
    if (!window.isDestroyed()) {
      window.webContents.send(DAEMON_STATUS_CHANGED_CHANNEL, status)
    }
  }
}

export function resetDaemonRuntimeStatusForTest(): void {
  currentStatus = makeInitialStatus()
}
