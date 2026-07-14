import type { ManagedPane } from '@/lib/pane-manager/pane-manager'
import { writeForegroundTerminalChunk } from '@/lib/pane-manager/pane-terminal-foreground-render-settle'
import { mirrorOutputToAterm } from '@/lib/pane-manager/aterm/aterm-output-mirror'
import { recordRendererCrashBreadcrumb } from '@/lib/crash-breadcrumb-recorder'

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

// Why stall handling exists: the decrement above only runs when the write
// completion fires (and, on aterm, only after the worker engine then settles).
// A wedged WriteBuffer (sync throw escaping a parse handler or a
// write-completion callback — see xterm-write-buffer-stall.repro.test.ts), a
// hung engine settle, or a disposed-terminal race can drop that completion
// forever, leaving the guard latched on a live pane — which silently eats every
// keystroke (Discord #performance / issue #2836).
//
// Why release is probe-certified, never time-based: a blind timeout release
// while a slow replay is still parsing would let the terminal's auto-replies
// leak into the shell — and into agent TUIs, where a leaked ESC reads as the
// user pressing Escape. Instead, when a completion looks overdue we enqueue an
// empty probe write. Writes parse in order, so only three states are possible,
// and release is provably safe in every state that releases:
//   1. probe completes, replay callback already ran   → normal release won.
//   2. probe completes, replay callback never ran     → every replay byte has
//      parsed (FIFO), so no further auto-replies can exist; the completion
//      was genuinely lost. Release.
//   3. probe never completes                          → the pipeline is
//      wedged; a dead parser can never emit auto-replies, so releasing after
//      a bounded wait cannot leak anything — and the pane needs recovery,
//      which the breadcrumb reports.
// While the probe is pending (slow-but-alive replay), the guard HOLDS.
const REPLAY_GUARD_STALL_CHECK_MS = 10_000

type ReplayTerminalOptions = {
  shouldRefreshViewportSynchronously?: () => boolean
  stallCheckMs?: number
}

export function isPaneReplaying(ref: ReplayingPanesRef, paneId: number): boolean {
  return (ref.current.get(paneId) ?? 0) > 0
}

/**
 * Engage the replay counter for one write and return the release function.
 * Release runs exactly once — from the write completion (gated on the aterm
 * worker settling, see below) or, failing that, from the probe-certified stall
 * path — so a lost completion or a wedged pipeline cannot latch the guard.
 */
function engageReplayGuard(
  pane: ManagedPane,
  map: Map<number, number>,
  stallCheckMs: number,
  onRelease?: () => void
): () => void {
  const paneId = pane.id
  const terminal = pane.terminal
  map.set(paneId, (map.get(paneId) ?? 0) + 1)
  let released = false
  let timer: ReturnType<typeof setTimeout> | null = null
  const release = (reason: 'parsed' | 'lost-completion' | 'wedged'): void => {
    if (released) {
      return
    }
    released = true
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
    const remaining = (map.get(paneId) ?? 1) - 1
    if (remaining <= 0) {
      map.delete(paneId)
    } else {
      map.set(paneId, remaining)
    }
    if (reason === 'lost-completion') {
      console.error(
        `[terminal] replay guard released for pane ${paneId} — the probe write parsed but the replay completion never arrived (lost write callback)`
      )
      recordRendererCrashBreadcrumb('terminal_replay_guard_lost_completion', { paneId })
    } else if (reason === 'wedged') {
      console.error(
        `[terminal] replay guard released for pane ${paneId} — the probe write never parsed (wedged terminal write pipeline; pane likely needs recovery)`
      )
      recordRendererCrashBreadcrumb('terminal_replay_guard_wedged_release', { paneId })
    }
    onRelease?.()
  }
  // Why the settle fence: the aterm facade acks writes synchronously, but the
  // default worker engine parses the posted bytes in a LATER task — auto-replies
  // (DA/CPR) from replayed queries would land after a synchronous decrement and
  // leak into the live PTY as stray input. settle() resolves only after the
  // engine parsed everything fed before it (replies already delivered and
  // dropped); panes without a controller (pre-attach) release synchronously,
  // matching the facade's own synchronous pre-attach buffering. The stall timer
  // stays armed across the settle wait, so a hung engine can't latch the guard.
  const releaseWhenEngineSettles = (): void => {
    const settled = pane.atermController?.settle()
    if (!settled) {
      release('parsed')
      return
    }
    void settled.finally(() => release('parsed'))
  }
  const probeForStall = (): void => {
    if (released) {
      return
    }
    try {
      // FIFO certification: this callback can only run after every replay byte
      // queued before it has parsed (state 2 above).
      terminal.write('', () => release('lost-completion'))
    } catch {
      // write threw (terminal disposed mid-replay): nothing will ever parse,
      // so no auto-replies can leak.
      release('wedged')
      return
    }
    timer = setTimeout(() => release('wedged'), stallCheckMs)
  }
  timer = setTimeout(probeForStall, stallCheckMs)
  return releaseWhenEngineSettles
}

/** Writes `data` into the pane's terminal with the replay guard engaged,
 *  so xterm's auto-replies to embedded query sequences do not leak to the
 *  shell as input. The counter increments/decrements so nested replays
 *  (e.g. clear-screen preamble + snapshot body) compose correctly. */
export function replayIntoTerminal(
  pane: ManagedPane,
  replayingPanesRef: ReplayingPanesRef,
  data: string,
  options: ReplayTerminalOptions = {}
): void {
  if (!data) {
    return
  }
  const releaseParsed = engageReplayGuard(
    pane,
    replayingPanesRef.current,
    options.stallCheckMs ?? REPLAY_GUARD_STALL_CHECK_MS
  )
  // Also paint the restored bytes onto the aterm canvas if this pane is aterm-
  // rendered (snapshot/reattach/cold-restore only fed xterm before, leaving the
  // visible canvas stale on reconnect). Raw PTY bytes, so safe to process; this
  // runs while the replay counter is up, so aterm's drained query replies are
  // dropped by the same onData guard. No-op for non-aterm terminals.
  mirrorOutputToAterm(pane.terminal, data)
  // Why: hidden/snapshot replay bypasses the live foreground write path, but
  // WebGL/canvas renderers still need a post-parse repaint to drop stale cells.
  writeForegroundTerminalChunk(pane.terminal, data, {
    forceViewportRefresh: true,
    followupViewportRefresh: true,
    shouldRefreshViewportSynchronously: options.shouldRefreshViewportSynchronously,
    onParsed: releaseParsed
  })
}

export function replayIntoTerminalAsync(
  pane: ManagedPane,
  replayingPanesRef: ReplayingPanesRef,
  data: string,
  options: ReplayTerminalOptions = {}
): Promise<void> {
  if (!data) {
    return Promise.resolve()
  }
  return new Promise((resolve) => {
    // Why resolve on either release path: callers await this to sequence restore
    // steps; a lost write completion or wedged pipeline must not hang the chain.
    const releaseParsed = engageReplayGuard(
      pane,
      replayingPanesRef.current,
      options.stallCheckMs ?? REPLAY_GUARD_STALL_CHECK_MS,
      resolve
    )
    // Mirror the restored bytes to the aterm canvas too (see replayIntoTerminal).
    mirrorOutputToAterm(pane.terminal, data)
    writeForegroundTerminalChunk(pane.terminal, data, {
      forceViewportRefresh: true,
      followupViewportRefresh: true,
      shouldRefreshViewportSynchronously: options.shouldRefreshViewportSynchronously,
      onParsed: releaseParsed
    })
  })
}
