export const MIN_TERMINAL_LINE_HEIGHT = 1
// Why: accessibility ceiling (upstream #7934); the aterm engine only floors the
// scale at 0.5 and has no upper bound, so 10 is a UI-sanity cap, not an engine one.
export const MAX_TERMINAL_LINE_HEIGHT = 10

export function normalizeTerminalLineHeight(value: unknown): number {
  // Why: older or user-edited profiles can bypass the UI clamp, and xterm
  // throws during construction when lineHeight is below one.
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return MIN_TERMINAL_LINE_HEIGHT
  }
  return Math.min(MAX_TERMINAL_LINE_HEIGHT, Math.max(MIN_TERMINAL_LINE_HEIGHT, value))
}
