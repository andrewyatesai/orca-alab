import { describe, expect, it, vi } from 'vitest'
import type { AtermTerminal } from './aterm_wasm.js'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import type { AtermWorkerPaneCommand } from './aterm-render-worker-protocol'
import { attachAtermWorkerRainFacade } from './aterm-worker-rain-facade'
import { applyAtermMatrixRainConfig, type AtermEffectsConfig } from './aterm-effects-settings'
import { driveAtermRainPulse } from './aterm-rain-pulse'

type RainFacadeTerm = AtermTerminal & {
  note_matrix_rain_signal: (code: number, weight: number) => void
}

function makePane() {
  const setters = {
    set_matrix_rain: vi.fn(),
    set_matrix_rain_reduced_motion: vi.fn(),
    set_matrix_rain_enabled: vi.fn(),
    set_effects_visibility: vi.fn(),
    note_keystroke: vi.fn(),
    note_matrix_rain_alt_scroll: vi.fn(),
    note_matrix_rain_signal: vi.fn()
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

  it('forwards only the bounded semantic pulse, never hook payload text', () => {
    const { pane, setters, schedule } = makePane()
    dispatchPaneCommand(pane, { type: 'matrixRainPulse', code: 4, weight: 5 })
    expect(setters.note_matrix_rain_signal).toHaveBeenCalledWith(4, 5)
    expect(schedule).toHaveBeenCalledWith(false)
  })

  it('does not schedule a pulse before the worker engine has finished construction', () => {
    const { pane, schedule } = makePane()
    pane.engineSetters = null

    dispatchPaneCommand(pane, { type: 'matrixRainPulse', code: 3, weight: 6 })

    expect(schedule).not.toHaveBeenCalled()
  })

  it('does not schedule a pulse when a version-skewed engine lacks the method', () => {
    const { pane, setters, schedule } = makePane()
    delete (setters as Partial<typeof setters>).note_matrix_rain_signal

    dispatchPaneCommand(pane, { type: 'matrixRainPulse', code: 3, weight: 6 })

    expect(schedule).not.toHaveBeenCalled()
  })
})

describe('matrix rain worker facade activity gating', () => {
  it('collapses the complete retained profile into one atomic worker command', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as RainFacadeTerm
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
    const term = {} as RainFacadeTerm
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
    const term = {} as RainFacadeTerm
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
    const term = {} as RainFacadeTerm
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

  it('posts semantic pulses only while live rain can render', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as RainFacadeTerm
    attachAtermWorkerRainFacade(term, (command) => commands.push(command))

    term.note_matrix_rain_signal(4, 5)
    expect(commands).toEqual([])

    term.set_matrix_rain(
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
      false,
      true,
      0n
    )
    term.set_matrix_rain_enabled(true)
    commands.length = 0
    term.note_matrix_rain_signal(4, 5)
    expect(commands).toEqual([{ type: 'matrixRainPulse', code: 4, weight: 5 }])

    term.set_matrix_rain_reduced_motion(true)
    term.note_matrix_rain_signal(3, 6)
    expect(commands).toHaveLength(1)
  })

  it('uses exactly one worker-side render request for a semantic pulse', () => {
    const commands: AtermWorkerPaneCommand[] = []
    const term = {} as RainFacadeTerm
    attachAtermWorkerRainFacade(term, (command) => commands.push(command))
    applyAtermMatrixRainConfig(term, {
      sparkleWords: true,
      sparkleProfanity: true,
      sparkleFeline: true,
      sparkleOrca: true,
      sparkleEmphasis: true,
      matrixRain: true,
      cursorGlow: false,
      cursorGlowStyle: 'water',
      reducedMotion: false
    })
    commands.length = 0

    expect(driveAtermRainPulse(term, { signal: 'modify', weight: 5 })).toBe(true)
    expect(commands).toEqual([{ type: 'matrixRainPulse', code: 2, weight: 5 }])

    const { pane, schedule } = makePane()
    const command = commands[0]
    if (!command || command.type !== 'matrixRainPulse') {
      throw new Error('expected one Matrix Rain pulse command')
    }
    dispatchPaneCommand(pane, command)
    expect(schedule).toHaveBeenCalledTimes(1)
    expect(schedule).toHaveBeenCalledWith(false)
  })
})
