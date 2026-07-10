import { describe, expect, it, vi } from 'vitest'

const store = vi.hoisted(() => ({ settings: {} as Record<string, unknown> }))

vi.mock('@/store', () => ({
  useAppStore: { getState: () => ({ settings: store.settings }) }
}))

import { readAtermEffectsConfig } from './aterm-effects-settings'

describe('aterm effects profile defaults', () => {
  it('keeps cats and the water trail enabled for pre-feature profiles', () => {
    store.settings = {}
    const config = readAtermEffectsConfig()
    expect(config.sparkleWords).toBe(true)
    expect(config.sparkleFeline).toBe(true)
    expect(config.cursorGlow).toBe(true)
    expect(config.cursorGlowStyle).toBe('water')
  })

  it('preserves an explicit user opt-out', () => {
    store.settings = {
      terminalEffectsSparkleWords: false,
      terminalEffectsCursorGlow: false,
      terminalEffectsCursorGlowStyle: 'lumen'
    }
    expect(readAtermEffectsConfig()).toMatchObject({
      sparkleWords: false,
      cursorGlow: false,
      cursorGlowStyle: 'lumen'
    })
  })
})
