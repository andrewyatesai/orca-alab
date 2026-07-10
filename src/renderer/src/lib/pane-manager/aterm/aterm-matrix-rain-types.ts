/** Positional wasm binding arguments for PHOSPHOR. */
export type AtermMatrixRainArgs = [
  fps: number,
  density: number,
  speed: number,
  trail: number,
  alpha: number | null | undefined,
  headAlpha: number | null | undefined,
  hue: string,
  hueColor: number | null | undefined,
  mutationMs: number,
  idleSecs: number,
  suppressInAltScreen: boolean,
  turnWave: boolean,
  bellAlert: boolean,
  outputMaterial: boolean,
  seed: bigint
]

export type AtermMatrixRainTarget = {
  set_matrix_rain_enabled: (on: boolean) => void
  set_matrix_rain: (...args: AtermMatrixRainArgs) => void
  set_matrix_rain_reduced_motion: (on: boolean) => void
}

/** Structured-clone-safe worker representation of the positional binding. */
export type AtermMatrixRainWireConfig = {
  fps: number
  density: number
  speed: number
  trail: number
  alpha: number | null
  headAlpha: number | null
  hue: string
  hueColor: number | null
  mutationMs: number
  idleSecs: number
  suppressInAltScreen: boolean
  turnWave: boolean
  bellAlert: boolean
  outputMaterial: boolean
  seed: bigint
}
