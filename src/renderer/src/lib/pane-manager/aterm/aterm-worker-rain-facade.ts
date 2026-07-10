import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermMatrixRainArgs, AtermMatrixRainWireConfig } from './aterm-matrix-rain-types'
import type { AtermWorkerPaneCommand } from './aterm-render-worker-protocol'

/** Add the local retained state needed to collapse the binding's three setters
 *  into one atomic worker command and keep disabled input at zero IPC. */
export function attachAtermWorkerRainFacade(
  term: AtermTerminal,
  post: (command: AtermWorkerPaneCommand) => void
): { setCursorGlowEnabled: (enabled: boolean) => void } {
  let config: AtermMatrixRainWireConfig | null = null
  let reducedMotion = false
  let enabled = false
  let cursorGlowEnabled = false
  Object.defineProperty(term, 'matrix_rain_enabled', {
    configurable: true,
    get: () => enabled
  })
  term.set_matrix_rain = (...args: AtermMatrixRainArgs): void => {
    config = {
      fps: args[0],
      density: args[1],
      speed: args[2],
      trail: args[3],
      alpha: args[4] ?? null,
      headAlpha: args[5] ?? null,
      hue: args[6],
      hueColor: args[7] ?? null,
      mutationMs: args[8],
      idleSecs: args[9],
      suppressInAltScreen: args[10],
      turnWave: args[11],
      bellAlert: args[12],
      outputMaterial: args[13],
      seed: args[14]
    }
  }
  term.set_matrix_rain_reduced_motion = (on: boolean): void => {
    reducedMotion = on
  }
  term.set_matrix_rain_enabled = (on: boolean): void => {
    enabled = on
    if (config) {
      post({ type: 'setMatrixRain', ...config, enabled, reducedMotion })
    }
  }
  term.set_effects_visibility = (state: string): void => {
    const normalized = state === 'hidden' || state === 'visible_unfocused' ? state : 'focused'
    post({ type: 'setEffectsVisibility', state: normalized })
  }
  term.note_keystroke = (): void => {
    // Typing cadence feeds both rain echo correlation and cursor-comet momentum.
    if (cursorGlowEnabled || (enabled && !reducedMotion)) {
      post({ type: 'effectActivity', kind: 'keystroke' })
    }
  }
  term.note_matrix_rain_alt_scroll = (): void => {
    if (enabled && !reducedMotion) {
      post({ type: 'effectActivity', kind: 'matrixRainAltScroll' })
    }
  }
  return {
    setCursorGlowEnabled: (on) => {
      cursorGlowEnabled = on
    }
  }
}
