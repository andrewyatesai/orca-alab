// Fed §6 hostRowAnchor remap matrix: in-window matches jump to the RIGHT
// client row, wrap-width differences flag approximate, and an anchorGen
// mismatch degrades to inline expansion instead of a wrong-row jump.
import { describe, expect, it } from 'vitest'
import { remapRemoteSearchRow } from './terminal-remote-search-protocol'

const anchor = (hostRowAnchor: number, anchorGen: number, anchorHostCols?: number) => ({
  hostRowAnchor,
  anchorGen,
  ...(anchorHostCols !== undefined ? { anchorHostCols } : {})
})

describe('remapRemoteSearchRow', () => {
  it('maps an in-window match to clientRow = replayOrigin + (hostRow − anchor)', () => {
    // Host serialized rows 100..199 (anchor 100); the client replayed them
    // starting at client row 0. A match at host row 137 is client row 37.
    const remap = remapRemoteSearchRow({
      matchHostRow: 137,
      responseAnchor: anchor(100, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100,
      clientCols: 80
    })
    expect(remap).toEqual({ kind: 'in-window', clientRow: 37, approximate: false })
  })

  it('offsets by a non-zero client replay origin', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 110,
      responseAnchor: anchor(100, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 25,
      replayedRowCount: 100,
      clientCols: 80
    })
    expect(remap).toEqual({ kind: 'in-window', clientRow: 35, approximate: false })
  })

  it('flags the jump approximate when host and client wrap widths differ', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 110,
      responseAnchor: anchor(100, 5, 120),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100,
      clientCols: 80
    })
    expect(remap).toEqual({ kind: 'in-window', clientRow: 10, approximate: true })
  })

  it('degrades to anchor-mismatch when the generations differ (client replayed a different snapshot)', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 110,
      responseAnchor: anchor(100, 6, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100
    })
    expect(remap).toEqual({ kind: 'anchor-mismatch' })
  })

  it('degrades to anchor-mismatch when either side has no anchor', () => {
    expect(
      remapRemoteSearchRow({
        matchHostRow: 110,
        responseAnchor: null,
        replayedAnchor: anchor(100, 5),
        replayOriginRow: 0,
        replayedRowCount: 100
      })
    ).toEqual({ kind: 'anchor-mismatch' })
    expect(
      remapRemoteSearchRow({
        matchHostRow: 110,
        responseAnchor: anchor(100, 5),
        replayedAnchor: null,
        replayOriginRow: 0,
        replayedRowCount: 100
      })
    ).toEqual({ kind: 'anchor-mismatch' })
  })

  it('degrades to anchor-mismatch when same-gen anchors disagree on the row (defensive)', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 110,
      responseAnchor: anchor(101, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100
    })
    expect(remap).toEqual({ kind: 'anchor-mismatch' })
  })

  it('classifies rows older than the replayed window out-of-window (deep history → inline expansion)', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 99,
      responseAnchor: anchor(100, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100
    })
    expect(remap).toEqual({ kind: 'out-of-window' })
  })

  it('classifies rows newer than the replayed window out-of-window (post-snapshot output must not clamp to a wrong row)', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 205,
      responseAnchor: anchor(100, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100
    })
    expect(remap).toEqual({ kind: 'out-of-window' })
  })

  it('maps the exact window edges correctly', () => {
    const first = remapRemoteSearchRow({
      matchHostRow: 100,
      responseAnchor: anchor(100, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 3,
      replayedRowCount: 100,
      clientCols: 80
    })
    expect(first).toEqual({ kind: 'in-window', clientRow: 3, approximate: false })
    const last = remapRemoteSearchRow({
      matchHostRow: 199,
      responseAnchor: anchor(100, 5, 80),
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 3,
      replayedRowCount: 100,
      clientCols: 80
    })
    expect(last).toEqual({ kind: 'in-window', clientRow: 102, approximate: false })
  })

  // (d) nearest-row-boundary remap for width-mismatched anchors.
  it('(d) lands on the WHOLE nearest client row when widths differ, flagged approximate', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 153,
      responseAnchor: anchor(100, 5, 132), // host serialized at 132 cols
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100,
      clientCols: 80 // client rewraps at 80 → the exact char position is unknowable
    })
    // Integer client row (nearest boundary): 153 − 100 + 0 = 53; approximate.
    expect(remap).toEqual({ kind: 'in-window', clientRow: 53, approximate: true })
  })

  it('(d) flags approximate when the host omitted its width (cannot confirm equal widths)', () => {
    const remap = remapRemoteSearchRow({
      matchHostRow: 110,
      responseAnchor: anchor(100, 5), // old host: no anchorHostCols
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 0,
      replayedRowCount: 100,
      clientCols: 80
    })
    // Cannot verify the host matched the client width — must not claim exact.
    expect(remap).toEqual({ kind: 'in-window', clientRow: 10, approximate: true })
  })

  // (d) stable-rows-across-host-resize: host rows are retained-origin-based, so a
  // host resize does NOT shift a match's stable host row within the emulator
  // incarnation. The remap depends only on the fixed SNAPSHOT anchor
  // (hostRowAnchor + anchorHostCols recorded at serialize time), never on the
  // host's live width — so a resize cannot move where a stable row lands.
  it('(d) the remap uses the snapshot anchor width, not the host live width (resize-stable)', () => {
    // Snapshot was serialized at 80 cols; the client replayed it at 80 cols.
    // The host has SINCE resized its live grid to 132, but the match still comes
    // back at the same stable host row 140 echoing the same snapshot anchor.
    const remap = remapRemoteSearchRow({
      matchHostRow: 140,
      responseAnchor: anchor(100, 5, 80), // anchorHostCols = SNAPSHOT width (80)
      replayedAnchor: anchor(100, 5),
      replayOriginRow: 10,
      replayedRowCount: 100,
      clientCols: 80
    })
    // Stable → exact jump at clientRow 50; the live resize to 132 is irrelevant.
    expect(remap).toEqual({ kind: 'in-window', clientRow: 50, approximate: false })
  })

  // (d4) MUTATION-PROOF anchorHostCols honoring: the width fields must be
  // load-bearing, not decorative. Hold EVERY other input fixed and vary ONLY the
  // snapshot anchor width — the `approximate` verdict must flip. A remap that
  // ignored anchorHostCols (the gate-failed shape) would return the same verdict
  // for both and fail this test. The clientRow is intentionally identical across
  // the pair, isolating the width field as the sole cause of the flip.
  it('(d4) approximate flips solely on anchorHostCols — matching width is exact, mismatched is approximate', () => {
    const fixed = {
      matchHostRow: 148,
      replayedAnchor: anchor(100, 7),
      replayOriginRow: 0,
      replayedRowCount: 100,
      clientCols: 80
    } as const
    const exact = remapRemoteSearchRow({ ...fixed, responseAnchor: anchor(100, 7, 80) })
    const approx = remapRemoteSearchRow({ ...fixed, responseAnchor: anchor(100, 7, 81) })
    // Same landing row in both — ONLY the width field moved.
    expect(exact).toEqual({ kind: 'in-window', clientRow: 48, approximate: false })
    expect(approx).toEqual({ kind: 'in-window', clientRow: 48, approximate: true })
    // The guarantee stated as a mutation: injecting a width the client cannot
    // confirm equal MUST downgrade the jump to approximate.
    expect(approx).not.toEqual(exact)
  })
})
