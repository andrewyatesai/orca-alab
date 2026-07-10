import { describe, expect, it, vi } from 'vitest'
import { setAtermMatrixRainActivity } from './aterm-effects-activity-gate'
import { driveAtermRainPulse } from './aterm-rain-pulse'

function harness() {
  const term = { note_matrix_rain_signal: vi.fn() }
  const scheduleDraw = vi.fn()
  return { term, scheduleDraw }
}

describe('aterm semantic rain pulse drive', () => {
  it('schedules the host canvas exactly once for an in-process engine', () => {
    const h = harness()
    setAtermMatrixRainActivity(h.term, true)

    expect(driveAtermRainPulse(h.term, { signal: 'network', weight: 5 }, h.scheduleDraw)).toBe(true)
    expect(h.term.note_matrix_rain_signal).toHaveBeenCalledWith(4, 5)
    expect(h.scheduleDraw).toHaveBeenCalledTimes(1)
  })

  it('leaves the sole render request to the worker command dispatcher', () => {
    const h = harness()
    setAtermMatrixRainActivity(h.term, true)

    expect(driveAtermRainPulse(h.term, { signal: 'execute', weight: 6 })).toBe(true)
    expect(h.term.note_matrix_rain_signal).toHaveBeenCalledWith(3, 6)
    expect(h.scheduleDraw).not.toHaveBeenCalled()
  })

  it('does not retain or schedule a pulse while reduced motion gates rain', () => {
    const h = harness()
    setAtermMatrixRainActivity(h.term, false)

    expect(driveAtermRainPulse(h.term, { signal: 'failure', weight: 8 }, h.scheduleDraw)).toBe(
      false
    )
    expect(h.term.note_matrix_rain_signal).not.toHaveBeenCalled()
    expect(h.scheduleDraw).not.toHaveBeenCalled()

    setAtermMatrixRainActivity(h.term, true)
    driveAtermRainPulse(h.term, { signal: 'inspect', weight: 3 }, h.scheduleDraw)
    expect(h.term.note_matrix_rain_signal).toHaveBeenCalledTimes(1)
    expect(h.term.note_matrix_rain_signal).toHaveBeenCalledWith(1, 3)
  })

  it('drops cleanly without scheduling when an older engine lacks the pulse method', () => {
    const term = {}
    const scheduleDraw = vi.fn()
    setAtermMatrixRainActivity(term, true)

    expect(driveAtermRainPulse(term, { signal: 'execute', weight: 6 }, scheduleDraw)).toBe(false)
    expect(scheduleDraw).not.toHaveBeenCalled()
  })
})
