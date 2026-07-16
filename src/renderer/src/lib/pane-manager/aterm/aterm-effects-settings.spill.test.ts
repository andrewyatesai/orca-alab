/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { applyAtermWindowChrome, type AtermEffectsConfig } from './aterm-effects-settings'
import { atermSpillOverlay } from './aterm-spill-overlay'

// The spill registration seam (stage 2, FEATURE-DARK): a pane joins the
// window-space overlay ONLY when chrome is nonzero AND the wiring set the
// spillExportCapable marker. No engine sets the marker yet, so every current
// caller must remain byte-identical (chrome applied, nothing registered).

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

const PANE_KEY = 'tab-spill:44444444-4444-4444-8444-444444444444'

function makeTarget(overrides: { spillExportCapable?: boolean; spillPaneKey?: string }): {
  target: Parameters<typeof applyAtermWindowChrome>[0]
  setChrome: ReturnType<typeof vi.fn>
} {
  const setChrome = vi.fn()
  return {
    target: {
      cell_height: 17,
      set_chrome: setChrome,
      windowChromeCapable: true as const,
      ...overrides
    },
    setChrome
  }
}

afterEach(() => {
  for (const paneKey of atermSpillOverlay.getPaneKeys()) {
    atermSpillOverlay.unregister(paneKey)
  }
})

describe('applyAtermWindowChrome spill registration seam', () => {
  it('registers the pane when chrome is granted AND the engine is spill-export capable', () => {
    const { target, setChrome } = makeTarget({ spillExportCapable: true, spillPaneKey: PANE_KEY })
    applyAtermWindowChrome(target, cfg())
    expect(setChrome).toHaveBeenCalledWith(13, 34)
    expect(atermSpillOverlay.getPaneCount()).toBe(1)
    expect(atermSpillOverlay.getPaneChrome(PANE_KEY)).toEqual({
      chromePadPx: 13,
      chromeHeadPx: 34
    })
  })

  it('fails closed without the capability marker (no engine sets it yet)', () => {
    const { target, setChrome } = makeTarget({ spillPaneKey: PANE_KEY })
    applyAtermWindowChrome(target, cfg())
    // Chrome behavior stays byte-identical; only the registration is withheld.
    expect(setChrome).toHaveBeenCalledWith(13, 34)
    expect(atermSpillOverlay.getPaneCount()).toBe(0)
  })

  it('fails closed with an explicit false marker and without a pane key', () => {
    const withFalseMarker = makeTarget({ spillExportCapable: false, spillPaneKey: PANE_KEY })
    applyAtermWindowChrome(withFalseMarker.target, cfg())
    const withoutKey = makeTarget({ spillExportCapable: true })
    applyAtermWindowChrome(withoutKey.target, cfg())
    expect(atermSpillOverlay.getPaneCount()).toBe(0)
  })

  it('0/0 chrome (glow off / reduced motion) unregisters the pane', () => {
    const { target, setChrome } = makeTarget({ spillExportCapable: true, spillPaneKey: PANE_KEY })
    applyAtermWindowChrome(target, cfg())
    expect(atermSpillOverlay.getPaneCount()).toBe(1)
    applyAtermWindowChrome(target, cfg({ cursorGlow: false }))
    expect(setChrome).toHaveBeenLastCalledWith(0, 0)
    expect(atermSpillOverlay.getPaneCount()).toBe(0)
    applyAtermWindowChrome(target, cfg({ reducedMotion: true }))
    expect(atermSpillOverlay.getPaneCount()).toBe(0)
  })

  it('an unready cell box (0) registers nothing', () => {
    const setChrome = vi.fn()
    applyAtermWindowChrome(
      {
        cell_height: 0,
        set_chrome: setChrome,
        windowChromeCapable: true,
        spillExportCapable: true,
        spillPaneKey: PANE_KEY
      },
      cfg()
    )
    expect(setChrome).toHaveBeenCalledWith(0, 0)
    expect(atermSpillOverlay.getPaneCount()).toBe(0)
  })

  it('never registers through an unmarked windowChromeCapable target', () => {
    const setChrome = vi.fn()
    applyAtermWindowChrome(
      { cell_height: 17, set_chrome: setChrome, spillExportCapable: true, spillPaneKey: PANE_KEY },
      cfg()
    )
    expect(setChrome).not.toHaveBeenCalled()
    expect(atermSpillOverlay.getPaneCount()).toBe(0)
  })
})
