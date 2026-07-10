// The in-process animation drive for the engine's clockless effects (cursor glow,
// sparkle words). The engine never reads a wall clock: the host advances it by the
// rAF delta each painted frame (advance_effects). Frame-rate effects return no
// deadline and keep rAF cadence; rain returns its exact 12/30 Hz engine deadline.
// Once everything settles, at most one timer remains for a sparse idle one-shot.
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
  /** Call after the frame painted: arms one engine deadline when available,
   *  otherwise the next rAF while animating (or nothing when settled). */
  afterFrame: () => void
  dispose: () => void
}

// One tick is clamped like the engine's own scene-tick contract (250 ms) so a
// backgrounded/suspended pane fast-forwards smoothly instead of one huge step.
const MAX_TICK_MS = 250

function boundedElapsedMs(now: number, previous: number): number {
  return Math.min(Math.max(0, now - previous), MAX_TICK_MS)
}

export function createAtermEffectsDrive(deps: {
  term: AtermEffectsDriveEngine
  scheduleDraw: () => void
  isDisposed: () => boolean
}): AtermEffectsDrive {
  const { term } = deps
  let lastTickMs: number | null = null
  let effectsTimer: ReturnType<typeof setTimeout> | null = null

  const clearEffectsTimer = (): void => {
    if (effectsTimer !== null) {
      clearTimeout(effectsTimer)
      effectsTimer = null
    }
  }

  // Worker facade / older engine: no effects surface → permanent no-op drive.
  if (!term.advance_effects || !term.is_effects_active) {
    return { beforeFrame: () => undefined, afterFrame: () => undefined, dispose: () => undefined }
  }

  return {
    beforeFrame: () => {
      // Real input/output redraws preempt a pending rain/idle wake. The frame
      // below advances by actual elapsed time, then re-arms the exact remainder.
      clearEffectsTimer()
      const now = performance.now()
      const dt = lastTickMs === null ? 0 : boundedElapsedMs(now, lastTickMs)
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
      const deadline = term.effects_next_deadline_ms?.()
      if (deadline !== undefined && Number.isFinite(deadline)) {
        const effectsAdvancedAtMs = lastTickMs ?? performance.now()
        effectsTimer = setTimeout(
          () => {
            effectsTimer = null
            if (deps.isDisposed()) {
              return
            }
            // Charge wall time, not the requested delay: a throttled timer may
            // arrive late. Rebase here so beforeFrame charges only the following
            // callback→paint interval and never double-counts this idle span.
            const now = performance.now()
            const elapsed = boundedElapsedMs(now, effectsAdvancedAtMs)
            term.advance_effects?.(elapsed)
            lastTickMs = now
            deps.scheduleDraw()
          },
          Math.max(0, deadline)
        )
        return
      }
      if (term.is_effects_active?.()) {
        // No finite deadline means a cursor/deco effect needs display-rAF cadence.
        deps.scheduleDraw()
        return
      }
      lastTickMs = null
    },
    dispose: clearEffectsTimer
  }
}
