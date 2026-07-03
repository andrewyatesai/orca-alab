/**
 * Decision logic for "long command finished" OS notifications.
 *
 * A foreground shell command (OSC 133 C→D) that ran at least the
 * settings-controlled threshold while its pane was not the visible focused
 * pane should raise a desktop notification. Kept as a pure tracker so the
 * threshold/focus/duration rules are unit-testable without a PTY.
 */

export type LongCommandNotificationSettings = {
  notificationsEnabled: boolean
  longCommandComplete: boolean
  longCommandThresholdSeconds: number
}

export type LongCommandFinishDecision =
  | { notify: false }
  | { notify: true; durationMs: number; exitCode: number | null }

type LongCommandNotificationTrackerDeps = {
  getSettings: () => LongCommandNotificationSettings | null
  /** True when the pane is the visible foreground pane of a focused window. */
  isPaneVisibleAndFocused: () => boolean
  now?: () => number
}

export function decideLongCommandNotification(input: {
  durationMs: number
  exitCode: number | null
  settings: LongCommandNotificationSettings | null
  paneVisibleAndFocused: boolean
}): LongCommandFinishDecision {
  const { settings } = input
  if (!settings || !settings.notificationsEnabled || !settings.longCommandComplete) {
    return { notify: false }
  }
  const thresholdSeconds = settings.longCommandThresholdSeconds
  if (!Number.isFinite(thresholdSeconds) || thresholdSeconds <= 0) {
    return { notify: false }
  }
  if (input.durationMs < thresholdSeconds * 1000) {
    return { notify: false }
  }
  // Why: a command the user is actively watching finish needs no OS
  // notification; only unfocused windows / hidden panes should notify.
  if (input.paneVisibleAndFocused) {
    return { notify: false }
  }
  return { notify: true, durationMs: input.durationMs, exitCode: input.exitCode }
}

export function createLongCommandNotificationTracker(deps: LongCommandNotificationTrackerDeps): {
  onCommandStarted: () => void
  onCommandFinished: (exitCode: number | null) => LongCommandFinishDecision
  reset: () => void
} {
  const now = deps.now ?? Date.now
  let commandStartedAt: number | null = null

  return {
    onCommandStarted() {
      commandStartedAt = now()
    },
    onCommandFinished(exitCode) {
      const startedAt = commandStartedAt
      commandStartedAt = null
      // Why: without an observed OSC 133;C start mark (e.g. bare-prompt D after
      // reattach) there is no trustworthy duration, so never notify.
      if (startedAt === null) {
        return { notify: false }
      }
      return decideLongCommandNotification({
        durationMs: Math.max(0, now() - startedAt),
        exitCode,
        settings: deps.getSettings(),
        paneVisibleAndFocused: deps.isPaneVisibleAndFocused()
      })
    },
    reset() {
      commandStartedAt = null
    }
  }
}
