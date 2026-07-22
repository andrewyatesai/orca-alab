// Plain-language session status for the coordinator grid, derived ONLY from
// what the daemon already reports (coordinator-v0-design.md: read, don't
// infer): getForegroundProcess, the exit stream event, and liveness.
import { isShellProcess } from '../../shared/shell-process-detection'

export type CoordinatorSessionStatus = 'working' | 'needs-you' | 'done' | 'failed' | 'ended'

export type SessionStatusSignals = {
  isAlive: boolean
  /** From the exit stream event; null while alive or when the session vanished
   *  from listSessions without one. */
  exitCode: number | null
  /** getForegroundProcess: the PTY foreground process-group command name. */
  foregroundProcess: string | null
}

export function deriveSessionStatus(signals: SessionStatusSignals): CoordinatorSessionStatus {
  if (!signals.isAlive) {
    if (signals.exitCode === null) {
      // Why: vanished with no exit event (daemon restart, reap during a stream
      // disconnect) is an unknown outcome — never the green done treatment.
      return 'ended'
    }
    return signals.exitCode === 0 ? 'done' : 'failed'
  }
  if (signals.foregroundProcess !== null && isShellProcess(signals.foregroundProcess)) {
    // The shell itself owns the PTY foreground → the session sits at a prompt
    // waiting for a human. That is the attention state.
    return 'needs-you'
  }
  return 'working'
}

/** Compact "time since last activity" for the tile: 'now', '42s', '5m', '3h', '2d'. */
export function formatTimeSinceActivity(nowMs: number, lastActivityMs: number): string {
  const elapsedSeconds = Math.max(0, Math.floor((nowMs - lastActivityMs) / 1000))
  if (elapsedSeconds < 10) {
    return 'now'
  }
  if (elapsedSeconds < 60) {
    return `${elapsedSeconds}s`
  }
  const minutes = Math.floor(elapsedSeconds / 60)
  if (minutes < 60) {
    return `${minutes}m`
  }
  const hours = Math.floor(minutes / 60)
  if (hours < 24) {
    return `${hours}h`
  }
  return `${Math.floor(hours / 24)}d`
}

type AttentionCandidate = {
  status: CoordinatorSessionStatus
  lastActivityAt: number
}

/** The attention queue: sessions needing input or finished — needs-you first,
 *  then done/failed/ended, newest activity first within each band (the design's
 *  "needs-you/done first, newest first"). Working sessions never queue. */
export function orderAttentionQueue<T extends AttentionCandidate>(sessions: readonly T[]): T[] {
  const band = (status: CoordinatorSessionStatus): number => (status === 'needs-you' ? 0 : 1)
  return sessions
    .filter((session) => session.status !== 'working')
    .sort((a, b) => band(a.status) - band(b.status) || b.lastActivityAt - a.lastActivityAt)
}
