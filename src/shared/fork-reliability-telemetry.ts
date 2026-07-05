// Fork-owned reliability telemetry: the closed-enum event family that gives
// the staging cohort remote visibility into the fork's actual risk surfaces
// (Rust daemon launch/degradation, aterm render-path downgrades, renderer
// process loss). Payloads are enum-only by design so no free-form string can
// carry PII, paths, or terminal content — matching the `.strict()` privacy
// discipline in `telemetry-events.ts`. Emit sites and the daemon hook points
// are documented in docs/reference/staging-observability.md.

import { z } from 'zod'

export const DAEMON_LAUNCH_FAILURE_CLASSES = [
  'binary_missing',
  'not_executable',
  'spawn_failed',
  'handshake_timeout',
  'socket_error',
  'unsupported_platform',
  'unknown'
] as const

export const DAEMON_DEGRADED_FALLBACK_REASONS = [
  // Total daemon launch failure — terminals silently run on LocalPtyProvider.
  'launch_failed',
  // A preserved daemon answered but cannot host fresh terminals
  // (`degraded-new-pty-fallback` launch mode).
  'preserved_unhealthy',
  // The daemon socket dropped after a healthy start.
  'socket_lost',
  'unknown'
] as const

export const TERMINAL_RENDER_DOWNGRADE_FROM = ['worker', 'gpu'] as const
export const TERMINAL_RENDER_DOWNGRADE_TO = ['in_process', 'cpu'] as const
export const TERMINAL_GPU_DOWNGRADE_REASONS = [
  'worker_init_failed',
  'gpu_init_failed',
  'gpu_init_timeout'
] as const

// Electron's `RenderProcessGoneDetails.reason` values, snake_cased for the
// telemetry wire, plus an `unknown` catch-all so a future Electron enum
// addition degrades to a bucketed value instead of a dropped event.
export const RENDERER_PROCESS_GONE_REASONS = [
  'clean_exit',
  'abnormal_exit',
  'killed',
  'crashed',
  'oom',
  'launch_failed',
  'integrity_failure',
  'unknown'
] as const

export const daemonLaunchFailureClassSchema = z.enum(DAEMON_LAUNCH_FAILURE_CLASSES)
export const daemonDegradedFallbackReasonSchema = z.enum(DAEMON_DEGRADED_FALLBACK_REASONS)
export const terminalGpuDowngradeReasonSchema = z.enum(TERMINAL_GPU_DOWNGRADE_REASONS)
export const rendererProcessGoneReasonSchema = z.enum(RENDERER_PROCESS_GONE_REASONS)

export type DaemonLaunchFailureClass = z.infer<typeof daemonLaunchFailureClassSchema>
export type DaemonDegradedFallbackReason = z.infer<typeof daemonDegradedFallbackReasonSchema>
export type TerminalGpuDowngradeReason = z.infer<typeof terminalGpuDowngradeReasonSchema>
export type RendererProcessGoneReason = z.infer<typeof rendererProcessGoneReasonSchema>

export const daemonLaunchFailedSchema = z
  .object({ error_class: daemonLaunchFailureClassSchema })
  .strict()

export const daemonDegradedFallbackSchema = z
  .object({ reason: daemonDegradedFallbackReasonSchema })
  .strict()

export const terminalGpuDowngradeSchema = z
  .object({
    from: z.enum(TERMINAL_RENDER_DOWNGRADE_FROM),
    to: z.enum(TERMINAL_RENDER_DOWNGRADE_TO),
    reason: terminalGpuDowngradeReasonSchema
  })
  .strict()

export const rendererProcessGoneSchema = z
  .object({ reason: rendererProcessGoneReasonSchema })
  .strict()

/** Map Electron's hyphenated `RenderProcessGoneDetails.reason` onto the wire
 *  enum; unrecognized values bucket to `unknown` rather than dropping the
 *  whole event at the strict validator. */
export function rendererProcessGoneTelemetryReason(reason: string): RendererProcessGoneReason {
  const normalized = reason.replaceAll('-', '_')
  return (RENDERER_PROCESS_GONE_REASONS as readonly string[]).includes(normalized)
    ? (normalized as RendererProcessGoneReason)
    : 'unknown'
}
