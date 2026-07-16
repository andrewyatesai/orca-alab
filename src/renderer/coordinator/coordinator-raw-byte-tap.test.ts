import { describe, expect, it } from 'vitest'
import { createSessionByteTaps, seedForEngineReplay } from './coordinator-raw-byte-tap'

describe('createSessionByteTaps', () => {
  it('delivers chunks only to sinks tapping that session', () => {
    const taps = createSessionByteTaps()
    const a: string[] = []
    const b: string[] = []
    taps.add('sess-a', (chunk) => a.push(chunk))
    taps.add('sess-b', (chunk) => b.push(chunk))

    taps.deliver('sess-a', 'one')
    taps.deliver('sess-b', 'two')
    taps.deliver('sess-c', 'lost')

    expect(a).toEqual(['one'])
    expect(b).toEqual(['two'])
  })

  it('fans one session out to every tapped sink in registration order', () => {
    const taps = createSessionByteTaps()
    const seen: string[] = []
    taps.add('sess', (chunk) => seen.push(`first:${chunk}`))
    taps.add('sess', (chunk) => seen.push(`second:${chunk}`))

    taps.deliver('sess', 'x')

    expect(seen).toEqual(['first:x', 'second:x'])
  })

  it('stops delivering after untap, without disturbing other sinks', () => {
    const taps = createSessionByteTaps()
    const kept: string[] = []
    const dropped: string[] = []
    const untap = taps.add('sess', (chunk) => dropped.push(chunk))
    taps.add('sess', (chunk) => kept.push(chunk))

    taps.deliver('sess', 'before')
    untap()
    taps.deliver('sess', 'after')

    expect(dropped).toEqual(['before'])
    expect(kept).toEqual(['before', 'after'])
  })

  it('untap is idempotent and safe after every sink is gone', () => {
    const taps = createSessionByteTaps()
    const untap = taps.add('sess', () => undefined)
    untap()
    untap()
    // No taps left: delivery is a no-op, not a crash.
    taps.deliver('sess', 'ignored')
  })

  it('skips empty chunks (nothing to feed an engine)', () => {
    const taps = createSessionByteTaps()
    const seen: string[] = []
    taps.add('sess', (chunk) => seen.push(chunk))

    taps.deliver('sess', '')

    expect(seen).toEqual([])
  })
})

describe('seedForEngineReplay', () => {
  it('returns an under-bound tail unchanged (it was never sliced)', () => {
    expect(seedForEngineReplay('[31mhi[0m\nline', 100)).toBe('[31mhi[0m\nline')
  })

  it('resyncs a bound-hit tail at the first line boundary', () => {
    // A slice mid-SGR leaves "8;2;10m…" garbage at the front; drop through \n.
    const tail = '8;2;10mgarbage\nclean line\nprompt $ '
    expect(seedForEngineReplay(tail, tail.length)).toBe('clean line\nprompt $ ')
  })

  it('keeps a bound-hit tail whole when it has no line boundary to resync at', () => {
    const tail = 'x'.repeat(64)
    expect(seedForEngineReplay(tail, 64)).toBe(tail)
  })

  it('treats exactly-at-bound length as sliced (appendBoundedTail slices to the bound)', () => {
    const tail = `abc\n${'y'.repeat(60)}`
    expect(seedForEngineReplay(tail, tail.length)).toBe('y'.repeat(60))
  })
})
