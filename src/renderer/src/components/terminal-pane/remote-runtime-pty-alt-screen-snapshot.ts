import type { RemoteRuntimeSerializedBufferSnapshot } from '../../runtime/remote-runtime-terminal-multiplexer'
import type { PtyBufferSnapshot } from './pty-transport-types'

// Why: the remote wire sends one combined blob (history + frame) with
// scrollbackChars marking the boundary; the restorer needs the local daemon
// shape — viewport-only `data` beside a separate `scrollbackAnsi` — or an
// alt-screen replay paints pre-TUI history into the alt buffer and the
// restore's clear wipes it (#6106).
export function splitRemoteAltScreenSnapshot(
  snapshot: RemoteRuntimeSerializedBufferSnapshot
): PtyBufferSnapshot {
  const { scrollbackChars, ...rest } = snapshot
  if (snapshot.alternateScreen !== true) {
    return rest
  }
  const boundary =
    typeof scrollbackChars === 'number' &&
    Number.isSafeInteger(scrollbackChars) &&
    scrollbackChars > 0
      ? Math.min(scrollbackChars, snapshot.data.length)
      : 0
  if (boundary === 0) {
    // No recoverable history in the snapshot; the alternateScreen flag alone
    // makes the restorer preserve whatever scrollback the pane already holds.
    return rest
  }
  return {
    ...rest,
    scrollbackAnsi: snapshot.data.slice(0, boundary),
    data: snapshot.data.slice(boundary)
  }
}
