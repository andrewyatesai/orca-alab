/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Mutable stand-in for the settings slice; vi.hoisted so the mock factory can
// reference it (vi.mock is hoisted above imports).
const { settingsHolder } = vi.hoisted(() => ({
  settingsHolder: { settings: {} as { experimentalAtermRenderer?: boolean } | undefined }
}))
vi.mock('@/store', () => ({ useAppStore: { getState: () => settingsHolder } }))

import { isAtermRendererEnabled } from './aterm-renderer-flag'

const w = window as unknown as {
  __atermRendererEnabled?: boolean
  __atermRendererDisabled?: boolean
}

beforeEach(() => {
  delete w.__atermRendererEnabled
  delete w.__atermRendererDisabled
  settingsHolder.settings = {}
})
afterEach(() => {
  delete w.__atermRendererEnabled
  delete w.__atermRendererDisabled
})

describe('isAtermRendererEnabled precedence', () => {
  it('defaults ON when the setting is unset (opt-out)', () => {
    settingsHolder.settings = {}
    expect(isAtermRendererEnabled()).toBe(true)
  })

  it('defaults ON when the whole settings object is undefined', () => {
    settingsHolder.settings = undefined
    expect(isAtermRendererEnabled()).toBe(true)
  })

  it('OFF when the setting is explicitly false', () => {
    settingsHolder.settings = { experimentalAtermRenderer: false }
    expect(isAtermRendererEnabled()).toBe(false)
  })

  it('ON when the setting is explicitly true', () => {
    settingsHolder.settings = { experimentalAtermRenderer: true }
    expect(isAtermRendererEnabled()).toBe(true)
  })

  it('force-OFF window flag disables even when the setting is unset', () => {
    w.__atermRendererDisabled = true
    expect(isAtermRendererEnabled()).toBe(false)
  })

  // The exact scenario the audit said was unexercised: the e2e suite sets the
  // force-OFF disable, and aterm specs set the force-ON enable — ON must win.
  it('explicit force-ON WINS over the force-OFF disable AND a false setting', () => {
    w.__atermRendererEnabled = true
    w.__atermRendererDisabled = true
    settingsHolder.settings = { experimentalAtermRenderer: false }
    expect(isAtermRendererEnabled()).toBe(true)
  })
})
