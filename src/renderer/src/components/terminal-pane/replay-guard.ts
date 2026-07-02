import type { ManagedPane } from '@/lib/pane-manager/pane-manager'
import { writeForegroundTerminalChunk } from '@/lib/pane-manager/pane-terminal-foreground-render-settle'
import { mirrorOutputToAterm } from '@/lib/pane-manager/aterm/aterm-output-mirror'

// Why: xterm.js auto-responds to terminal query sequences (DA1 `CSI c`,
// DECRQM `CSI ? Ps $ p`, OSC 10/11 color queries, focus events, CPR) by
// emitting the reply through its onData callback. In pty-connection.ts that
// callback is wired directly to `transport.sendInput`, which pipes the reply
// to the shell's stdin. When we restore terminal state at startup or on
// reattach we write recorded PTY bytes back into xterm — including any
// queries the previous agent CLI emitted — and the auto-replies end up as
// stray characters on the new shell's prompt (e.g. `?1;2c`, `2026;2$y`,
// OSC 10/11 color fragments).
//
// xterm does not expose a `wasUserInput` flag on its public onData, so we
// cannot distinguish replay-induced replies from real keystrokes after the
// fact. Instead, we track an in-flight replay counter per pane: callers
// replay into xterm via `replayIntoTerminal`, which increments the counter,
// writes, and decrements in xterm's write-completion callback. The onData
// handler in pty-connection.ts drops data while the counter is non-zero.
//
// The guard window is bounded by real parse completion — the write callback
// plus the aterm controller's settle() fence (the worker engine parses in a
// later task than the write ack) — not a wall-clock timer, so only replies
// generated while parsing the replayed bytes are suppressed. User keystrokes
// typed after the replay completes are unaffected. In practice replay finishes
// within milliseconds — before the user could meaningfully type — so the
// few-ms window where real input would also be dropped is acceptable relative
// to correctness.

export type ReplayingPanesRef = React.RefObject<Map<number, number>>

export function isPaneReplaying(ref: ReplayingPanesRef, paneId: number): boolean {
  return (ref.current.get(paneId) ?? 0) > 0
}

function releaseReplayGuard(map: Map<number, number>, paneId: number): void {
  const remaining = (map.get(paneId) ?? 1) - 1
  if (remaining <= 0) {
    map.delete(paneId)
  } else {
    map.set(paneId, remaining)
  }
}

/** Why the settle fence: the facade acks writes synchronously, but the default
 *  worker engine parses the posted bytes in a LATER task — auto-replies (DA/CPR)
 *  from replayed queries would land after a synchronous decrement and leak into
 *  the live PTY as stray input. controller.settle() resolves only after the
 *  engine parsed everything fed before it (replies already delivered and
 *  dropped); panes without a controller (pre-attach) release synchronously,
 *  matching the facade's own synchronous pre-attach buffering. */
function releaseWhenEngineSettles(
  pane: ManagedPane,
  map: Map<number, number>,
  onReleased?: () => void
): void {
  const settled = pane.atermController?.settle()
  if (!settled) {
    releaseReplayGuard(map, pane.id)
    onReleased?.()
    return
  }
  void settled.finally(() => {
    releaseReplayGuard(map, pane.id)
    onReleased?.()
  })
}

/** Writes `data` into the pane's terminal with the replay guard engaged,
 *  so xterm's auto-replies to embedded query sequences do not leak to the
 *  shell as input. The counter increments/decrements so nested replays
 *  (e.g. clear-screen preamble + snapshot body) compose correctly. */
export function replayIntoTerminal(
  pane: ManagedPane,
  replayingPanesRef: ReplayingPanesRef,
  data: string
): void {
  if (!data) {
    return
  }
  const map = replayingPanesRef.current
  map.set(pane.id, (map.get(pane.id) ?? 0) + 1)
  // Also paint the restored bytes onto the aterm canvas if this pane is aterm-
  // rendered (snapshot/reattach/cold-restore only fed xterm before, leaving the
  // visible canvas stale on reconnect). Raw PTY bytes, so safe to process; this
  // runs synchronously while the replay counter is up, so aterm's drained query
  // replies are dropped by the same onData guard as xterm's. No-op for xterm panes.
  mirrorOutputToAterm(pane.terminal, data)
  // Why: hidden/snapshot replay bypasses the live foreground write path, but
  // WebGL/canvas renderers still need a post-parse repaint to drop stale cells.
  writeForegroundTerminalChunk(pane.terminal, data, {
    forceViewportRefresh: true,
    followupViewportRefresh: true,
    onParsed: () => releaseWhenEngineSettles(pane, map)
  })
}

export function replayIntoTerminalAsync(
  pane: ManagedPane,
  replayingPanesRef: ReplayingPanesRef,
  data: string
): Promise<void> {
  if (!data) {
    return Promise.resolve()
  }
  const map = replayingPanesRef.current
  map.set(pane.id, (map.get(pane.id) ?? 0) + 1)
  // Mirror the restored bytes to the aterm canvas too (see replayIntoTerminal).
  mirrorOutputToAterm(pane.terminal, data)
  return new Promise((resolve) => {
    writeForegroundTerminalChunk(pane.terminal, data, {
      forceViewportRefresh: true,
      followupViewportRefresh: true,
      onParsed: () => releaseWhenEngineSettles(pane, map, resolve)
    })
  })
}
