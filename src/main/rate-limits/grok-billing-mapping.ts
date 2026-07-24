import type { RateLimitWindow } from '../../shared/rate-limit-types'

const WEEKLY_WINDOW_MINUTES = 10_080
const MONTHLY_WINDOW_MINUTES = 43_200

export type GrokMoneyVal = { val?: string | number }

export type GrokUsagePeriod = {
  type?: string
  start?: string
  end?: string
}

export type GrokBillingConfig = {
  creditUsagePercent?: number
  currentPeriod?: GrokUsagePeriod
  billingPeriodStart?: string
  billingPeriodEnd?: string
  subscriptionTier?: string
  monthlyLimit?: GrokMoneyVal
  used?: GrokMoneyVal
  onDemandCap?: GrokMoneyVal
  onDemandUsed?: GrokMoneyVal
  prepaidBalance?: GrokMoneyVal
  isUnifiedBillingUser?: boolean
}

export type GrokBillingResponse = GrokBillingConfig & {
  config?: GrokBillingConfig
}

function parseResetDescription(isoString: string | undefined): string | null {
  if (!isoString) {
    return null
  }
  const date = new Date(isoString)
  if (Number.isNaN(date.getTime())) {
    return null
  }
  const isToday = date.toDateString() === new Date().toDateString()
  return isToday
    ? date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
    : date.toLocaleDateString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' })
}

function timestampsMatch(left: string | undefined, right: string | undefined): boolean {
  const leftTimestamp = left ? Date.parse(left) : Number.NaN
  const rightTimestamp = right ? Date.parse(right) : Number.NaN
  return Number.isFinite(leftTimestamp) && leftTimestamp === rightTimestamp
}

function hasConfirmedWeeklyPeriod(config: GrokBillingConfig): boolean {
  const period = config.currentPeriod
  // Why: monthly unified-billing responses can also carry a weekly currentPeriod;
  // matching billing bounds identify Grok's omitted protobuf zero unambiguously.
  return (
    period?.type === 'USAGE_PERIOD_TYPE_WEEKLY' &&
    timestampsMatch(period.start, config.billingPeriodStart) &&
    timestampsMatch(period.end, config.billingPeriodEnd)
  )
}

export function mapWeeklyCredits(config: GrokBillingConfig): RateLimitWindow | null {
  const usedPercent =
    config.creditUsagePercent === undefined && hasConfirmedWeeklyPeriod(config)
      ? 0
      : config.creditUsagePercent
  if (typeof usedPercent !== 'number' || !Number.isFinite(usedPercent)) {
    return null
  }
  const periodEnd = config.currentPeriod?.end ?? config.billingPeriodEnd
  const resetsAt = periodEnd ? Date.parse(periodEnd) : null
  return {
    usedPercent: Math.min(100, Math.max(0, usedPercent)),
    windowMinutes: WEEKLY_WINDOW_MINUTES,
    resetsAt: resetsAt !== null && Number.isFinite(resetsAt) ? resetsAt : null,
    resetDescription: parseResetDescription(periodEnd)
  }
}

function parseMoneyVal(value: GrokMoneyVal | undefined): number | null {
  const raw = value?.val
  const num = typeof raw === 'string' ? Number.parseFloat(raw) : raw
  return typeof num === 'number' && Number.isFinite(num) ? num : null
}

export function mapMonthlyUsage(config: GrokBillingConfig): RateLimitWindow | null {
  const limit = parseMoneyVal(config.monthlyLimit)
  const used = parseMoneyVal(config.used)
  if (limit === null || used === null || limit <= 0) {
    return null
  }
  const periodEnd = config.currentPeriod?.end ?? config.billingPeriodEnd
  const resetsAt = periodEnd ? Date.parse(periodEnd) : null
  return {
    usedPercent: Math.min(100, Math.max(0, (used / limit) * 100)),
    windowMinutes: MONTHLY_WINDOW_MINUTES,
    resetsAt: resetsAt !== null && Number.isFinite(resetsAt) ? resetsAt : null,
    resetDescription: parseResetDescription(periodEnd)
  }
}
