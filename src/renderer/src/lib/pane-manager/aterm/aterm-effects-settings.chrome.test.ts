/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import {
  applyAtermCursorGlowConfig,
  applyAtermWindowChrome,
  wireAtermWindowChrome,
  type AtermEffectsConfig
} from './aterm-effects-settings'

// Proves the window-space chrome seam: chrome is granted to EVERY enabled cursor
// glow style on a windowChromeCapable target (every emission clips at the frame
// edge without headroom) — glow off / reduced motion, and every unmarked engine
// (e.g. the settings demo, which HAS set_chrome but no offset handling), must
// stay at the byte-identical 0/0 default. Sizing follows the cell box.

// The store is consulted only by wireAtermWindowChrome's re-derivation hook
// (readAtermEffectsConfig); glow on + fire so the hook grants chrome.
vi.mock('@/store', () => ({
  useAppStore: {
    getState: () => ({
      settings: { terminalEffectsCursorGlow: true, terminalEffectsCursorGlowStyle: 'fire' }
    })
  }
}))

function cfg(overrides: Partial<AtermEffectsConfig> = {}): AtermEffectsConfig {
  return {
    sparkleWords: false,
    sparkleProfanity: true,
    sparkleFeline: true,
    sparkleOrca: true,
    sparkleEmphasis: true,
    matrixRain: false,
    cursorGlow: true,
    cursorGlowStyle: 'fire',
    reducedMotion: false,
    ...overrides
  }
}

function makeTarget(opts: { capable: boolean; cellHeight?: number }): {
  target: Parameters<typeof applyAtermWindowChrome>[0]
  setChrome: ReturnType<typeof vi.fn>
} {
  const setChrome = vi.fn()
  return {
    target: {
      cell_height: opts.cellHeight ?? 17,
      set_chrome: setChrome,
      ...(opts.capable ? { windowChromeCapable: true as const } : {})
    },
    setChrome
  }
}

describe('applyAtermWindowChrome', () => {
  it('an enabled glow on a capable target sizes chrome from the cell box (ceil)', () => {
    const { target, setChrome } = makeTarget({ capable: true, cellHeight: 17 })
    applyAtermWindowChrome(target, cfg())
    // pad = ceil(17 * 0.75) = 13, head = ceil(17 * 2) = 34.
    expect(setChrome).toHaveBeenCalledTimes(1)
    expect(setChrome).toHaveBeenCalledWith(13, 34)
  })

  it('grants chrome to EVERY glow style (all emissions clip at the frame edge)', () => {
    for (const style of ['water', 'lumen', 'rainbow'] as const) {
      const { target, setChrome } = makeTarget({ capable: true, cellHeight: 17 })
      applyAtermWindowChrome(target, cfg({ cursorGlowStyle: style }))
      expect(setChrome).toHaveBeenCalledWith(13, 34)
    }
  })

  it('glow disabled resets chrome even when the style is fire', () => {
    const { target, setChrome } = makeTarget({ capable: true })
    applyAtermWindowChrome(target, cfg({ cursorGlow: false }))
    expect(setChrome).toHaveBeenCalledWith(0, 0)
  })

  it('reduced motion resets chrome (the glow is host-gated fully off)', () => {
    const { target, setChrome } = makeTarget({ capable: true })
    applyAtermWindowChrome(target, cfg({ reducedMotion: true }))
    expect(setChrome).toHaveBeenCalledWith(0, 0)
  })

  it('an unready cell box (0) never grants chrome', () => {
    const { target, setChrome } = makeTarget({ capable: true, cellHeight: 0 })
    applyAtermWindowChrome(target, cfg())
    expect(setChrome).toHaveBeenCalledWith(0, 0)
  })

  it('NEVER touches set_chrome without the capability marker (bare-engine safety)', () => {
    // Unwired engines (the settings demo) DO expose set_chrome; the missing
    // marker alone must keep them chrome-free (no canvas offset handling).
    const { target, setChrome } = makeTarget({ capable: false })
    applyAtermWindowChrome(target, cfg())
    expect(setChrome).not.toHaveBeenCalled()
  })

  it('is a no-op when the target lacks set_chrome (artifact skew)', () => {
    expect(() =>
      applyAtermWindowChrome({ cell_height: 17, windowChromeCapable: true }, cfg())
    ).not.toThrow()
  })
})

describe('wireAtermWindowChrome (pane-wiring seam)', () => {
  it('marks the wired target capable and re-derives chrome from the live config', () => {
    const { target, setChrome } = makeTarget({ capable: false, cellHeight: 17 })
    const syncDependents = vi.fn()
    const onMetricsChanged = wireAtermWindowChrome(target, syncDependents)
    expect(target.windowChromeCapable).toBe(true)
    // The hook runs the wiring's dependents sync, then re-reads the LIVE store
    // config (glow on + fire, mocked above), so a cell-metrics change re-sizes
    // the chrome instead of leaving stale headroom.
    onMetricsChanged()
    expect(syncDependents).toHaveBeenCalledTimes(1)
    expect(setChrome).toHaveBeenLastCalledWith(13, 34)
    ;(target as { cell_height: number }).cell_height = 20
    onMetricsChanged()
    expect(setChrome).toHaveBeenLastCalledWith(15, 40)
  })
})

describe('applyAtermCursorGlowConfig chrome routing', () => {
  it('applies glow chrome through the glow-config seam (both apply paths end here)', () => {
    const { target, setChrome } = makeTarget({ capable: true, cellHeight: 16 })
    const glowTarget = { ...target, set_cursor_glow: vi.fn() }
    applyAtermCursorGlowConfig(glowTarget, cfg())
    expect(setChrome).toHaveBeenCalledWith(12, 32)
    // A style change keeps chrome (all styles emit past the grid)...
    applyAtermCursorGlowConfig(glowTarget, cfg({ cursorGlowStyle: 'lumen' }))
    expect(setChrome).toHaveBeenLastCalledWith(12, 32)
    // ...and only disabling the glow restores the byte-identical 0/0 frame.
    applyAtermCursorGlowConfig(glowTarget, cfg({ cursorGlow: false }))
    expect(setChrome).toHaveBeenLastCalledWith(0, 0)
  })
})
