import { beforeEach, describe, expect, it } from 'vitest'
import {
  markTerminalInteractivePresentStart,
  measureTerminalFramePresented,
  TERMINAL_PERF_MARKS,
  TERMINAL_PERF_MEASURES
} from './terminal-perf-marks'

const allNames = [...Object.values(TERMINAL_PERF_MARKS), ...Object.values(TERMINAL_PERF_MEASURES)]

function measuresNamed(name: string): PerformanceEntry[] {
  return performance.getEntriesByName(name, 'measure')
}

describe('terminal perf marks family', () => {
  beforeEach(() => {
    for (const name of allNames) {
      performance.clearMarks(name)
      performance.clearMeasures(name)
    }
  })

  it('emits the interactive-present measure for a marked fast-path present', () => {
    markTerminalInteractivePresentStart()
    measureTerminalFramePresented()
    const entries = measuresNamed(TERMINAL_PERF_MEASURES.interactivePresent)
    expect(entries).toHaveLength(1)
    expect(entries[0].duration).toBeGreaterThanOrEqual(0)
  })

  it('does not throw or emit when the present fires without a start mark', () => {
    expect(() => measureTerminalFramePresented()).not.toThrow()
    expect(measuresNamed(TERMINAL_PERF_MEASURES.interactivePresent)).toHaveLength(0)
  })

  it('attributes a fresh keydown mark to exactly one presented frame', () => {
    performance.mark(TERMINAL_PERF_MARKS.keydown)
    markTerminalInteractivePresentStart()
    measureTerminalFramePresented()
    const first = measuresNamed(TERMINAL_PERF_MEASURES.keydownToFramePresented)
    expect(first).toHaveLength(1)
    expect(first[0].duration).toBeGreaterThanOrEqual(0)

    // The keydown mark was consumed: a second frame must not re-attribute it.
    // (Re-emits under the same name replace, so still exactly one entry, and it
    // must be the ORIGINAL measure, not a fresh one.)
    const firstStart = first[0].startTime
    markTerminalInteractivePresentStart()
    measureTerminalFramePresented()
    const second = measuresNamed(TERMINAL_PERF_MEASURES.keydownToFramePresented)
    expect(second).toHaveLength(1)
    expect(second[0].startTime).toBe(firstStart)
  })

  it('ignores a keydown mark older than the attribution window', () => {
    performance.mark(TERMINAL_PERF_MARKS.keydown)
    // Let the clock visibly advance so the mark is measurably old.
    const stampedAt = performance.now()
    while (performance.now() - stampedAt < 2) {
      // busy-wait ~2ms
    }
    markTerminalInteractivePresentStart()
    // Window of 0ms: any real elapsed time makes the mark stale.
    measureTerminalFramePresented(0)
    expect(measuresNamed(TERMINAL_PERF_MEASURES.keydownToFramePresented)).toHaveLength(0)
    // The stale mark stays for a later, correctly-attributed frame pair.
    expect(performance.getEntriesByName(TERMINAL_PERF_MARKS.keydown, 'mark')).toHaveLength(1)
  })

  it('keeps the user-timing buffer bounded across repeated presents', () => {
    for (let i = 0; i < 5; i++) {
      performance.mark(TERMINAL_PERF_MARKS.keydown)
      markTerminalInteractivePresentStart()
      measureTerminalFramePresented()
    }
    for (const name of [
      TERMINAL_PERF_MARKS.interactivePresentStart,
      TERMINAL_PERF_MARKS.framePresented
    ]) {
      expect(performance.getEntriesByName(name, 'mark').length).toBeLessThanOrEqual(1)
    }
    for (const name of Object.values(TERMINAL_PERF_MEASURES)) {
      expect(measuresNamed(name).length).toBeLessThanOrEqual(1)
    }
  })
})
