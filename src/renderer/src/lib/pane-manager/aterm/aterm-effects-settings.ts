import { useAppStore } from '@/store'
import type { TerminalCursorGlowStyle } from '../../../../../shared/types'

// The aterm effects settings surface: one host-side config snapshot (read live from
// the store, like the other engine-settings readers) and one applier that maps it
// onto the engine's effects setters. Shared by BOTH render paths — in-process it
// targets the real engine, on the worker path the same calls hit the worker-backed
// term facade, which posts the equivalent commands across the seam.

/** The engine effects setters both wasm bindings (and the worker facade) expose. */
export type AtermEffectsTarget = {
  set_sparkle_words_enabled: (on: boolean) => void
  set_sparkle_classes: (
    profanity: boolean,
    feline: boolean,
    orca: boolean,
    emphasis: boolean
  ) => void
  set_sparkle_reduced_motion: (on: boolean) => void
  set_cursor_glow: (
    enabled: boolean,
    style: string,
    color: number | null | undefined,
    accent: number | null | undefined,
    duration_ms: number,
    length: number,
    intensity: number,
    radius: number,
    ring: boolean
  ) => void
}

export type AtermEffectsConfig = {
  /** Master sparkle-words switch (terminalEffectsSparkleWords); default false. */
  sparkleWords: boolean
  sparkleProfanity: boolean
  sparkleFeline: boolean
  sparkleOrca: boolean
  sparkleEmphasis: boolean
  /** Cursor aurora (terminalEffectsCursorGlow); default false. */
  cursorGlow: boolean
  cursorGlowStyle: TerminalCursorGlowStyle
  /** OS accessibility gate (prefers-reduced-motion), read at apply time. */
  reducedMotion: boolean
}

// The native app's cursor-trail defaults (aterm-gui app_config *_or_default):
// fade 260ms, comet length 24 cells, intensity 0.7, bloom crown 0.6 cells,
// landing-ring on. color/accent stay unset so the engine derives them from the
// theme cursor exactly like the native app.
export const ATERM_CURSOR_GLOW_DEFAULTS = {
  durationMs: 260,
  lengthCells: 24,
  intensity: 0.7,
  radiusCells: 0.6,
  ring: true
} as const

/** Live OS reduce-motion preference (no listener — re-read on every apply). */
export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false
}

/** Read the live effects config from the settings store (defaults = engine OFF). */
export function readAtermEffectsConfig(): AtermEffectsConfig {
  const settings = useAppStore.getState().settings
  return {
    sparkleWords: settings?.terminalEffectsSparkleWords ?? false,
    sparkleProfanity: settings?.terminalEffectsSparkleProfanity ?? true,
    sparkleFeline: settings?.terminalEffectsSparkleFeline ?? true,
    sparkleOrca: settings?.terminalEffectsSparkleOrca ?? true,
    sparkleEmphasis: settings?.terminalEffectsSparkleEmphasis ?? true,
    cursorGlow: settings?.terminalEffectsCursorGlow ?? false,
    // 'water' is Orca's native trail: a self-contained ORCA_PALETTE ocean ramp
    // that reads on any theme, unlike 'lumen' additive white (white-on-white on
    // light backgrounds). Only fires when settings are wholly unloaded.
    cursorGlowStyle: settings?.terminalEffectsCursorGlowStyle ?? 'water',
    reducedMotion: prefersReducedMotion()
  }
}

/** Map a config snapshot onto the engine. Everything OFF is a no-op by the engine's
 *  default-off contract, so a pane with effects disabled renders byte-identically. */
export function applyAtermEffectsConfig(
  term: AtermEffectsTarget,
  cfg: AtermEffectsConfig,
  cursorColor?: number
): void {
  term.set_sparkle_words_enabled(cfg.sparkleWords)
  term.set_sparkle_classes(
    cfg.sparkleProfanity,
    cfg.sparkleFeline,
    cfg.sparkleOrca,
    cfg.sparkleEmphasis
  )
  // The engine's reduced-motion path keeps sparkle decorations but makes them static
  // (its flash-limiter floors always apply); the glow is pure motion, so the host
  // gates it fully off under the OS preference.
  term.set_sparkle_reduced_motion(cfg.reducedMotion)
  applyAtermCursorGlowConfig(term, cfg, cursorColor)
}

/** Apply ONLY the cursor-glow config: `cursorColor` is the live OSC 12 override
 *  (engine cursor_color); unset → the engine derives the colour from the theme
 *  cursor exactly like the native app. Accent always stays theme-derived. */
export function applyAtermCursorGlowConfig(
  term: Pick<AtermEffectsTarget, 'set_cursor_glow'>,
  cfg: AtermEffectsConfig,
  cursorColor?: number
): void {
  const d = ATERM_CURSOR_GLOW_DEFAULTS
  term.set_cursor_glow(
    cfg.cursorGlow && !cfg.reducedMotion,
    cfg.cursorGlowStyle,
    cursorColor ?? undefined,
    undefined,
    d.durationMs,
    d.lengthCells,
    d.intensity,
    d.radiusCells,
    d.ring
  )
}
