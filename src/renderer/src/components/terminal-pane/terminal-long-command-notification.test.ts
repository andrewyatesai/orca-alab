import { describe, expect, it } from 'vitest'
import {
  createLongCommandNotificationTracker,
  decideLongCommandNotification,
  type LongCommandNotificationSettings
} from './terminal-long-command-notification'

const enabledSettings: LongCommandNotificationSettings = {
  notificationsEnabled: true,
  longCommandComplete: true,
  longCommandThresholdSeconds: 15
}

describe('decideLongCommandNotification', () => {
  it('notifies when duration meets the threshold and the pane is unfocused', () => {
    const decision = decideLongCommandNotification({
      durationMs: 15_000,
      exitCode: 0,
      settings: enabledSettings,
      paneVisibleAndFocused: false
    })
    expect(decision).toEqual({ notify: true, durationMs: 15_000, exitCode: 0 })
  })

  it('does not notify below the threshold', () => {
    const decision = decideLongCommandNotification({
      durationMs: 14_999,
      exitCode: 0,
      settings: enabledSettings,
      paneVisibleAndFocused: false
    })
    expect(decision).toEqual({ notify: false })
  })

  it('does not notify when the pane is visible and focused', () => {
    const decision = decideLongCommandNotification({
      durationMs: 60_000,
      exitCode: 1,
      settings: enabledSettings,
      paneVisibleAndFocused: true
    })
    expect(decision).toEqual({ notify: false })
  })

  it.each([
    ['notifications disabled', { ...enabledSettings, notificationsEnabled: false }],
    ['long-command source disabled', { ...enabledSettings, longCommandComplete: false }],
    ['zero threshold', { ...enabledSettings, longCommandThresholdSeconds: 0 }],
    ['non-finite threshold', { ...enabledSettings, longCommandThresholdSeconds: Number.NaN }],
    ['missing settings', null]
  ])('does not notify with %s', (_label, settings) => {
    const decision = decideLongCommandNotification({
      durationMs: 60_000,
      exitCode: 0,
      settings,
      paneVisibleAndFocused: false
    })
    expect(decision).toEqual({ notify: false })
  })

  it('preserves failing and missing exit codes for the notification body', () => {
    const failed = decideLongCommandNotification({
      durationMs: 20_000,
      exitCode: 130,
      settings: enabledSettings,
      paneVisibleAndFocused: false
    })
    expect(failed).toEqual({ notify: true, durationMs: 20_000, exitCode: 130 })

    const unknown = decideLongCommandNotification({
      durationMs: 20_000,
      exitCode: null,
      settings: enabledSettings,
      paneVisibleAndFocused: false
    })
    expect(unknown).toEqual({ notify: true, durationMs: 20_000, exitCode: null })
  })
})

describe('createLongCommandNotificationTracker', () => {
  function makeTracker(overrides?: {
    settings?: LongCommandNotificationSettings | null
    focused?: boolean
  }): {
    tracker: ReturnType<typeof createLongCommandNotificationTracker>
    advance: (ms: number) => void
  } {
    let nowMs = 100_000
    const tracker = createLongCommandNotificationTracker({
      getSettings: () => overrides?.settings ?? enabledSettings,
      isPaneVisibleAndFocused: () => overrides?.focused ?? false,
      now: () => nowMs
    })
    return {
      tracker,
      advance: (ms) => {
        nowMs += ms
      }
    }
  }

  it('measures OSC 133 C→D duration and notifies past the threshold', () => {
    const { tracker, advance } = makeTracker()
    tracker.onCommandStarted()
    advance(16_000)
    expect(tracker.onCommandFinished(0)).toEqual({
      notify: true,
      durationMs: 16_000,
      exitCode: 0
    })
  })

  it('does not notify for a D mark without an observed C start', () => {
    const { tracker, advance } = makeTracker()
    advance(60_000)
    expect(tracker.onCommandFinished(0)).toEqual({ notify: false })
  })

  it('consumes the start mark so a second D cannot reuse a stale duration', () => {
    const { tracker, advance } = makeTracker()
    tracker.onCommandStarted()
    advance(20_000)
    expect(tracker.onCommandFinished(1).notify).toBe(true)
    advance(20_000)
    expect(tracker.onCommandFinished(0)).toEqual({ notify: false })
  })

  it('restarts timing from the most recent C mark', () => {
    const { tracker, advance } = makeTracker()
    tracker.onCommandStarted()
    advance(60_000)
    tracker.onCommandStarted()
    advance(10_000)
    expect(tracker.onCommandFinished(0)).toEqual({ notify: false })
  })

  it('reset drops an in-flight start mark', () => {
    const { tracker, advance } = makeTracker()
    tracker.onCommandStarted()
    advance(60_000)
    tracker.reset()
    expect(tracker.onCommandFinished(0)).toEqual({ notify: false })
  })

  it('gates on live settings at finish time', () => {
    const { tracker, advance } = makeTracker({
      settings: { ...enabledSettings, longCommandComplete: false }
    })
    tracker.onCommandStarted()
    advance(60_000)
    expect(tracker.onCommandFinished(0)).toEqual({ notify: false })
  })

  it('does not notify when the pane is the visible focused pane', () => {
    const { tracker, advance } = makeTracker({ focused: true })
    tracker.onCommandStarted()
    advance(60_000)
    expect(tracker.onCommandFinished(0)).toEqual({ notify: false })
  })
})
