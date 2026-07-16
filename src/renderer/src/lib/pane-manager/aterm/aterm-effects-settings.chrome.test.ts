/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it, vi } from 'vitest'
import {
  applyAtermCursorGlowConfig,
  applyAtermWindowChrome,
  type AtermEffectsConfig
} from './aterm-effects-settings'

// Proves the window-space chrome seam: chrome is granted ONLY to the fire cursor
// style on a windowChromeCapable target (the worker facade) — every other config,
// and every in-process engine (no marker, even though it HAS set_chrome), must
// stay at the byte-identical 0/0 default. Sizing follows the cell box.

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
  it('fire style on a capable target sizes chrome from the cell box (ceil)', () => {
    const { target, setChrome } = makeTarget({ capable: true, cellHeight: 17 })
    applyAtermWindowChrome(target, cfg())
    // pad = ceil(17 * 0.75) = 13, head = ceil(17 * 2) = 34.
    expect(setChrome).toHaveBeenCalledTimes(1)
    expect(setChrome).toHaveBeenCalledWith(13, 34)
  })

  it('non-fire styles reset chrome to the byte-identical 0/0 default', () => {
    const { target, setChrome } = makeTarget({ capable: true })
    applyAtermWindowChrome(target, cfg({ cursorGlowStyle: 'water' }))
    expect(setChrome).toHaveBeenCalledWith(0, 0)
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

  it('NEVER touches set_chrome without the capability marker (in-process safety)', () => {
    // The real in-process engines DO expose set_chrome; the missing marker alone
    // must keep them chrome-free (their drawers pin the canvas box unoffset).
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

describe('applyAtermCursorGlowConfig chrome routing', () => {
  it('applies fire chrome through the glow-config seam (both apply paths end here)', () => {
    const { target, setChrome } = makeTarget({ capable: true, cellHeight: 16 })
    const glowTarget = { ...target, set_cursor_glow: vi.fn() }
    applyAtermCursorGlowConfig(glowTarget, cfg())
    expect(setChrome).toHaveBeenCalledWith(12, 32)
    // And a style change back to a grid-space trail restores 0/0.
    applyAtermCursorGlowConfig(glowTarget, cfg({ cursorGlowStyle: 'lumen' }))
    expect(setChrome).toHaveBeenLastCalledWith(0, 0)
  })
})
