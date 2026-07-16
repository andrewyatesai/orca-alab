import { describe, expect, it } from 'vitest'
import { compareMetricPair, comparePerfMetrics } from './perf-proof-compare.mjs'
import { extractMetrics, PERF_PROOF_METRICS } from './perf-proof-metrics.mjs'

const lowerGate = { id: 'm', direction: 'lower', policy: 'gate' }
const higherGate = { id: 'm', direction: 'higher', policy: 'gate' }
const THRESHOLD = 0.15

describe('compareMetricPair', () => {
  it('passes within the threshold and fails beyond it (lower-is-better)', () => {
    expect(compareMetricPair(lowerGate, 100, 114.9, THRESHOLD).status).toBe('ok')
    // Exactly at the threshold is NOT a regression (strictly greater fails).
    expect(compareMetricPair(lowerGate, 100, 115, THRESHOLD).status).toBe('ok')
    expect(compareMetricPair(lowerGate, 100, 115.1, THRESHOLD).status).toBe('regressed')
  })

  it('normalizes higher-is-better so a throughput drop regresses', () => {
    const drop = compareMetricPair(higherGate, 880, 700, THRESHOLD)
    expect(drop.status).toBe('regressed')
    expect(drop.deltaPct).toBeGreaterThan(0)
    expect(compareMetricPair(higherGate, 880, 1000, THRESHOLD).status).toBe('ok')
  })

  it('treats improvements as ok, never regressed', () => {
    const improved = compareMetricPair(lowerGate, 100, 40, THRESHOLD)
    expect(improved.status).toBe('ok')
    expect(improved.deltaPct).toBeLessThan(0)
  })

  it('fails a gated metric that vanished from the latest run', () => {
    expect(compareMetricPair(lowerGate, 100, null, THRESHOLD).status).toBe('lost')
  })

  it('skips a gate-if-present metric that vanished (manual step not run)', () => {
    const metric = { ...lowerGate, policy: 'gate-if-present' }
    expect(compareMetricPair(metric, 100, null, THRESHOLD).status).toBe('skipped')
  })

  it('never fails an info metric, whatever the delta', () => {
    const metric = { ...lowerGate, policy: 'info' }
    expect(compareMetricPair(metric, 100, 900, THRESHOLD).status).toBe('info')
  })

  it('marks a metric with no baseline as new, and both-absent as skipped', () => {
    expect(compareMetricPair(lowerGate, null, 5, THRESHOLD).status).toBe('new')
    expect(compareMetricPair(lowerGate, null, null, THRESHOLD).status).toBe('skipped')
  })

  it('skips a zero baseline instead of dividing by it', () => {
    expect(compareMetricPair(lowerGate, 0, 5, THRESHOLD).status).toBe('skipped')
  })
})

describe('comparePerfMetrics over the catalog', () => {
  const baseRun = {
    keydown: { stats: { medianMs: 4, p95Ms: 9 } },
    latency: { renderHalf: { cpu: { medianMs: 1.2 }, gpu: { medianMs: 0.4 } } },
    gpuCpu: {
      rows: [
        {
          cols: 80,
          rows: 24,
          sparse: { cpu: { msPerFrame: 2.0 }, gpu: { msPerFrame: 0.3 } },
          full: { cpu: { msPerFrame: 7.0 }, gpu: { msPerFrame: 0.5 } }
        },
        {
          cols: 200,
          rows: 50,
          sparse: { cpu: { msPerFrame: 8.0 }, gpu: { msPerFrame: 0.4 } },
          full: { cpu: { msPerFrame: 30.0 }, gpu: { msPerFrame: 0.9 } }
        }
      ]
    },
    memory: { kbPerPane: 900 },
    startup: { summaryMedianMs: { totalToDidFinishLoad: 2500, totalToWorkspaceReady: null } },
    engine: {
      'engine_throughput/ascii': { medianMs: 1.22, mibPerSec: null },
      'comparative/aterm/ascii': { medianMs: 1.13, mibPerSec: 883.6 }
    }
  }

  it('passes when the latest run matches the baseline', () => {
    const metrics = extractMetrics(baseRun)
    const { failures } = comparePerfMetrics(metrics, metrics)
    expect(failures).toEqual([])
  })

  it('fails when a gated frame-cost cell regresses >15%', () => {
    const regressed = structuredClone(baseRun)
    regressed.gpuCpu.rows[1].full.gpu.msPerFrame = 0.9 * 1.2
    const { failures } = comparePerfMetrics(extractMetrics(baseRun), extractMetrics(regressed))
    expect(failures.map((f) => f.id)).toEqual(['gpu_frame_200x50_full_ms'])
  })

  it('fails when the GPU path is lost entirely (gated metric goes missing)', () => {
    const lost = structuredClone(baseRun)
    lost.gpuCpu.rows[0].sparse.gpu = null
    lost.latency.renderHalf.gpu = null
    const { failures } = comparePerfMetrics(extractMetrics(baseRun), extractMetrics(lost))
    expect(failures.map((f) => f.status)).toContain('lost')
    expect(failures.map((f) => f.id)).toContain('gpu_frame_80x24_sparse_ms')
    expect(failures.map((f) => f.id)).toContain('render_half_gpu_p50_ms')
  })

  it('does not fail when the manual engine step was skipped this run', () => {
    const withoutEngine = structuredClone(baseRun)
    withoutEngine.engine = null
    const { failures, results } = comparePerfMetrics(
      extractMetrics(baseRun),
      extractMetrics(withoutEngine)
    )
    expect(failures).toEqual([])
    const engineRow = results.find((r) => r.id === 'engine_throughput_ascii_ms')
    expect(engineRow?.status).toBe('skipped')
  })

  it('fails when the engine throughput (higher-is-better) drops >15% on a run that did measure it', () => {
    const slower = structuredClone(baseRun)
    slower.engine['comparative/aterm/ascii'].mibPerSec = 883.6 * 0.8
    const { failures } = comparePerfMetrics(extractMetrics(baseRun), extractMetrics(slower))
    expect(failures.map((f) => f.id)).toEqual(['engine_comparative_aterm_ascii_mibps'])
  })

  it('keeps noisy percentiles informational', () => {
    const noisy = structuredClone(baseRun)
    noisy.keydown.stats.p95Ms = 90
    const { failures, results } = comparePerfMetrics(extractMetrics(baseRun), extractMetrics(noisy))
    expect(failures).toEqual([])
    expect(results.find((r) => r.id === 'keydown_to_frame_p95_ms')?.status).toBe('info')
  })

  it('covers every catalog metric exactly once', () => {
    const { results } = comparePerfMetrics({}, {})
    expect(results.map((r) => r.id)).toEqual(PERF_PROOF_METRICS.map((m) => m.id))
  })
})
