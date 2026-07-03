import { loadAtermCpuDrawer } from '@/lib/pane-manager/aterm/aterm-cpu-drawer'
import {
  applyAtermEffectsConfig,
  type AtermEffectsConfig
} from '@/lib/pane-manager/aterm/aterm-effects-settings'
import {
  applyAtermLiveTheme,
  type AtermThemeColors
} from '@/lib/pane-manager/aterm/aterm-theme-colors'

// The LIVE effects demo behind the Terminal Engine settings panel: a REAL aterm CPU
// engine (the same drawer live panes use — no fake, no video) that types a small
// script on a loop so the cursor moves (driving the glow wake) and sparkle words
// land ("orca" → splash, "kitten" → cat), while the host runs the engine's real
// animation-drive contract: advance_effects per rAF ONLY while is_effects_active(),
// idle-to-zero once settled.

/** Demo script: each entry is typed character-by-character, then newline. Words
 *  chosen from the engine's builtin lexicon classes that are safe to print
 *  (orca class: "orca"; feline class: "kitten"). The emphasis class ships an
 *  empty builtin lexicon, so no emphasis line exists to demo honestly. */
const DEMO_LINES = ['echo orca', 'echo kitten', 'ls ~/pods/orca']

const TYPE_INTERVAL_MS = 110
const LINE_PAUSE_MS = 900
// Clear + home before each script pass so the fixed demo grid never scrolls.
const CLEAR_AND_HOME = '\x1b[2J\x1b[3J\x1b[H'
// One dt step is clamped like the engine's scene-tick contract (250 ms).
const MAX_TICK_MS = 250
const PROMPT = '\x1b[32m❯\x1b[0m '

export type TerminalEngineEffectsDemoEngine = {
  /** Re-apply the live effects config (sparkle master/classes, glow, reduced motion). */
  applyEffects: (cfg: AtermEffectsConfig) => void
  /** Re-theme the live engine in place and repaint. */
  applyTheme: (themeColors: AtermThemeColors) => void
  dispose: () => void
}

type CreateArgs = {
  canvas: HTMLCanvasElement
  cols: number
  rows: number
  fontPx: number
  themeColors: AtermThemeColors
  effects: AtermEffectsConfig
}

export async function createTerminalEngineEffectsDemoEngine(
  args: CreateArgs,
  isCancelled: () => boolean
): Promise<TerminalEngineEffectsDemoEngine | null> {
  const ctx = args.canvas.getContext('2d')
  const dpr = window.devicePixelRatio || 1
  const pending = await loadAtermCpuDrawer({
    canvas: args.canvas,
    themeColors: args.themeColors,
    fontPx: Math.round(args.fontPx * dpr)
  })
  if (isCancelled()) {
    pending.term.free()
    return null
  }
  const term = pending.term
  let disposed = false

  const repaint = (): void => {
    if (!ctx || disposed) {
      return
    }
    term.render()
    const width = term.width
    const height = term.height
    if (args.canvas.width !== width || args.canvas.height !== height) {
      args.canvas.width = width
      args.canvas.height = height
    }
    const liveDpr = window.devicePixelRatio || 1
    args.canvas.style.width = `${width / liveDpr}px`
    args.canvas.style.height = `${height / liveDpr}px`
    ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), width, height), 0, 0)
  }

  // The engine animation-drive contract: rAF cadence only while an effect is
  // animating; once settled, drop to zero scheduled work until the typewriter
  // (fresh "PTY output") re-arms it.
  let rafId: number | null = null
  let lastTickMs: number | null = null
  const pump = (): void => {
    rafId = null
    if (disposed) {
      return
    }
    const now = performance.now()
    const dt = lastTickMs === null ? 0 : Math.min(now - lastTickMs, MAX_TICK_MS)
    lastTickMs = now
    term.advance_effects(dt)
    repaint()
    if (term.is_effects_active()) {
      rafId = requestAnimationFrame(pump)
    } else {
      lastTickMs = null
    }
  }
  const kick = (): void => {
    if (rafId === null && !disposed) {
      rafId = requestAnimationFrame(pump)
    }
  }

  // Typewriter: real bytes through the real parser, one char per tick, so the
  // cursor moves (glow) and lexicon words complete (sparkle) exactly as they
  // would in a live pane.
  let lineIndex = 0
  let charIndex = 0
  let timer: ReturnType<typeof setTimeout> | null = null
  const encoder = new TextEncoder()
  const typeNext = (): void => {
    if (disposed) {
      return
    }
    let delay = TYPE_INTERVAL_MS
    const line = DEMO_LINES[lineIndex]
    if (charIndex === 0 && lineIndex === 0) {
      term.process(encoder.encode(CLEAR_AND_HOME + PROMPT))
    }
    if (charIndex < line.length) {
      term.process(encoder.encode(line[charIndex]))
      charIndex += 1
    } else {
      lineIndex = (lineIndex + 1) % DEMO_LINES.length
      charIndex = 0
      term.process(encoder.encode(lineIndex === 0 ? '\r\n' : `\r\n${PROMPT}`))
      delay = LINE_PAUSE_MS
    }
    repaint()
    kick()
    timer = setTimeout(typeNext, delay)
  }

  term.resize(args.rows, args.cols)
  applyAtermEffectsConfig(term, args.effects)
  repaint()
  timer = setTimeout(typeNext, TYPE_INTERVAL_MS)

  return {
    applyEffects(cfg) {
      applyAtermEffectsConfig(term, cfg)
      repaint()
      kick()
    },
    applyTheme(themeColors) {
      applyAtermLiveTheme(term, themeColors, term.cell_width, term.cell_height)
      repaint()
    },
    dispose() {
      disposed = true
      if (timer !== null) {
        clearTimeout(timer)
      }
      if (rafId !== null) {
        cancelAnimationFrame(rafId)
      }
      try {
        term.free()
      } catch {
        /* ignore — engine may already be freed */
      }
    }
  }
}
