// TUI wheel policy facade: the multiplier/report math lives in
// pane-terminal-tui-wheel-reports (upstream #7179/#7205); the aterm seam
// (aterm-mouse-input + aterm-wheel-lines) owns actual wheel-report forwarding,
// so the xterm replay/attach machinery is intentionally absent here.
export {
  TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER,
  TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MAX,
  TERMINAL_TUI_MOUSE_WHEEL_MULTIPLIER_MIN,
  createTerminalTuiMouseWheelDistanceState,
  normalizeTerminalTuiMouseWheelMultiplier,
  resolveTerminalTuiMouseWheelReportCount
} from './pane-terminal-tui-wheel-reports'
export type { TerminalTuiMouseWheelDistanceState } from './pane-terminal-tui-wheel-reports'

const XTERM_MOUSE_REPORTING_CLASS = 'enable-mouse-events'
const REPLAYED_WHEEL_EVENT_PROPERTY = '__orcaReplayedTerminalWheelEvent'

type ReplayedWheelEvent = WheelEvent & {
  [REPLAYED_WHEEL_EVENT_PROPERTY]?: boolean
}

function isReplayedWheelEvent(event: WheelEvent): boolean {
  return (event as ReplayedWheelEvent)[REPLAYED_WHEEL_EVENT_PROPERTY] === true
}

export function shouldMultiplyTerminalMouseWheel(
  event: WheelEvent,
  terminalElement: HTMLElement | null | undefined
): boolean {
  // Why: no discrete-wheel gate (upstream #7179) — trackpad pixel streams also
  // feed the TUI report path; per-event report counts are resolved downstream.
  if (
    isReplayedWheelEvent(event) ||
    !terminalElement?.classList.contains(XTERM_MOUSE_REPORTING_CLASS) ||
    event.deltaY === 0 ||
    event.shiftKey
  ) {
    return false
  }

  return true
}
