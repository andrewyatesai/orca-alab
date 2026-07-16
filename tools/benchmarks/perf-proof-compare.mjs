/**
 * Pure comparison logic for the perf-proof gate: latest run metrics vs the
 * last committed trend entry. Kept side-effect free so the >15% regression
 * rules are unit-testable apart from the perf-proof-check.mjs CLI.
 */
import { PERF_PROOF_METRICS, REGRESSION_THRESHOLD } from './perf-proof-metrics.mjs'

/**
 * Compare one metric pair under the catalog's policy/direction rules.
 * Returns { status, deltaPct } where status is one of:
 *   'ok'         — within threshold (or an improvement)
 *   'regressed'  — worse than baseline beyond the threshold (fails the gate)
 *   'lost'       — gated metric present in baseline but missing now (fails:
 *                  a dead harness or lost GPU path is itself a regression)
 *   'new'        — no baseline value yet; recorded, never fails
 *   'skipped'    — absent on both sides, or unusable baseline (<= 0)
 *   'info'       — reported only, never fails, whatever the delta
 */
export function compareMetricPair(metric, baselineValue, latestValue, threshold) {
  const hasBaseline = typeof baselineValue === 'number' && Number.isFinite(baselineValue)
  const hasLatest = typeof latestValue === 'number' && Number.isFinite(latestValue)
  if (!hasBaseline && !hasLatest) {
    return { status: 'skipped', deltaPct: null }
  }
  if (!hasBaseline) {
    return { status: 'new', deltaPct: null }
  }
  if (!hasLatest) {
    if (metric.policy === 'gate') {
      return { status: 'lost', deltaPct: null }
    }
    // 'gate-if-present' and 'info' metrics may legitimately be absent (e.g. the
    // manual aterm criterion step was not run this time).
    return { status: 'skipped', deltaPct: null }
  }
  if (baselineValue <= 0) {
    // A zero/negative baseline makes the relative delta meaningless.
    return { status: 'skipped', deltaPct: null }
  }
  // Normalize so positive deltaPct always means "worse".
  const deltaPct =
    metric.direction === 'higher'
      ? (baselineValue - latestValue) / baselineValue
      : (latestValue - baselineValue) / baselineValue
  if (metric.policy === 'info') {
    return { status: 'info', deltaPct }
  }
  return { status: deltaPct > threshold ? 'regressed' : 'ok', deltaPct }
}

/**
 * Compare two extracted-metric maps across the whole catalog.
 * Returns { results, failures } where results is one row per catalog metric
 * and failures is the subset that must fail the gate.
 */
export function comparePerfMetrics(
  baselineMetrics,
  latestMetrics,
  threshold = REGRESSION_THRESHOLD
) {
  const results = PERF_PROOF_METRICS.map((metric) => {
    const baseline = baselineMetrics?.[metric.id] ?? null
    const latest = latestMetrics?.[metric.id] ?? null
    const { status, deltaPct } = compareMetricPair(metric, baseline, latest, threshold)
    return {
      id: metric.id,
      label: metric.label,
      unit: metric.unit,
      policy: metric.policy,
      direction: metric.direction,
      baseline,
      latest,
      deltaPct,
      status
    }
  })
  const failures = results.filter((r) => r.status === 'regressed' || r.status === 'lost')
  return { results, failures }
}
