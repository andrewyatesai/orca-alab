import { buildWallClockTimestamp, type WallClockDateParts } from './time-zone-wall-clock'

export const MONTH_INDEX_BY_NAME: Record<string, number> = {
  jan: 0,
  january: 0,
  feb: 1,
  february: 1,
  mar: 2,
  march: 2,
  apr: 3,
  april: 3,
  may: 4,
  jun: 5,
  june: 5,
  jul: 6,
  july: 6,
  aug: 7,
  august: 7,
  sep: 8,
  sept: 8,
  september: 8,
  oct: 9,
  october: 9,
  nov: 10,
  november: 10,
  dec: 11,
  december: 11
}

export const WEEKDAY_INDEX_BY_NAME: Record<string, number> = {
  sun: 0,
  sunday: 0,
  mon: 1,
  monday: 1,
  tue: 2,
  tuesday: 2,
  wed: 3,
  wednesday: 3,
  thu: 4,
  thursday: 4,
  fri: 5,
  friday: 5,
  sat: 6,
  saturday: 6
}

export type ZonedCalendarDay = { year: number; monthIndex: number; day: number; weekday: number }

// Why: all absolute-time reset branches share one contract — build the wall-clock
// instant in the resolved zone, then roll forward by the period if it already
// passed — so honoring the IANA zone can never drift between branches.
export function buildRollingResetTimestamp(
  parts: WallClockDateParts,
  timeZone: string | null,
  advance: (parts: WallClockDateParts) => WallClockDateParts
): number | null {
  const timestamp = buildWallClockTimestamp(parts, timeZone)
  if (timestamp === null || timestamp > Date.now()) {
    return timestamp
  }
  return buildWallClockTimestamp(advance(parts), timeZone)
}

// Why: reset weekday/time are relative to the zone's current calendar day, so
// resolve year/month/day/day-of-week in that zone (local when timeZone is null).
export function getZonedNowParts(timeZone: string | null): ZonedCalendarDay {
  const now = new Date()
  if (!timeZone) {
    return {
      year: now.getFullYear(),
      monthIndex: now.getMonth(),
      day: now.getDate(),
      weekday: now.getDay()
    }
  }
  const formatter = new Intl.DateTimeFormat('en-US', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    weekday: 'short'
  })
  const parts = Object.fromEntries(
    formatter.formatToParts(now).map((part) => [part.type, part.value])
  )
  const weekday = WEEKDAY_INDEX_BY_NAME[String(parts.weekday ?? '').toLowerCase()]
  return {
    year: Number(parts.year),
    monthIndex: Number(parts.month) - 1,
    day: Number(parts.day),
    weekday: weekday ?? now.getDay()
  }
}

// Why: normalize month/year rollover via UTC calendar math so buildWallClockTimestamp
// never receives an out-of-range day (e.g. day 32) that it would reject as invalid.
export function shiftCalendarDays<T extends { year: number; monthIndex: number; day: number }>(
  parts: T,
  days: number
): T {
  const shifted = new Date(Date.UTC(parts.year, parts.monthIndex, parts.day + days))
  return {
    ...parts,
    year: shifted.getUTCFullYear(),
    monthIndex: shifted.getUTCMonth(),
    day: shifted.getUTCDate()
  }
}
