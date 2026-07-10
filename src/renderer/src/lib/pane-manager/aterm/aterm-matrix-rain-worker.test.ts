import { describe, expect, it, vi } from 'vitest'
import type { AtermTerminal } from './aterm_wasm.js'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import type { AtermWorkerPaneCommand } from './aterm-render-worker-protocol'
import { attachAtermWorkerRainFacade } from './aterm-worker-rain-facade'
import { applyAtermMatrixRainConfig, type AtermEffectsConfig } from './aterm-effects-settings'

function makePane() {
  const setters = {
    set_matrix_rain: vi.fn(),
    set_matrix_rain_reduced_motion: vi.fn(),
    set_matrix_rain_enabled: vi.fn(),
    set_effects_visibility: vi.fn(),
    note_keystroke: vi.fn(),
    note_matrix_rain_alt_scroll: vi.fn()
  }
  const schedule = vi.fn()
  const pane = {
    term: {},
    engineSetters: setters,
    frameScheduler: { schedule }
  } as unknown as PaneRuntime
  return { pane, setters, schedule }
}

describe('matrix rain worker forwarding', () => {
  it('forwards the literal profile, accessibility gate and master in order-safe commands', () => {
    const { pane, setters } = makePane()
    dispatchPaneCommand(pane, {
      type: 'setMatrixRain',
      fps: 30,
      density: 6,
      speed: 5,
      trail: 5,
      alpha: null,
      headAlpha: null,
      hue: 'theme',
      hueColor: null,
      mutationMs: 133,
      idleSecs: 8,
      suppressInAltScreen: false,
      turnWave: true,
      bellAlert: true,
      outputMaterial: true,
      seed: 0n,
      enabled: true,
      reducedMotion: true
    })

    expect(setters.set_matrix_rain).toHaveBeenCalledWith(
      30,
      6,
      5,
      5,
      undefined,
      undefined,
      'theme',
      undefined,
      133,
      8,
      false,
      true,
      true,
      true,
      0n
    )
    expect(setters.set_matrix_rain_reduced_motion).toHaveBeenCalledWith(true)
    expect(setters.set_matrix_rain_enabled).toHaveBeenCalledWith(true)
  })

  it('forwards visibility, typing cadence and alternate-screen reading activity', () => {
    const { pane, setters } = makePane()
    dispatchPaneCommand(pane, { type: 'setEffectsVisibility', state: 'hidden' })
    dispatchPaneCommand(pane, { type: 'effectActivity', kind: 'keystroke' })
    dispatchPaneCommand(pane, { type: 'effectActivity', kind: 'matrixRainAltScroll' })

    expect(setters.set_effects_visibility).toHaveBeenCalledWith('hidden')
    expect(setters.note_keystroke).toHaveBeenCalledTimes(1)
    expect(setters.note_matrix_rain_alt_scroll).toHaveBeenCalledTimes(1)
  })
})

describe('matrix rain worker facade activity gating', () => {
  it('collapses the complete retained profile into one atomic worker command', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as AtermTerminal
    attachAtermWorkerRainFacade(term, (command) => commands.push(command))
    const config: AtermEffectsConfig = {
      sparkleWords: false,
      sparkleProfanity: true,
      sparkleFeline: true,
      sparkleOrca: true,
      sparkleEmphasis: true,
      matrixRain: true,
      cursorGlow: false,
      cursorGlowStyle: 'water',
      reducedMotion: false
    }

    applyAtermMatrixRainConfig(term, config)

    expect(commands).toEqual([
      {
        type: 'setMatrixRain',
        fps: 30,
        density: 6,
        speed: 5,
        trail: 5,
        alpha: null,
        headAlpha: null,
        hue: 'theme',
        hueColor: null,
        mutationMs: 133,
        idleSecs: 8,
        suppressInAltScreen: false,
        turnWave: true,
        bellAlert: false,
        outputMaterial: true,
        seed: 0n,
        enabled: true,
        reducedMotion: false
      }
    ])
  })

  it('keeps cursor-comet typing momentum when rain is off and glow is on', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as AtermTerminal
    const facade = attachAtermWorkerRainFacade(term, (command) => commands.push(command))

    term.note_keystroke()
    expect(commands).toEqual([])

    facade.setCursorGlowEnabled(true)
    term.note_keystroke()
    expect(commands).toEqual([{ type: 'effectActivity', kind: 'keystroke' }])

    facade.setCursorGlowEnabled(false)
    term.note_keystroke()
    expect(commands).toHaveLength(1)
  })

  it('gates alternate-screen reading activity on rain only', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as AtermTerminal
    const facade = attachAtermWorkerRainFacade(term, (command) => commands.push(command))

    facade.setCursorGlowEnabled(true)
    term.note_matrix_rain_alt_scroll()
    expect(commands).toEqual([])

    term.set_matrix_rain_enabled(true)
    term.note_matrix_rain_alt_scroll()
    expect(commands).toEqual([{ type: 'effectActivity', kind: 'matrixRainAltScroll' }])
  })

  it('suppresses rain-only activity under reduced motion without muting cursor glow', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as AtermTerminal
    const facade = attachAtermWorkerRainFacade(term, (command) => commands.push(command))

    term.set_matrix_rain_reduced_motion(true)
    term.set_matrix_rain_enabled(true)
    term.note_matrix_rain_alt_scroll()
    term.note_keystroke()
    expect(commands).toEqual([])

    facade.setCursorGlowEnabled(true)
    term.note_keystroke()
    expect(commands).toEqual([{ type: 'effectActivity', kind: 'keystroke' }])
  })
})
