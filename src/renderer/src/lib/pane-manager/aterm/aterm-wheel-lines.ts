// Pure wheel-delta → grid-line math shared by the scrollback wheel path
// (aterm-scroll-input) and the mouse-report wheel forwarder (aterm-mouse-input),
// so trackpad pixel deltas, notched line deltas, and page deltas convert to
// lines identically on both paths.

// DOM WheelEvent.deltaMode discriminants.
export const WHEEL_DELTA_PIXEL = 0
export const WHEEL_DELTA_LINE = 1
export const WHEEL_DELTA_PAGE = 2

// A pixel-mode event this large is a notched mouse wheel routed through
// Chromium (one notch ≈ 100–120px), not a trackpad; same threshold the legacy
// wheel-replay gate used, kept so the TUI multiplier never touches trackpads.
const DISCRETE_WHEEL_PIXEL_THRESHOLD = 50

export type WheelLineConversion = {
  deltaY: number
  deltaMode: number
  /** Device-pixel ratio: pixel-mode deltas are CSS px, cellHeight is device px. */
  dpr: number
  cellHeight: number
  /** Viewport rows (page-mode scaling). */
  rows: number
  /** Sensitivity multiplier applied to the line count. */
  sensitivity: number
  /** Fractional lines carried from earlier events (trackpad sub-line deltas). */
  remainder: number
}

/** Convert one wheel event into WHOLE grid lines, carrying the fractional rest
 *  so trackpad sub-line deltas accumulate instead of being dropped. Positive
 *  lines = wheel down (toward newer output / ArrowDown). */
export function accumulateWheelLines(args: WheelLineConversion): {
  lines: number
  remainder: number
} {
  let lines: number
  if (args.deltaMode === WHEEL_DELTA_LINE) {
    lines = args.deltaY
  } else if (args.deltaMode === WHEEL_DELTA_PAGE) {
    lines = args.deltaY * Math.max(1, args.rows)
  } else {
    // WHEEL_DELTA_PIXEL: convert device pixels to grid lines.
    lines = (args.deltaY * args.dpr) / args.cellHeight
  }
  const total = args.remainder + lines * args.sensitivity
  const whole = Math.trunc(total)
  return { lines: whole, remainder: total - whole }
}

/** Scrollback wheel sensitivity: scrollSensitivity always, times
 *  fastScrollSensitivity while the fast-scroll modifier is held (Alt — xterm's
 *  fastScrollModifier default; the same DOM altKey is Option on macOS). */
export function resolveScrollbackWheelSensitivity(args: {
  altKey: boolean
  scrollSensitivity: number
  fastScrollSensitivity: number
}): number {
  return args.scrollSensitivity * (args.altKey ? args.fastScrollSensitivity : 1)
}

/** TUI wheel multiplier (terminalTuiScrollSensitivity), applied only to
 *  DISCRETE wheel events: trackpads already emit many fine-grained pixel
 *  events, so multiplying them would over-scroll TUIs at the default setting. */
export function resolveTuiWheelMultiplier(
  event: { deltaY: number; deltaMode: number },
  multiplier: number
): number {
  const discrete =
    event.deltaMode !== WHEEL_DELTA_PIXEL ||
    Math.abs(event.deltaY) >= DISCRETE_WHEEL_PIXEL_THRESHOLD
  return discrete ? multiplier : 1
}
