import { createElement, useEffect } from 'react'
import { act, create, type ReactTestRenderer } from 'react-test-renderer'
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest'
import { useThrottledLatestValue } from './use-throttled-latest-value'

describe('useThrottledLatestValue', () => {
  let renderer: ReactTestRenderer | null = null
  let latest: string | undefined
  let consoleSpy: MockInstance

  function Harness({ value, resetKey }: { value: string | undefined; resetKey?: unknown }): null {
    latest = useThrottledLatestValue(value, 50, resetKey)
    return null
  }

  function render(value: string | undefined, resetKey?: unknown): void {
    act(() => {
      renderer = create(createElement(Harness, { value, resetKey }))
    })
  }

  function update(value: string | undefined, resetKey?: unknown): void {
    act(() => renderer?.update(createElement(Harness, { value, resetKey })))
  }

  beforeEach(() => {
    vi.useFakeTimers()
    globalThis.IS_REACT_ACT_ENVIRONMENT = true
    latest = undefined
    const original = console.error
    consoleSpy = vi.spyOn(console, 'error').mockImplementation((...args) => {
      if (typeof args[0] === 'string' && args[0].includes('react-test-renderer is deprecated')) {
        return
      }
      original(...args)
    })
  })

  afterEach(() => {
    act(() => renderer?.unmount())
    renderer = null
    vi.useRealTimers()
    consoleSpy.mockRestore()
  })

  it('emits the first frame immediately', () => {
    render('a')
    expect(latest).toBe('a')
  })

  it('holds rapid updates but eventually emits the latest value', () => {
    render('a')
    update('ab')
    update('abc')
    expect(latest).toBe('a')
    act(() => vi.advanceTimersByTime(50))
    expect(latest).toBe('abc')
  })

  it('emits the new source immediately when resetKey changes mid-throttle', () => {
    // Session A is streaming: its held value is throttled and a trailing emit is
    // pending (not yet elapsed).
    render('a-old', 'session-a')
    update('a-newer', 'session-a')
    expect(latest).toBe('a-old')

    // Switch to session B (new resetKey) before A's interval elapses. B's value
    // must show at once, never A's stale trailing frame.
    update('b-current', 'session-b')
    expect(latest).toBe('b-current')

    // A's pending trailing emit must have been cancelled, not fire late over B.
    act(() => vi.advanceTimersByTime(50))
    expect(latest).toBe('b-current')
  })

  it('does not carry a stale value when the new source is undefined (idle) on switch', () => {
    render('a-old', 'session-a')
    update('a-newer', 'session-a')
    expect(latest).toBe('a-old')

    update(undefined, 'session-b')
    expect(latest).toBeUndefined()
    act(() => vi.advanceTimersByTime(50))
    expect(latest).toBeUndefined()
  })

  it('still throttles within a stable resetKey', () => {
    render('a', 'session')
    update('ab', 'session')
    update('abc', 'session')
    expect(latest).toBe('a')
    act(() => vi.advanceTimersByTime(50))
    expect(latest).toBe('abc')
  })

  it('corrects the returned value during render on a source switch (reset before paint)', () => {
    // Records the interleaving of render-phase values and passive-effect flushes.
    // A reset done only in the effect produces the new value AFTER the first
    // effect (a stale paint); a reset before paint produces it during render,
    // before any effect runs.
    const log: string[] = []
    function ProbeHarness({ value, resetKey }: { value: string; resetKey: unknown }): null {
      const v = useThrottledLatestValue(value, 50, resetKey)
      log.push(`R:${v}`)
      useEffect(() => {
        log.push('EFFECT')
      })
      return null
    }
    act(() => {
      renderer = create(createElement(ProbeHarness, { value: 'a-old', resetKey: 'session-a' }))
    })
    // A held/throttled frame is pending for session-a.
    act(() =>
      renderer?.update(createElement(ProbeHarness, { value: 'a-newer', resetKey: 'session-a' }))
    )

    log.length = 0
    // Switch to session-b: its value must be produced during render, before the
    // effect flush, so the newly-keyed view never paints session-a's frame.
    act(() =>
      renderer?.update(createElement(ProbeHarness, { value: 'b-current', resetKey: 'session-b' }))
    )

    const bIdx = log.indexOf('R:b-current')
    const effectIdx = log.indexOf('EFFECT')
    expect(bIdx).toBeGreaterThanOrEqual(0)
    expect(effectIdx).toBeGreaterThanOrEqual(0)
    expect(bIdx).toBeLessThan(effectIdx)
  })

  it('clears immediately and drops the trailing emit when the value goes undefined', () => {
    render('a')
    update('ab')
    expect(latest).toBe('a')
    update(undefined)
    expect(latest).toBeUndefined()
    act(() => vi.advanceTimersByTime(50))
    expect(latest).toBeUndefined()
  })
})
