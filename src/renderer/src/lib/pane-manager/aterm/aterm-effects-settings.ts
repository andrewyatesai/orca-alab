import { useAppStore } from '@/store'
import type { TerminalCursorGlowStyle } from '../../../../../shared/types'
import type { AtermMatrixRainTarget } from './aterm-matrix-rain-types'
import {
  setAtermCursorGlowActivity,
  setAtermMatrixRainActivity
} from './aterm-effects-activity-gate'

// The aterm effects settings surface: one host-side config snapshot (read live from
// the store, like the other engine-settings readers) and one applier that maps it
// onto the engine's effects setters. Shared by BOTH render paths — in-process it
// targets the real engine, on the worker path the same calls hit the worker-backed
// term facade, which posts the equivalent commands across the seam.

/** The engine effects setters both wasm bindings (and the worker facade) expose. */
export type AtermEffectsTarget = AtermMatrixRainTarget & {
  /** Device-px cell height (both engines + the worker facade expose the getter);
   *  sizes the window-space chrome for effects that escape the grid. */
  readonly cell_height: number
  /** Window-space effects chrome (pad per edge + top head band, device px).
   *  Optional while orc and aterm generated artifacts roll independently. */
  set_chrome?: (pad: number, head: number) => void
  /** Set by the pane wiring on real render paths only: their drawers pin the
   *  canvas box WITH chrome offsets (worker loader, frame painter, GPU drawer).
   *  Bare engines (e.g. the settings demo) have no offset handling — no marker. */
  windowChromeCapable?: boolean
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
  /** Master sparkle-words switch (terminalEffectsSparkleWords); default true. */
  sparkleWords: boolean
  sparkleProfanity: boolean
  sparkleFeline: boolean
  sparkleOrca: boolean
  sparkleEmphasis: boolean
  /** Literal, output-derived PHOSPHOR rain; default false. */
  matrixRain: boolean
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

/** Orca's single rain profile. Bounds mirror aterm's native defaults; the
 *  literal material bank is intentional: rain is made from supported codepoints
 *  in the authoritative aterm grid, never a decorative substitute alphabet. */
export const ATERM_MATRIX_RAIN_DEFAULTS = {
  fps: 30,
  density: 6,
  speed: 5,
  trail: 5,
  alpha: undefined,
  headAlpha: undefined,
  hue: 'theme',
  hueColor: undefined,
  mutationMs: 133,
  idleSecs: 8,
  suppressInAltScreen: false,
  turnWave: true,
  // Orca's independent bell detector is not yet bridged into this engine seam.
  bellAlert: false,
  outputMaterial: true,
  seed: 0n
} as const

/** Live OS reduce-motion preference (no listener — re-read on every apply). */
export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-reduced-motion: reduce)').matches
    : false
}

/** Read the live effects config from the settings store. Missing keys represent
 *  pre-feature profiles and must inherit the same defaults as new profiles. */
