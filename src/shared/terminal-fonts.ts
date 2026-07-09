// Terminal font-weight range constants. The normalize/resolve logic moved to the
// Rust `orca_core::terminal_fonts` port (renderer via orca-git wasm, main via
// napi); only the numeric range data still lives in TS — imported by
// shared/constants.ts (the settings default) and the settings sliders (min/max/
// step). Keep this file data-only: no napi/wasm import.
export const DEFAULT_TERMINAL_FONT_WEIGHT = 500
export const TERMINAL_FONT_WEIGHT_MIN = 100
export const TERMINAL_FONT_WEIGHT_MAX = 900
export const TERMINAL_FONT_WEIGHT_STEP = 100
