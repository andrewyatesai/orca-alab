import { describe, expect, it } from 'vitest'
import { parseCriterionOutput } from './criterion-output-parse.mjs'

// Layouts copied from real `cargo bench -p aterm-bench` output: the bench id
// appears either on the `time:` line itself or alone on the line above it.
const SAME_LINE = `
Benchmarking engine_throughput/ascii: Analyzing
engine_throughput/ascii time:   [1.2181 ms 1.2185 ms 1.2189 ms]
Found 11 outliers among 100 measurements (11.00%)
`

const ID_ABOVE_WITH_THRPT = `
Benchmarking comparative/alacritty/ascii: Analyzing
comparative/alacritty/ascii
                        time:   [3.8338 ms 3.8411 ms 3.8482 ms]
                        thrpt:  [259.87 MiB/s 260.35 MiB/s 260.84 MiB/s]
`

const MICROS_AND_GIBS = `
comparative/vte-parser-only/ascii
                        time:   [285.07 µs 287.03 µs 288.95 µs]
                        thrpt:  [3.3798 GiB/s 3.4024 GiB/s 3.4257 GiB/s]
`

describe('parseCriterionOutput', () => {
  it('parses the id-on-time-line layout', () => {
    const benches = parseCriterionOutput(SAME_LINE)
    expect(benches['engine_throughput/ascii']).toEqual({
      medianMs: 1.2185,
      mibPerSec: null
    })
  })

  it('parses the id-above-time layout and the throughput triple', () => {
    const benches = parseCriterionOutput(ID_ABOVE_WITH_THRPT)
    expect(benches['comparative/alacritty/ascii'].medianMs).toBeCloseTo(3.8411, 4)
    expect(benches['comparative/alacritty/ascii'].mibPerSec).toBeCloseTo(260.35, 2)
  })

  it('normalizes µs to ms and GiB/s to MiB/s', () => {
    const benches = parseCriterionOutput(MICROS_AND_GIBS)
    const bench = benches['comparative/vte-parser-only/ascii']
    expect(bench.medianMs).toBeCloseTo(0.28703, 5)
    expect(bench.mibPerSec).toBeCloseTo(3.4024 * 1024, 1)
  })

  it('does not misattribute the Benchmarking/Analyzing progress lines', () => {
    const benches = parseCriterionOutput(SAME_LINE + ID_ABOVE_WITH_THRPT)
    expect(Object.keys(benches).sort()).toEqual([
      'comparative/alacritty/ascii',
      'engine_throughput/ascii'
    ])
  })

  it('returns an empty map for noise-only output', () => {
    expect(parseCriterionOutput('warning: nothing to see\nFinished bench profile\n')).toEqual({})
  })
})
