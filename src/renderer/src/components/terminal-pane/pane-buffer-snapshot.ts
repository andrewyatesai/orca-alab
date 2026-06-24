import type { ManagedPane } from '@/lib/pane-manager/pane-manager'

/** The minimal pane shape needed to snapshot a buffer: the aterm controller (when
 *  the pane is aterm-rendered) and the legacy xterm SerializeAddon (fallback). */
type SnapshotablePane = Pick<ManagedPane, 'serializeAddon' | 'atermController'>

/** Serialize a pane's buffer to replayable ANSI, preferring the aterm engine's
 *  native serialize. The xterm SerializeAddon is only consulted for legacy xterm
 *  panes (and goes away once xterm is fully removed). `scrollbackRows`: undefined →
 *  all history, `n` → the last n rows, `0` → viewport only — same semantics as
 *  `SerializeAddon.serialize({ scrollback })`. */
export function serializePaneBuffer(pane: SnapshotablePane, scrollbackRows?: number): string {
  if (pane.atermController) {
    return pane.atermController.serialize(scrollbackRows)
  }
  return pane.serializeAddon.serialize({ scrollback: scrollbackRows })
}

/** Scrollback HISTORY only (the main buffer's off-screen lines) — the only
 *  recoverable history when cold-restoring an alt-screen session. aterm-native;
 *  for legacy xterm panes there is no separate history channel, so it returns ''. */
export function serializePaneScrollback(pane: SnapshotablePane, maxRows?: number): string {
  return pane.atermController ? pane.atermController.serializeScrollback(maxRows) : ''
}
