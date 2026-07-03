// The in-process animation drive for the engine's clockless effects (cursor glow,
// sparkle words). The engine never reads a wall clock: the host advances it by the
// rAF delta each painted frame (advance_effects), keeps rAF cadence ONLY while
// `is_effects_active()` holds, and drops back to zero scheduled work once every
// effect settles to its stable fingerprint (the engine's idle-to-zero contract).
// While settled, at most ONE timer is armed for the engine's next idle one-shot
// (settled-cat blink) via `effects_next_deadline_ms` — never a permanent rAF.
//
// The worker render path does NOT use this: its engine lives in the worker, which
// runs the same contract inside its own frame scheduler. The worker-backed term
// facade lacks the effects methods, so this drive detects that and no-ops.

/** The effects surface of the wasm engine; all optional so the worker-backed term
 *  facade (which drives effects worker-side) degrades this drive to a no-op. */
export type AtermEffectsDriveEngine = {
  advance_effects?: (dtMs: number) => void
  is_effects_active?: () => boolean
  effects_next_deadline_ms?: () => number | undefined
}

export type AtermEffectsDrive = {
  /** Call right before the frame renders: advances the effects clock by the
   *  elapsed wall time (clamped, so a long-hidden pane fast-forwards smoothly). */
  beforeFrame: () => void
  /** Call after the frame painted: re-arms the next rAF while animating, else
   *  arms the single idle one-shot timer (or nothing — zero work when settled). */
  afterFrame: () => void
  dispose: () => void
}

// One tick is clamped like the engine's own scene-tick contract (250 ms) so a
// backgrounded/suspended pane fast-forwards smoothly instead of one huge step.
const MAX_TICK_MS = 250

export function createAtermEffectsDrive(deps: {
  term: AtermEffectsDriveEngine
  scheduleDraw: () => void
  isDisposed: () => boolean
}): AtermEffectsDrive {
  const { term } = deps
  let lastTickMs: number | null = null
  let idleTimer: ReturnType<typeof setTimeout> | null = null

  const clearIdleTimer = (): void => {
    if (idleTimer !== null) {
      clearTimeout(idleTimer)
      idleTimer = null
    }
  }

  // Worker facade / older engine: no effects surface → permanent no-op drive.
  if (!term.advance_effects || !term.is_effects_active) {
    return { beforeFrame: () => undefined, afterFrame: () => undefined, dispose: () => undefined }
  }

  return {
    beforeFrame: () => {
      const now = performance.now()
      const dt = lastTickMs === null ? 0 : Math.min(now - lastTickMs, MAX_TICK_MS)
      lastTickMs = now
      // Unconditional: with no effect configured/animating this is a cheap no-op,
      // and PTY-output frames re-arm effects (a freshly typed sparkle word) exactly
      // as the engine contract expects.
      term.advance_effects?.(dt)
    },
    afterFrame: () => {
      if (deps.isDisposed()) {
        return
      }
      if (term.is_effects_active?.()) {
        clearIdleTimer()
        // Keep rAF cadence while animating; the scheduler coalesces to one frame.
        deps.scheduleDraw()
        return
      }
      // Settled: zero rAF work. Arm at most one timer for the engine's next idle
      // one-shot (focus-gated feline blink); no deadline → nothing scheduled at all.
      lastTickMs = null
      clearIdleTimer()
      const deadline = term.effects_next_deadline_ms?.()
      if (deadline !== undefined && Number.isFinite(deadline)) {
        idleTimer = setTimeout(
          () => {
            idleTimer = null
            if (deps.isDisposed()) {
              return
            }
            // The injected effects clock advanced 0 while idle, so cross the armed
            // one-shot deadline explicitly, then resume the frame loop there.
            term.advance_effects?.(deadline)
            lastTickMs = performance.now()
            deps.scheduleDraw()
          },
          Math.max(0, deadline)
        )
      }
    },
    dispose: clearIdleTimer
  }
}
