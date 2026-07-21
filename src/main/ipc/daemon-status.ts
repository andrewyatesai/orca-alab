import { ipcMain } from 'electron'
import { getDaemonRuntimeStatus } from './daemon-status-registry'
import { getDaemonProvider, initDaemonPtyProvider, restartDaemon } from '../daemon/daemon-init'

/**
 * IPC surface for the daemon-status registry
 * (docs/reference/daemon-staleness-ux.md §Phase 2): read the current status and retry a
 * failed/degraded daemon. Registered alongside the pty:management handlers so
 * it rides the same re-registration lifecycle on window recreation.
 */

export type RelaunchDaemonResult = { success: boolean; error?: string }

// Why: coalesce concurrent relaunch requests (toast Retry racing the settings
// button) onto one in-flight attempt, mirroring restartDaemon's coalescer.
let relaunchInFlight: Promise<RelaunchDaemonResult> | null = null

export async function relaunchDaemonForRecovery(): Promise<RelaunchDaemonResult> {
  if (relaunchInFlight) {
    return relaunchInFlight
  }
  relaunchInFlight = runRelaunch().finally(() => {
    relaunchInFlight = null
  })
  return relaunchInFlight
}

async function runRelaunch(): Promise<RelaunchDaemonResult> {
  try {
    const state = getDaemonRuntimeStatus().state
    if (state === 'running') {
      // Why: a Retry click can race a background recovery; never restart a
      // healthy daemon (and kill its sessions) from this recovery path.
      return { success: true }
    }
    if (state === 'starting' && !getDaemonProvider()) {
      // Why: an init attempt is already in flight (e.g. the settings Restart
      // button clicked during startup); racing a second init would
      // double-spawn daemons. The registry transition announces the outcome.
      return { success: true }
    }
    if (getDaemonProvider()) {
      // Degraded provider installed — the Phase 1 restart sequence already
      // handles fallback-session shutdown and synthetic exits.
      await restartDaemon()
      return { success: true }
    }
    // Total launch failure: no daemon provider was ever installed, so fresh
    // terminals spawned on the in-process LocalPtyProvider. Kill them through
    // the still-bound local provider BEFORE re-running init: the provider swap
    // at the end of initDaemonPtyProvider re-routes PTY IPC by id, and ids the
    // daemon provider doesn't know would black-hole writes. Killing first
    // delivers real pty:exit events, matching the restart flow's
    // "panes show Process exited" contract.
    //
    // Why dynamic import: ./pty is the heavy PTY IPC module (native deps);
    // loading it lazily keeps this status module importable by lightweight
    // consumers and unit tests without stubbing the whole PTY surface.
    const { getLocalPtyProvider } = await import('./daemon-status-pty-deferred')
    const local = getLocalPtyProvider()
    const processes = await local.listProcesses()
    await Promise.allSettled(processes.map((proc) => local.shutdown(proc.id, { immediate: true })))
    await initDaemonPtyProvider()
    return { success: true }
  } catch (error) {
    // Why: initDaemonPtyProvider/restartDaemon already report 'failed' into the
    // registry on throw; here we only shape the invoke() reply for the caller.
    return { success: false, error: error instanceof Error ? error.message : String(error) }
  }
}

export function registerDaemonStatusHandlers(): void {
  ipcMain.removeHandler('daemon:status:get')
  ipcMain.removeHandler('daemon:status:relaunch')

  ipcMain.handle('daemon:status:get', () => getDaemonRuntimeStatus())
  ipcMain.handle('daemon:status:relaunch', () => relaunchDaemonForRecovery())
}