export function readAtermEffectsConfig(): AtermEffectsConfig {
  const settings = useAppStore.getState().settings
  return {
    sparkleWords: settings?.terminalEffectsSparkleWords ?? true,
    sparkleProfanity: settings?.terminalEffectsSparkleProfanity ?? true,
    sparkleFeline: settings?.terminalEffectsSparkleFeline ?? true,
    sparkleOrca: settings?.terminalEffectsSparkleOrca ?? true,
    sparkleEmphasis: settings?.terminalEffectsSparkleEmphasis ?? true,
    matrixRain: settings?.terminalMatrixRainEnabled ?? false,
    cursorGlow: settings?.terminalEffectsCursorGlow ?? true,
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
  applyAtermMatrixRainConfig(term, cfg)
  applyAtermCursorGlowConfig(term, cfg, cursorColor)
}

/** Apply PHOSPHOR's retained config before the master switch. This keeps OFF
 *  zero-cost while ensuring the first enabled frame is already literal/theme-aware. */
export function applyAtermMatrixRainConfig(
  term: Pick<
    AtermEffectsTarget,
    'set_matrix_rain' | 'set_matrix_rain_enabled' | 'set_matrix_rain_reduced_motion'
  >,
  cfg: AtermEffectsConfig
): void {
  const d = ATERM_MATRIX_RAIN_DEFAULTS
  term.set_matrix_rain(
    d.fps,
    d.density,
    d.speed,
    d.trail,
    d.alpha,
    d.headAlpha,
    d.hue,
    d.hueColor,
    d.mutationMs,
    d.idleSecs,
    d.suppressInAltScreen,
    d.turnWave,
    d.bellAlert,
    d.outputMaterial,
    d.seed
  )
  term.set_matrix_rain_reduced_motion(cfg.reducedMotion)
  term.set_matrix_rain_enabled(cfg.matrixRain)
  setAtermMatrixRainActivity(term, cfg.matrixRain && !cfg.reducedMotion)
}

/** Apply ONLY the cursor-glow config: `cursorColor` is the live OSC 12 override
 *  (engine cursor_color); unset → the engine derives the colour from the theme
 *  cursor exactly like the native app. Accent always stays theme-derived. */
export function applyAtermCursorGlowConfig(
  term: Pick<
    AtermEffectsTarget,
    'set_cursor_glow' | 'cell_height' | 'set_chrome' | 'windowChromeCapable'
  >,
  cfg: AtermEffectsConfig,
  cursorColor?: number
): void {
  const d = ATERM_CURSOR_GLOW_DEFAULTS
  const enabled = cfg.cursorGlow && !cfg.reducedMotion
  term.set_cursor_glow(
    enabled,
    cfg.cursorGlowStyle,
    cursorColor ?? undefined,
    undefined,
    d.durationMs,
    d.lengthCells,
    d.intensity,
    d.radiusCells,
    d.ring
  )
  setAtermCursorGlowActivity(term, enabled)
  // Both apply paths (full effects config + glow-only) end here, so chrome
  // follows every style/enable/reduced-motion change through one seam.
  applyAtermWindowChrome(term, cfg)
}

/** Give ANY active cursor-glow style window-space chrome: every glow/trail
 *  emission (fire flames, water droplets, lumen bloom, ...) escapes the cell box
 *  and clips at the frame edge without a head band (~2 cells) plus a breathing
 *  pad. Gated via `windowChromeCapable` (only render paths whose drawers offset
 *  the canvas box); glow off / reduced motion sets 0/0, so effects-off rendering
 *  stays byte-identical. */
export function applyAtermWindowChrome(
  term: Pick<AtermEffectsTarget, 'cell_height' | 'set_chrome' | 'windowChromeCapable'>,
  cfg: AtermEffectsConfig
): void {
  if (term.windowChromeCapable !== true || typeof term.set_chrome !== 'function') {
    return
  }
  const glowing = cfg.cursorGlow && !cfg.reducedMotion
  const ch = term.cell_height
  if (glowing && ch > 0) {
    term.set_chrome(Math.ceil(ch * 0.75), Math.ceil(ch * 2))
  } else {
    term.set_chrome(0, 0)
  }
}

/** Wire window chrome for a REAL render-path engine — only the pane wiring may
 *  call this: every real drawer pins the canvas box WITH chrome offsets (worker
 *  loader, frame painter, GPU drawer), while bare engines (the settings demo)
 *  must stay unmarked. Returns the wiring's cell-metrics-change hook: run the
 *  given dependents sync, then re-derive chrome from the LIVE config — chrome is
 *  sized from cell_height at apply time, so a font-size/line-height/dpr/
 *  primary-font change would otherwise leave stale headroom. */
export function wireAtermWindowChrome(
  term: Pick<AtermEffectsTarget, 'cell_height' | 'set_chrome' | 'windowChromeCapable'>,
  syncDependents: () => void
): () => void {
  term.windowChromeCapable = true
  return () => {
    syncDependents()
    applyAtermWindowChrome(term, readAtermEffectsConfig())
  }
}
