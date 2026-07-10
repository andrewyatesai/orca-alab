import type { AtermMatrixRainWireConfig } from './aterm-matrix-rain-types'

type SetMatrixRain = AtermMatrixRainWireConfig & {
  type: 'setMatrixRain'
  enabled: boolean
  reducedMotion: boolean
}

type SetEffectsVisibility = {
  type: 'setEffectsVisibility'
  state: 'focused' | 'visible_unfocused' | 'hidden'
}

type EffectActivity = {
  type: 'effectActivity'
  kind: 'keystroke' | 'matrixRainAltScroll'
}

export type AtermWorkerRainCommand = SetMatrixRain | SetEffectsVisibility | EffectActivity
