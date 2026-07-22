/**
 * Park snapshot-class resolution: which authority can restore a parked pane
 * (ssh-pane-parking.md §3.1). Split from terminal-hidden-view-parking.ts so the
 * hysteresis/selection policy module stays focused on park timing.
 */
import { isRemoteRuntimePtyId } from '@/runtime/runtime-terminal-inspection'
import { PTY_SESSION_ID_SEPARATOR } from '../../../../shared/pty-session-id-format'
import { parseAppSshPtyId } from '../../../../shared/ssh-pty-id'

export type TerminalParkSnapshotClass = 'daemon' | 'ssh-main-model' | 'remote-wire'

export function terminalPtyParkSnapshotClass(
  ptyId: string | null,
  worktreeId: string
): TerminalParkSnapshotClass | null {
  if (!ptyId) {
    return null
  }
  if (isRemoteRuntimePtyId(ptyId)) {
    return 'remote-wire'
  }
  if (parseAppSshPtyId(ptyId)) {
    return 'ssh-main-model'
  }
  // Why: separator-less ids come from the daemon-fail-open LocalPtyProvider;
  // they have no daemon session model, so revealing a parked pane would
  // silently respawn a fresh shell instead of restoring the snapshot.
  const separatorIdx = ptyId.lastIndexOf(PTY_SESSION_ID_SEPARATOR)
  return separatorIdx !== -1 && ptyId.slice(0, separatorIdx) === worktreeId ? 'daemon' : null
}

export type SnapshotBackedTerminalPtyOptions = {
  /** Callers pass settings.terminalRemotePaneParking !== false — the scoped
   *  remote-class kill switch; the global terminalHiddenViewParking still dominates. */
  remoteParkingEnabled?: boolean
}

export function isSnapshotBackedTerminalPty(
  ptyId: string | null,
  worktreeId: string,
  opts?: SnapshotBackedTerminalPtyOptions
): boolean {
  const snapshotClass = terminalPtyParkSnapshotClass(ptyId, worktreeId)
  if (snapshotClass === 'daemon') {
    return true
  }
  // Why: ssh bytes transit local main's headless model, so reveal restores from
  // pty:getMainBufferSnapshot — parkable once the remote-class switch is on.
  if (snapshotClass === 'ssh-main-model') {
    return opts?.remoteParkingEnabled === true
  }
  // Why: remote-wire reveal needs the phase-2 watcher byte source and exit
  // classification (Wave 4); keep the class excluded until that lands.
  return false
}
