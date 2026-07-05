import { describe, expect, it } from 'vitest'
import {
  DAEMON_DEGRADED_FALLBACK_REASONS,
  DAEMON_LAUNCH_FAILURE_CLASSES,
  RENDERER_PROCESS_GONE_REASONS,
  TERMINAL_GPU_DOWNGRADE_REASONS,
  daemonDegradedFallbackSchema,
  daemonLaunchFailedSchema,
  rendererProcessGoneSchema,
  rendererProcessGoneTelemetryReason,
  terminalGpuDowngradeSchema
} from './fork-reliability-telemetry'
import { eventSchemas } from './telemetry-events'

describe('fork reliability event schemas', () => {
  it('registers all four events in the eventSchemas roster', () => {
    expect(eventSchemas.daemon_launch_failed).toBe(daemonLaunchFailedSchema)
    expect(eventSchemas.daemon_degraded_fallback).toBe(daemonDegradedFallbackSchema)
    expect(eventSchemas.terminal_gpu_downgrade).toBe(terminalGpuDowngradeSchema)
    expect(eventSchemas.renderer_process_gone).toBe(rendererProcessGoneSchema)
  })

  it('accepts every declared enum value', () => {
    for (const errorClass of DAEMON_LAUNCH_FAILURE_CLASSES) {
      expect(daemonLaunchFailedSchema.safeParse({ error_class: errorClass }).success).toBe(true)
    }
    for (const reason of DAEMON_DEGRADED_FALLBACK_REASONS) {
      expect(daemonDegradedFallbackSchema.safeParse({ reason }).success).toBe(true)
    }
    for (const reason of TERMINAL_GPU_DOWNGRADE_REASONS) {
      expect(terminalGpuDowngradeSchema.safeParse({ from: 'gpu', to: 'cpu', reason }).success).toBe(
        true
      )
    }
    for (const reason of RENDERER_PROCESS_GONE_REASONS) {
      expect(rendererProcessGoneSchema.safeParse({ reason }).success).toBe(true)
    }
  })

  it('rejects free-form strings so no payload can carry PII or terminal content', () => {
    expect(
      daemonLaunchFailedSchema.safeParse({ error_class: '/Users/someone/orca-daemon: ENOENT' })
        .success
    ).toBe(false)
    expect(daemonDegradedFallbackSchema.safeParse({ reason: 'some raw error text' }).success).toBe(
      false
    )
    expect(rendererProcessGoneSchema.safeParse({ reason: 'segfault at 0x0' }).success).toBe(false)
  })

  it('rejects extra keys (.strict() discipline)', () => {
    expect(
      daemonLaunchFailedSchema.safeParse({ error_class: 'binary_missing', detail: 'x' }).success
    ).toBe(false)
    expect(
      terminalGpuDowngradeSchema.safeParse({
        from: 'worker',
        to: 'in_process',
        reason: 'worker_init_failed',
        adapter: 'ANGLE'
      }).success
    ).toBe(false)
  })

  it('accepts the two render-path downgrade transitions the strategy selector emits', () => {
    expect(
      terminalGpuDowngradeSchema.safeParse({
        from: 'worker',
        to: 'in_process',
        reason: 'worker_init_failed'
      }).success
    ).toBe(true)
    expect(
      terminalGpuDowngradeSchema.safeParse({
        from: 'gpu',
        to: 'cpu',
        reason: 'gpu_init_timeout'
      }).success
    ).toBe(true)
  })
})

describe('rendererProcessGoneTelemetryReason', () => {
  it("maps each of Electron's hyphenated reasons onto the wire enum", () => {
    expect(rendererProcessGoneTelemetryReason('clean-exit')).toBe('clean_exit')
    expect(rendererProcessGoneTelemetryReason('abnormal-exit')).toBe('abnormal_exit')
    expect(rendererProcessGoneTelemetryReason('killed')).toBe('killed')
    expect(rendererProcessGoneTelemetryReason('crashed')).toBe('crashed')
    expect(rendererProcessGoneTelemetryReason('oom')).toBe('oom')
    expect(rendererProcessGoneTelemetryReason('launch-failed')).toBe('launch_failed')
    expect(rendererProcessGoneTelemetryReason('integrity-failure')).toBe('integrity_failure')
  })

  it('buckets unrecognized values to unknown instead of dropping at the strict validator', () => {
    expect(rendererProcessGoneTelemetryReason('some-future-reason')).toBe('unknown')
    expect(rendererProcessGoneTelemetryReason('')).toBe('unknown')
  })
})
