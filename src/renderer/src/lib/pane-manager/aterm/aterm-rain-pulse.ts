import {
  ATERM_RAIN_SIGNAL_CODES,
  type AtermRainPulse
} from '../../../../../shared/aterm-rain-signal'
import { shouldNoteAtermMatrixRainActivity } from './aterm-effects-activity-gate'

type AtermRainPulseTarget = object & {
  /** Optional across an aterm artifact rollout: old generated engines do not
   * expose semantic pulses yet, and must keep rendering/status IPC normally. */
  note_matrix_rain_signal?: (code: number, weight: number) => void
}

/** Apply one payload-free pulse to a live rain engine. The activity gate already
 * combines the master switch with reduced motion. Worker engines schedule from
 * their command dispatcher; in-process engines need the host canvas draw. */
export function driveAtermRainPulse(
  term: AtermRainPulseTarget,
  pulse: AtermRainPulse,
  scheduleDraw?: () => void
): boolean {
  const noteSignal = term.note_matrix_rain_signal
  if (!shouldNoteAtermMatrixRainActivity(term) || typeof noteSignal !== 'function') {
    return false
  }
  noteSignal.call(term, ATERM_RAIN_SIGNAL_CODES[pulse.signal], pulse.weight)
  scheduleDraw?.()
  return true
}
