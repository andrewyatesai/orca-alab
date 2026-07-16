/**
 * The perf-proof metric catalog: the single source of truth for which numbers
 * the lane trends and gates, shared by perf-proof-run.mjs (reporting) and
 * perf-proof-check.mjs (the >15% regression gate).
 *
 * policy:
 *   - 'gate'            — regression fails; metric missing from the latest run
 *                         while present in the baseline ALSO fails (a lost
 *                         GPU path or dead harness is itself a regression).
 *   - 'gate-if-present' — compared only when both runs carry it (e.g. the
 *                         manual aterm criterion step, see perf-proof.md).
 *   - 'info'            — reported, never gated (known-noisy percentiles).
 * direction: 'lower' = smaller is better, 'higher' = bigger is better.
 */

export const REGRESSION_THRESHOLD = 0.15

function gpuCpuCell(run, cols, rows, mutation, pathKey) {
  const row = run?.gpuCpu?.rows?.find((r) => r.cols === cols && r.rows === rows)
  const cell = row?.[mutation]?.[pathKey]
  return typeof cell?.msPerFrame === 'number' ? cell.msPerFrame : null
}

export const PERF_PROOF_METRICS = [
  {
    id: 'keydown_to_frame_p50_ms',
    label: 'keydown→frame-presented median (typed, real pane)',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => run?.keydown?.stats?.medianMs ?? null
  },
  {
    id: 'keydown_to_frame_p95_ms',
    label: 'keydown→frame-presented p95',
    unit: 'ms',
    direction: 'lower',
    policy: 'info',
    extract: (run) => run?.keydown?.stats?.p95Ms ?? null
  },
  {
    id: 'render_half_cpu_p50_ms',
    label: 'render-half single-cell median, aterm CPU @80x24',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => run?.latency?.renderHalf?.cpu?.medianMs ?? null
  },
  {
    id: 'render_half_gpu_p50_ms',
    label: 'render-half single-cell median, aterm GPU @80x24',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => run?.latency?.renderHalf?.gpu?.medianMs ?? null
  },
  {
    id: 'cpu_frame_80x24_sparse_ms',
    label: 'CPU ms/frame @80x24 sparse',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => gpuCpuCell(run, 80, 24, 'sparse', 'cpu')
  },
  {
    id: 'gpu_frame_80x24_sparse_ms',
    label: 'GPU ms/frame @80x24 sparse',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => gpuCpuCell(run, 80, 24, 'sparse', 'gpu')
  },
  {
    id: 'gpu_frame_200x50_full_ms',
    label: 'GPU ms/frame @200x50 full-grid (scaling guard)',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => gpuCpuCell(run, 200, 50, 'full', 'gpu')
  },
  {
    id: 'wasm_kb_per_pane',
    label: 'wasm heap per live pane (grid+scrollback+fb+atlas)',
    unit: 'KB',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => run?.memory?.kbPerPane ?? null
  },
  {
    id: 'startup_did_finish_load_ms',
    label: 'startup: spawn → did-finish-load (median)',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate',
    extract: (run) => run?.startup?.summaryMedianMs?.totalToDidFinishLoad ?? null
  },
  {
    id: 'startup_workspace_ready_ms',
    label: 'startup: spawn → renderer workspace ready (median)',
    unit: 'ms',
    direction: 'lower',
    policy: 'gate-if-present',
    extract: (run) => run?.startup?.summaryMedianMs?.totalToWorkspaceReady ?? null
  },
  // aterm engine criterion benches — a named MANUAL step (docs/reference/
  // perf-proof.md), folded in via --engine-log; gated only when both runs ran it.
  ...['ascii', 'sgr', 'cjk'].flatMap((corpus) => [
    {
      id: `engine_throughput_${corpus}_ms`,
      label: `aterm engine_throughput/${corpus} (criterion median)`,
      unit: 'ms',
      direction: 'lower',
      policy: 'gate-if-present',
      extract: (run) => run?.engine?.[`engine_throughput/${corpus}`]?.medianMs ?? null
    },
    {
      id: `engine_comparative_aterm_${corpus}_mibps`,
      label: `aterm comparative/${corpus} throughput (criterion median)`,
      unit: 'MiB/s',
      direction: 'higher',
      policy: 'gate-if-present',
      extract: (run) => run?.engine?.[`comparative/aterm/${corpus}`]?.mibPerSec ?? null
    }
  ])
]

export function extractMetrics(run) {
  const metrics = {}
  for (const metric of PERF_PROOF_METRICS) {
    const value = metric.extract(run)
    metrics[metric.id] = typeof value === 'number' && Number.isFinite(value) ? value : null
  }
  return metrics
}

export function formatMetricValue(value, unit) {
  if (value === null || value === undefined) {
    return 'n/a'
  }
  const precision = value >= 100 ? 0 : value >= 10 ? 1 : 3
  return `${value.toFixed(precision)} ${unit}`
}

export function machineKeyOf(run) {
  return `${run.platform}-${run.arch}`
}
