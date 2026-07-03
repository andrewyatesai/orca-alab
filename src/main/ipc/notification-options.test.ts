import { describe, expect, it } from 'vitest'
import type { NotificationDispatchRequest } from '../../shared/types'
import {
  buildNotificationOptions,
  formatCommandDuration,
  formatCommandExitStatus
} from './notification-options'

function longCommandRequest(
  overrides: Partial<NotificationDispatchRequest> = {}
): NotificationDispatchRequest {
  return {
    source: 'long-command-complete',
    worktreeId: 'repo::wt',
    worktreeLabel: 'feature-branch',
    repoLabel: 'orc',
    terminalTitle: 'pnpm build',
    commandDurationMs: 92_000,
    commandExitCode: 0,
    ...overrides
  }
}

describe('formatCommandExitStatus', () => {
  it('formats success, failure, and missing exit codes', () => {
    expect(formatCommandExitStatus(0)).toBe('Command succeeded')
    expect(formatCommandExitStatus(130)).toBe('Command failed (exit 130)')
    expect(formatCommandExitStatus(null)).toBe('Command finished')
    expect(formatCommandExitStatus(undefined)).toBe('Command finished')
  })
})

describe('formatCommandDuration', () => {
  it('formats seconds, minutes, and hours', () => {
    expect(formatCommandDuration(0)).toBe('0s')
    expect(formatCommandDuration(15_000)).toBe('15s')
    expect(formatCommandDuration(59_400)).toBe('59s')
    expect(formatCommandDuration(60_000)).toBe('1m')
    expect(formatCommandDuration(92_000)).toBe('1m 32s')
    expect(formatCommandDuration(3_600_000)).toBe('1h')
    expect(formatCommandDuration(3_840_000)).toBe('1h 4m')
  })

  it('never reports a negative duration', () => {
    expect(formatCommandDuration(-5_000)).toBe('0s')
  })
})

describe('buildNotificationOptions long-command-complete', () => {
  it('uses the pane title and exit status + duration body', () => {
    expect(buildNotificationOptions(longCommandRequest())).toEqual({
      title: 'pnpm build',
      body: 'orc · Command succeeded in 1m 32s'
    })
  })

  it('reports failures with the exit code', () => {
    expect(buildNotificationOptions(longCommandRequest({ commandExitCode: 2 })).body).toBe(
      'orc · Command failed (exit 2) in 1m 32s'
    )
  })

  it('falls back to the worktree label when the pane title is blank', () => {
    const options = buildNotificationOptions(longCommandRequest({ terminalTitle: '  ' }))
    expect(options.title).toBe('Command finished in feature-branch')
  })

  it('omits duration and repo prefix when unavailable', () => {
    const options = buildNotificationOptions(
      longCommandRequest({
        repoLabel: undefined,
        commandDurationMs: undefined,
        commandExitCode: null
      })
    )
    expect(options.body).toBe('Command finished')
  })
})
