import type { GlobalSettings } from './types'

export const AUTO_FETCH_DEFAULT_INTERVAL_MINUTES = 5
export const AUTO_FETCH_MIN_INTERVAL_MINUTES = 1
export const AUTO_FETCH_MAX_INTERVAL_MINUTES = 120

export type GitAutoFetchSettings = {
  enabled: boolean
  intervalMinutes: number
}

/** Resolve the automatic-fetch settings with clamped defaults (issue #6258).
 *  Off by default: periodic network traffic must be an explicit opt-in. */
export function resolveGitAutoFetchSettings(
  settings: Pick<GlobalSettings, 'autoFetchEnabled' | 'autoFetchIntervalMinutes'> | undefined
): GitAutoFetchSettings {
  const rawInterval = settings?.autoFetchIntervalMinutes
  const intervalMinutes =
    typeof rawInterval === 'number' && Number.isFinite(rawInterval)
      ? Math.min(
          AUTO_FETCH_MAX_INTERVAL_MINUTES,
          Math.max(AUTO_FETCH_MIN_INTERVAL_MINUTES, Math.round(rawInterval))
        )
      : AUTO_FETCH_DEFAULT_INTERVAL_MINUTES
  return {
    enabled: settings?.autoFetchEnabled === true,
    intervalMinutes
  }
}
