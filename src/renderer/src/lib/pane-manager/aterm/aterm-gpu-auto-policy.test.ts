/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Mutable settings stand-in (vi.hoisted so the mock factory can reference it).
const { settingsHolder } = vi.hoisted(() => ({
  settingsHolder: {
    settings: {} as { terminalGpuAcceleration?: 'auto' | 'on' | 'off' } | undefined
  }
}))
vi.mock('@/store', () => ({ useAppStore: { getState: () => settingsHolder } }))

// Controllable stand-ins for the two safety inputs the policy composes.
const { probeHolder, webglHolder } = vi.hoisted(() => ({
  probeHolder: { available: true },
  webglHolder: { allowWebgl: true }
}))
vi.mock('./aterm-gpu-probe', () => ({
  probeAtermGpu: () => ({ available: probeHolder.available, renderer: null, vendor: null })
}))
vi.mock('../terminal-webgl-auto-policy', () => ({
  getTerminalWebglAutoDecision: () => ({
    allowWebgl: webglHolder.allowWebgl,
    reason: 'non-linux',
    renderer: null,
    vendor: null
  })
}))

import { decideAtermGpu, isAtermGpuEnabled } from './aterm-gpu-auto-policy'

const w = window as unknown as {
  __atermGpuEnabled?: boolean
  __atermGpuDisabled?: boolean
}

beforeEach(() => {
  delete w.__atermGpuEnabled
  delete w.__atermGpuDisabled
  settingsHolder.settings = {}
  probeHolder.available = true
  webglHolder.allowWebgl = true
})
afterEach(() => {
  delete w.__atermGpuEnabled
  delete w.__atermGpuDisabled
})

describe('decideAtermGpu precedence', () => {
  it('defaults to GPU on capable hardware with no setting (auto)', () => {
    expect(decideAtermGpu()).toEqual({ useGpu: true, reason: 'auto-allowed' })
    expect(isAtermGpuEnabled()).toBe(true)
  })

  it('window force-ON wins over an off setting + bypasses the safety gate', () => {
    w.__atermGpuEnabled = true
    settingsHolder.settings = { terminalGpuAcceleration: 'off' }
    webglHolder.allowWebgl = false // software renderer — gate would reject auto
    expect(decideAtermGpu()).toEqual({ useGpu: true, reason: 'forced-on' })
  })

  it('window force-ON still needs a creatable webgl2 context', () => {
    w.__atermGpuEnabled = true
    probeHolder.available = false
    expect(decideAtermGpu()).toEqual({ useGpu: false, reason: 'auto-no-webgl2' })
  })

  it('window force-OFF wins over an on setting', () => {
    w.__atermGpuDisabled = true
    settingsHolder.settings = { terminalGpuAcceleration: 'on' }
    expect(decideAtermGpu()).toEqual({ useGpu: false, reason: 'forced-off' })
  })

  it('setting off keeps the pane on CPU even on capable hardware', () => {
    settingsHolder.settings = { terminalGpuAcceleration: 'off' }
    expect(decideAtermGpu()).toEqual({ useGpu: false, reason: 'setting-off' })
  })

  it('setting on uses GPU even when the auto gate would reject the renderer', () => {
    settingsHolder.settings = { terminalGpuAcceleration: 'on' }
    webglHolder.allowWebgl = false
    expect(decideAtermGpu()).toEqual({ useGpu: true, reason: 'setting-on' })
  })

  it('setting on still falls back to CPU when no webgl2 context is creatable', () => {
    settingsHolder.settings = { terminalGpuAcceleration: 'on' }
    probeHolder.available = false
    expect(decideAtermGpu()).toEqual({ useGpu: false, reason: 'auto-no-webgl2' })
  })

  it('auto falls back to CPU when no webgl2 context is creatable', () => {
    probeHolder.available = false
    expect(decideAtermGpu()).toEqual({ useGpu: false, reason: 'auto-no-webgl2' })
  })

  it('auto falls back to CPU on a known-bad software/Linux renderer', () => {
    webglHolder.allowWebgl = false
    expect(decideAtermGpu()).toEqual({ useGpu: false, reason: 'auto-unsafe-renderer' })
  })
})
