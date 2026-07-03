// Worker-side effects clock (the engine is clockless; the host owns time). One
// instance per worker terminal: dt = wall delta between ticked frames, clamped
// like the engine's own 250 ms scene-tick contract so a long gap fast-forwards
// smoothly instead of one huge kinematic step.

type EffectsEngine = {
  advance_effects: (dtMs: number) => void
  is_effects_active: () => boolean
  effects_next_deadline_ms: () => number | undefined
}

const MAX_EFFECTS_TICK_MS = 250

export type AtermWorkerEffectsTick = {
  /** Advance pre-render; true while an effect is still animating (keep rAF
   *  cadence), false once settled — the engine's idle-to-zero contract. */
  tick: () => boolean
  /** Ms until the engine's next idle one-shot (settled-cat blink), or undefined. */
  idleDeadlineMs: () => number | undefined
  /** Cross an armed idle deadline on the injected clock (timer-fired frames). */
  advanceBy: (dtMs: number) => void
}

export function createAtermWorkerEffectsTick(e: EffectsEngine): AtermWorkerEffectsTick {
  let lastTickMs: number | null = null
  return {
    tick: () => {
      const now = performance.now()
      const dt = lastTickMs === null ? 0 : Math.min(now - lastTickMs, MAX_EFFECTS_TICK_MS)
      lastTickMs = now
      // Unconditional advance: a no-op while nothing is enabled/animating, and
      // output frames re-arm effects (a freshly printed sparkle word) as the
      // engine contract expects.
      e.advance_effects(dt)
      const active = e.is_effects_active()
      if (!active) {
        lastTickMs = null
      }
      return active
    },
    idleDeadlineMs: () => e.effects_next_deadline_ms(),
    advanceBy: (dtMs) => {
      e.advance_effects(dtMs)
      lastTickMs = performance.now()
    }
  }
}
