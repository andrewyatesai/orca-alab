// Emit helpers for the fork reliability event family. Daemon and window code
// call these named helpers instead of importing the PostHog client directly,
// so the enum-only payload discipline is enforced at one seam.
//
// Wired today: `trackRendererProcessGone` (createMainWindow render-process-
// gone handler). The two daemon helpers are exported for the daemon startup /
// fallback paths — the exact hook points are documented in
// docs/reference/staging-observability.md and land with the daemon
// failure-visibility workstream, which owns those files.

import {
  rendererProcessGoneTelemetryReason,
  type DaemonDegradedFallbackReason,
  type DaemonLaunchFailureClass
} from '../../shared/fork-reliability-telemetry'
import { track } from './client'

/** Bucket a daemon launch error into the closed `error_class` enum. Only the
 *  bucket crosses the wire — never the message, which can carry paths. */
export function classifyDaemonLaunchError(error: unknown): DaemonLaunchFailureClass {
  const code = (error as { code?: unknown } | null)?.code
  if (code === 'ENOENT') {
    return 'binary_missing'
  }
  if (code === 'EACCES' || code === 'EPERM') {
    return 'not_executable'
  }
  if (code === 'ECONNREFUSED' || code === 'ECONNRESET' || code === 'EPIPE' || code === 'ENOTSOCK') {
    return 'socket_error'
  }
  const message = error instanceof Error ? error.message : String(error)
  if (/timed?\s?out/i.test(message)) {
    return 'handshake_timeout'
  }
  if (/unsupported/i.test(message)) {
    return 'unsupported_platform'
  }
  if (/spawn/i.test(message)) {
    return 'spawn_failed'
  }
  return 'unknown'
}

export function trackDaemonLaunchFailed(errorClass: DaemonLaunchFailureClass): void {
  track('daemon_launch_failed', { error_class: errorClass })
}

export function trackDaemonDegradedFallback(reason: DaemonDegradedFallbackReason): void {
  track('daemon_degraded_fallback', { reason })
}

export function trackRendererProcessGone(details: { reason?: unknown } | undefined): void {
  // Why: telemetry must never throw into the render-process-gone handler —
  // a missing/malformed details object buckets to `unknown` instead.
  const reason = typeof details?.reason === 'string' ? details.reason : 'unknown'
  track('renderer_process_gone', { reason: rendererProcessGoneTelemetryReason(reason) })
}
