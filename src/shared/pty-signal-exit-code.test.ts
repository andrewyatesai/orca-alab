import { describe, expect, it } from 'vitest'
import { resolvePtyExitCode } from './pty-signal-exit-code'

describe('resolvePtyExitCode', () => {
  it('passes through a normal exit code when no signal is present', () => {
    expect(resolvePtyExitCode({ exitCode: 0 })).toBe(0)
    expect(resolvePtyExitCode({ exitCode: 42 })).toBe(42)
  })

  // Why: node-pty reports exitCode 0 for signaled children (WIFEXITED-only); without 128+signal a SIGKILL/OOM death reads as a clean exit.
  it('encodes a signal-terminated exit as POSIX 128+signal', () => {
    expect(resolvePtyExitCode({ exitCode: 0, signal: 9 })).toBe(137)
    expect(resolvePtyExitCode({ exitCode: 0, signal: 11 })).toBe(139)
    expect(resolvePtyExitCode({ exitCode: 0, signal: 15 })).toBe(143)
  })

  it('treats signal 0 or undefined as not signaled', () => {
    expect(resolvePtyExitCode({ exitCode: 7, signal: 0 })).toBe(7)
    expect(resolvePtyExitCode({ exitCode: 7, signal: undefined })).toBe(7)
  })
})
