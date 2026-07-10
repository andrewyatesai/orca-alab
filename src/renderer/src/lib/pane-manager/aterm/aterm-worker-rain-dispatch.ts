import type { AtermMatrixRainTarget } from './aterm-matrix-rain-types'
import type { AtermWorkerRainCommand } from './aterm-worker-rain-protocol'

export type AtermWorkerRainTarget = AtermMatrixRainTarget & {
  set_effects_visibility: (state: string) => void
  note_keystroke: () => void
  note_matrix_rain_alt_scroll: () => void
  note_matrix_rain_signal?: (code: number, weight: number) => void
}

/** Handle the compact rain/activity subset outside the already-large pane dispatcher. */
export function dispatchAtermWorkerRainCommand(
  target: AtermWorkerRainTarget | null,
  scheduleDraw: (postState?: boolean) => void,
  msg: AtermWorkerRainCommand
): void {
  switch (msg.type) {
    case 'setMatrixRain':
      target?.set_matrix_rain(
        msg.fps,
        msg.density,
        msg.speed,
        msg.trail,
        msg.alpha ?? undefined,
        msg.headAlpha ?? undefined,
        msg.hue,
        msg.hueColor ?? undefined,
        msg.mutationMs,
        msg.idleSecs,
        msg.suppressInAltScreen,
        msg.turnWave,
        msg.bellAlert,
        msg.outputMaterial,
        msg.seed
      )
      target?.set_matrix_rain_reduced_motion(msg.reducedMotion)
      target?.set_matrix_rain_enabled(msg.enabled)
      scheduleDraw(false)
      return
    case 'setEffectsVisibility':
      target?.set_effects_visibility(msg.state)
      scheduleDraw(false)
      return
    case 'effectActivity':
      if (msg.kind === 'keystroke') {
        target?.note_keystroke()
      } else {
        target?.note_matrix_rain_alt_scroll()
        scheduleDraw(false)
      }
      return
    case 'matrixRainPulse':
      // The facade is attached only after the worker's first STATE, so normal
      // construction cannot post this command. Still fail closed under version
      // skew or a synthetic early command: no engine call means no empty draw.
      if (target?.note_matrix_rain_signal) {
        target.note_matrix_rain_signal(msg.code, msg.weight)
        scheduleDraw(false)
      }
  }
}
