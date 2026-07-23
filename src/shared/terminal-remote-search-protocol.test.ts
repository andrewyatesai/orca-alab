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
})
