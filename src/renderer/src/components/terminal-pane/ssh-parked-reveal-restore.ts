/**
 * One-shot reveal-restore notes for parked `ssh:` panes.
 *
 * Why: a revealed parked ssh pane must repaint from main's model snapshot, not
 * the relay's 100 KB attach tail — and the gate's `unhide` restore marker can
 * beat the reattach round trip, landing before the pane registers its
 * pty:modelRestoreNeeded handler (markers are not buffered pre-registration;
 * ssh-pane-parking.md Critic note 2). The watcher teardown records the reveal
 * here and the mounting pane consumes it after reattach, so the restore never
 * depends on marker timing.
 */
const revealRestorePtyIds = new Set<string>()

export function noteSshParkedPaneRevealRestore(ptyId: string): void {
  revealRestorePtyIds.add(ptyId)
}

/** One-shot: true exactly once per recorded reveal for this PTY. */
export function consumeSshParkedPaneRevealRestore(ptyId: string | null): boolean {
  if (!ptyId) {
    return false
  }
  return revealRestorePtyIds.delete(ptyId)
}

/** Test seam: reset module state between tests. */
export function _resetSshParkedPaneRevealRestoreForTest(): void {
  revealRestorePtyIds.clear()
}
