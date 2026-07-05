import { beforeEach, describe, expect, it, vi } from 'vitest'

const { trackMock } = vi.hoisted(() => ({ trackMock: vi.fn() }))

vi.mock('./client', () => ({ track: trackMock }))

import {
  classifyDaemonLaunchError,
  trackDaemonDegradedFallback,
  trackDaemonLaunchFailed,
  trackRendererProcessGone
} from './fork-reliability-events'

function errnoError(message: string, code: string): Error {
  const err = new Error(message) as Error & { code: string }
  err.code = code
  return err
}

describe('classifyDaemonLaunchError', () => {
  it('buckets errno codes', () => {
    expect(classifyDaemonLaunchError(errnoError('spawn ENOENT', 'ENOENT'))).toBe('binary_missing')
    expect(classifyDaemonLaunchError(errnoError('spawn EACCES', 'EACCES'))).toBe('not_executable')
    expect(classifyDaemonLaunchError(errnoError('spawn EPERM', 'EPERM'))).toBe('not_executable')
    expect(classifyDaemonLaunchError(errnoError('connect ECONNREFUSED', 'ECONNREFUSED'))).toBe(
      'socket_error'
    )
    expect(classifyDaemonLaunchError(errnoError('write EPIPE', 'EPIPE'))).toBe('socket_error')
  })

  it('buckets message shapes when no errno code is present', () => {
    expect(classifyDaemonLaunchError(new Error('daemon hello timed out after 10000ms'))).toBe(
      'handshake_timeout'
    )
    expect(classifyDaemonLaunchError(new Error('Daemon launch timeout'))).toBe('handshake_timeout')
    expect(classifyDaemonLaunchError(new Error('Unsupported platform: win32'))).toBe(
      'unsupported_platform'
    )
    expect(classifyDaemonLaunchError(new Error('failed to spawn daemon process'))).toBe(
      'spawn_failed'
    )
  })

  it('falls back to unknown for anything else, including non-Error values', () => {
    expect(classifyDaemonLaunchError(new Error('something odd'))).toBe('unknown')
    expect(classifyDaemonLaunchError('string failure')).toBe('unknown')
    expect(classifyDaemonLaunchError(null)).toBe('unknown')
    expect(classifyDaemonLaunchError(undefined)).toBe('unknown')
  })
})

describe('emit helpers', () => {
  beforeEach(() => {
    trackMock.mockReset()
  })

  it('trackDaemonLaunchFailed emits the enum-only payload', () => {
    trackDaemonLaunchFailed('binary_missing')
    expect(trackMock).toHaveBeenCalledWith('daemon_launch_failed', {
      error_class: 'binary_missing'
    })
  })

  it('trackDaemonDegradedFallback emits the enum-only payload', () => {
    trackDaemonDegradedFallback('preserved_unhealthy')
    expect(trackMock).toHaveBeenCalledWith('daemon_degraded_fallback', {
      reason: 'preserved_unhealthy'
    })
  })

  it('trackRendererProcessGone maps the Electron reason onto the wire enum', () => {
    trackRendererProcessGone({ reason: 'abnormal-exit' })
    expect(trackMock).toHaveBeenCalledWith('renderer_process_gone', { reason: 'abnormal_exit' })
  })

  it('trackRendererProcessGone never throws on missing or malformed details', () => {
    trackRendererProcessGone(undefined)
    expect(trackMock).toHaveBeenCalledWith('renderer_process_gone', { reason: 'unknown' })
    trackMock.mockReset()
    trackRendererProcessGone({ reason: 42 })
    expect(trackMock).toHaveBeenCalledWith('renderer_process_gone', { reason: 'unknown' })
  })
})
