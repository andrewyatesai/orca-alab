const XTERM_MOUSE_REPORTING_CLASS = 'enable-mouse-events'
const REPLAYED_WHEEL_EVENT_PROPERTY = '__orcaReplayedTerminalWheelEvent'
const DOM_DELTA_PIXEL = 0

export const TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER = 3
export const TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MIN = 1
export const TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MAX = 10

type ReplayedWheelEvent = WheelEvent & {
  [REPLAYED_WHEEL_EVENT_PROPERTY]?: boolean
}

function isReplayedWheelEvent(event: WheelEvent): boolean {
  return (event as ReplayedWheelEvent)[REPLAYED_WHEEL_EVENT_PROPERTY] === true
}

function isDiscreteWheelEvent(event: WheelEvent): boolean {
  if (event.deltaMode !== DOM_DELTA_PIXEL) {
    return true
  }

  return Math.abs(event.deltaY) >= 50
}

export function shouldMultiplyTerminalMouseWheel(
  event: WheelEvent,
  terminalElement: HTMLElement | null | undefined
): boolean {
  if (
    isReplayedWheelEvent(event) ||
    !terminalElement?.classList.contains(XTERM_MOUSE_REPORTING_CLASS) ||
    event.deltaY === 0 ||
    event.shiftKey ||
    !isDiscreteWheelEvent(event)
  ) {
    return false
  }

  return true
}

export function normalizeTerminalTuiMouseWheelMultiplier(value: number | undefined): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER
  }
  return Math.round(
    Math.min(
      TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MAX,
      Math.max(TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MIN, value)
    )
  )
}
