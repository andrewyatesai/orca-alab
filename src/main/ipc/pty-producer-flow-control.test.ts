import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  PRODUCER_FLOW_HIGH_WATERMARK_CHARS,
  PRODUCER_FLOW_LOW_WATERMARK_CHARS,
  PRODUCER_PAUSE_REASSERT_INTERVAL_MS,
  PtyProducerFlowController
} from './pty-producer-flow-control'

const HIGH = PRODUCER_FLOW_HIGH_WATERMARK_CHARS
const LOW = PRODUCER_FLOW_LOW_WATERMARK_CHARS

describe('PtyProducerFlowController', () => {
  let pauseProducer: ReturnType<typeof vi.fn<(id: string) => void>>
  let resumeProducer: ReturnType<typeof vi.fn<(id: string) => void>>
  let controller: PtyProducerFlowController

  beforeEach(() => {
    vi.useFakeTimers()
    pauseProducer = vi.fn<(id: string) => void>()
    resumeProducer = vi.fn<(id: string) => void>()
    controller = new PtyProducerFlowController({
      pauseProducer,
      resumeProducer
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('does not pause at or below the high watermark', () => {
    controller.update('pty-1', 0)
    controller.update('pty-1', LOW)
    controller.update('pty-1', HIGH)
    expect(pauseProducer).not.toHaveBeenCalled()
    expect(controller.isPaused('pty-1')).toBe(false)
  })

  it('pauses exactly once when pending crosses the high watermark, not per chunk', () => {
    controller.update('pty-1', HIGH + 1)
    controller.update('pty-1', HIGH + 64 * 1024)
    controller.update('pty-1', HIGH + 128 * 1024)
    expect(pauseProducer).toHaveBeenCalledTimes(1)
    expect(pauseProducer).toHaveBeenCalledWith('pty-1')
    expect(controller.isPaused('pty-1')).toBe(true)
  })

  it('resumes exactly once when pending drains below the low watermark', () => {
    controller.update('pty-1', HIGH + 1)
    controller.update('pty-1', LOW - 1)
    expect(resumeProducer).toHaveBeenCalledTimes(1)
    expect(resumeProducer).toHaveBeenCalledWith('pty-1')
    expect(controller.isPaused('pty-1')).toBe(false)
    // A second drain report on the now-unpaused pty must not resume again.
    controller.update('pty-1', 0)
    expect(resumeProducer).toHaveBeenCalledTimes(1)
  })

  it('holds hysteresis: no flapping while pending sits between the watermarks', () => {
    controller.update('pty-1', HIGH + 1)
    expect(pauseProducer).toHaveBeenCalledTimes(1)
    // Draining but still above LOW: stay paused, no extra calls either way.
    controller.update('pty-1', HIGH - 16 * 1024)
    controller.update('pty-1', 128 * 1024)
    controller.update('pty-1', LOW)
    expect(pauseProducer).toHaveBeenCalledTimes(1)
    expect(resumeProducer).not.toHaveBeenCalled()
    expect(controller.isPaused('pty-1')).toBe(true)
    // An unpaused pty hovering in the same band must not pause.
    controller.update('pty-2', LOW + 1)
    controller.update('pty-2', HIGH)
    expect(pauseProducer).toHaveBeenCalledTimes(1)
  })

  it('re-asserts the pause after the failsafe interval while still flooded', () => {
    controller.update('pty-1', HIGH + 1)
    expect(pauseProducer).toHaveBeenCalledTimes(1)

    // Within the failsafe window: no re-assert even far above HIGH.
    vi.advanceTimersByTime(PRODUCER_PAUSE_REASSERT_INTERVAL_MS - 1)
    controller.update('pty-1', HIGH * 4)
    expect(pauseProducer).toHaveBeenCalledTimes(1)

    // After the window (daemon failsafe has auto-resumed by now): re-pause.
    vi.advanceTimersByTime(1)
    controller.update('pty-1', HIGH * 4)
    expect(pauseProducer).toHaveBeenCalledTimes(2)

    // The re-assert re-stamps the clock — no immediate third pause.
    controller.update('pty-1', HIGH * 4)
    expect(pauseProducer).toHaveBeenCalledTimes(2)
  })

  it('release resumes only ptys that are actually paused', () => {
    controller.update('paused-pty', HIGH + 1)
    controller.release('paused-pty')
    controller.release('never-paused-pty')
    expect(resumeProducer).toHaveBeenCalledTimes(1)
    expect(resumeProducer).toHaveBeenCalledWith('paused-pty')
    expect(controller.isPaused('paused-pty')).toBe(false)
  })

  it('releaseAll resumes every paused pty', () => {
    controller.update('pty-1', HIGH + 1)
    controller.update('pty-2', HIGH + 1)
    controller.update('pty-3', LOW)
    controller.releaseAll()
    expect(resumeProducer).toHaveBeenCalledTimes(2)
    expect(resumeProducer).toHaveBeenCalledWith('pty-1')
    expect(resumeProducer).toHaveBeenCalledWith('pty-2')
    expect(controller.isPaused('pty-1')).toBe(false)
    expect(controller.isPaused('pty-2')).toBe(false)
  })

  it('keeps bookkeeping consistent when the transport throws', () => {
    pauseProducer.mockImplementation(() => {
      throw new Error('provider gone')
    })
    resumeProducer.mockImplementation(() => {
      throw new Error('provider gone')
    })
    expect(() => controller.update('pty-1', HIGH + 1)).not.toThrow()
    expect(controller.isPaused('pty-1')).toBe(true)
    expect(() => controller.update('pty-1', 0)).not.toThrow()
    expect(controller.isPaused('pty-1')).toBe(false)
  })

  it('tracks watermark state per pty independently', () => {
    controller.update('pty-1', HIGH + 1)
    controller.update('pty-2', HIGH + 1)
    controller.update('pty-1', 0)
    expect(pauseProducer).toHaveBeenCalledTimes(2)
    expect(resumeProducer).toHaveBeenCalledTimes(1)
    expect(controller.isPaused('pty-1')).toBe(false)
    expect(controller.isPaused('pty-2')).toBe(true)
  })
})

// The cross-language parity certificate (P3 stage 2): this TS production
// controller and the Rust `orca-flow-control` spec run the SAME shared corpus and
// must emit identical pause/resume actions. Production stays in TS because
// `update` is per-chunk hot-path (a napi hop would regress it like the rejected
// pty:data cutover); the Rust core is the machine-checkable, ay-provable spec
// proven equivalent to it here.
describe('PtyProducerFlowController shared parity corpus', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('matches the Rust orca-flow-control corpus step for step', () => {
    const corpusPath = fileURLToPath(
      new URL('../../../rust/crates/orca-flow-control/parity-corpus.txt', import.meta.url)
    )
    const corpus = readFileSync(corpusPath, 'utf8')
    const actions: string[] = []
    // Compact watermarks matching the corpus header (high=100 low=10 reassert=5000).
    const fc = new PtyProducerFlowController(
      {
        pauseProducer: (id) => actions.push(`pause:${id}`),
        resumeProducer: (id) => actions.push(`resume:${id}`)
      },
      { highWatermarkChars: 100, lowWatermarkChars: 10, reassertIntervalMs: 5000 }
    )
    let lineNo = 0
    for (const raw of corpus.split('\n')) {
      lineNo++
      const line = raw.trim()
      if (line === '' || line.startsWith('#')) {
        continue
      }
      const [lhs, rhs = ''] = line.split('=>')
      const expected = rhs.trim().split(/\s+/).filter(Boolean).sort()
      const toks = lhs.trim().split(/\s+/)
      const before = actions.length
      if (toks[0] === 'update') {
        // Drive Date.now() (the reassert clock) to the corpus timestamp.
        vi.setSystemTime(Number(toks[3]))
        fc.update(toks[1], Number(toks[2]))
      } else if (toks[0] === 'release') {
        fc.release(toks[1])
      } else if (toks[0] === 'releaseAll') {
        fc.releaseAll()
      } else {
        throw new Error(`line ${lineNo}: unknown op ${toks[0]}`)
      }
      const got = actions.slice(before).sort()
      expect(got, `parity mismatch at line ${lineNo}: ${line}`).toEqual(expected)
    }
    // The corpus must actually exercise the machine, not silently no-op.
    expect(actions.length).toBeGreaterThanOrEqual(8)
  })
})
