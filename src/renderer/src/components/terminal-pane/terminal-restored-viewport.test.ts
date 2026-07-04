import { describe, expect, it } from 'vitest'

import { buildFreshShellViewportBlankingSequence } from './terminal-restored-viewport'

// The upstream suite (#7310) wrote this sequence into a headless xterm primed
// with a stale TUI scroll region (`CSI 2;4r` + DECOM) and inspected the buffer
// to prove the restored rows moved into scrollback while the viewport went
// blank. Under aterm there is no in-process engine in vitest (see
// terminal-replay-cursor-state.test.ts), so the renderer-side guard is the
// literal byte contract of the sequence: margins/DECOM reset first, then
// newline scrolling — which every VT engine keeps in scrollback — and never a
// CSI S or ED erase, which would drop the restored rows instead.
describe('buildFreshShellViewportBlankingSequence', () => {
  it('scrolls the whole viewport into scrollback with newlines, then homes the cursor', () => {
    // DECOM off + DECSTBM full reset, CUP to the bottom row, one CRLF per
    // visible row (so every restored row leaves the viewport into history),
    // cursor home for the fresh prompt.
    expect(buildFreshShellViewportBlankingSequence(5)).toBe(
      `\x1b[?6l\x1b[r\x1b[5;1H${'\r\n'.repeat(5)}\x1b[H`
    )
  })

  it('resets DECOM and the margins before positioning, so a stale TUI scroll region cannot trap the scroll', () => {
    // Upstream scenario: a crashed TUI left `CSI 2;4r` + DECOM behind. Without
    // the `?6l` + `r` prefix, the CUP would be origin-relative and the newlines
    // would recycle inside the region, overwriting restored rows in place.
    const sequence = buildFreshShellViewportBlankingSequence(24)
    expect(sequence.startsWith('\x1b[?6l\x1b[r')).toBe(true)
    expect(sequence.indexOf('\x1b[r')).toBeLessThan(sequence.indexOf('\x1b[24;1H'))
  })

  it('never erases or pans the restored rows away', () => {
    for (const rows of [1, 5, 24, 50]) {
      const sequence = buildFreshShellViewportBlankingSequence(rows)
      // CSI S (pan up) discards scrolled-out lines; ED (`J`) erases in place.
      // Either would lose the restored rows upstream proved survive in
      // scrollback (`row1`..`row5` after blanking).
      expect(sequence).not.toContain('S')
      expect(sequence).not.toContain('J')
      // Exactly one CRLF per visible row: enough to blank the full viewport,
      // no more (extra newlines would push blank filler into scrollback).
      expect(sequence.split('\r\n').length - 1).toBe(rows)
    }
  })

  it('clamps non-finite, fractional, and sub-1 row counts to a usable viewport height', () => {
    expect(buildFreshShellViewportBlankingSequence(Number.NaN)).toBe(
      buildFreshShellViewportBlankingSequence(24)
    )
    expect(buildFreshShellViewportBlankingSequence(Number.POSITIVE_INFINITY)).toBe(
      buildFreshShellViewportBlankingSequence(24)
    )
    expect(buildFreshShellViewportBlankingSequence(5.7)).toBe(
      buildFreshShellViewportBlankingSequence(5)
    )
    expect(buildFreshShellViewportBlankingSequence(0)).toBe(
      buildFreshShellViewportBlankingSequence(1)
    )
  })
})
