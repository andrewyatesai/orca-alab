// Residual 4 / (d3): a REAL rewrap-driven proof of the remote-search row remap,
// NOT the plain-offset tautology it replaces. The earlier §6 matrix fed the
// remap the very numbers it recomputes (clientRow = replayOrigin + offset) and
// asserted the arithmetic against itself — an off-by-one in the remap would have
// passed. Here the CLIENT rows come out of a live `AtermTerminal`: the snapshot
// is replayed into a real engine, the engine assigns the physical rows, and the
// remap must reproduce the engine's own row for each match. A real width change
// then drives an actual scrollback reflow through the engine (`resize` +
// `pump_reflow`), moving those physical rows, and we assert the honesty contract
// (widths-differ ⇒ approximate; the fixed-origin remap is no longer engine-exact).
//
// Mutation-proof: every asserted client row is the engine's real absRow, and the
// replay origin is read from the engine (a non-zero baseline established by a
// prefix), so a ±1 error in `replayOriginRow + (matchHostRow − hostRowAnchor)`
// mismatches a real row and fails.

import { readFileSync } from 'node:fs'
import { beforeAll, afterEach, describe, expect, it } from 'vitest'
import { initSync, AtermTerminal } from './aterm_wasm.js'
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'
import { createAtermSearchSummaryReader } from './aterm-worker-search-summary'
import { remapRemoteSearchRow } from '../../../../../shared/terminal-remote-search-protocol'

const ATERM_DIR = new URL('./', import.meta.url)
const FONT_URL = new URL('../../../assets/fonts/jetbrains-mono.ttf', import.meta.url)
let fontBytes: Uint8Array

beforeAll(() => {
  initSync({ module: readFileSync(new URL('aterm_wasm_bg.wasm', ATERM_DIR)) })
  fontBytes = new Uint8Array(readFileSync(FONT_URL))
})

const openTerms: AtermTerminal[] = []
afterEach(() => {
  for (const t of openTerms.splice(0)) {
    t.free()
  }
})

function open(rows: number, cols: number): AtermTerminal {
  const t = new AtermTerminal(
    rows,
    cols,
    fontBytes,
    ATERM_RENDERER_FONT_PX,
    0xffffff,
    0x000000,
    0xffffff,
    0x334455
  )
  openTerms.push(t)
  return t
}

function drainReflow(t: AtermTerminal): void {
  // A width change stashes a bounded scrollback rewrap; pump it to completion so
  // the physical rows we read reflect the fully-reflowed history (the engine's
  // `reflow_step_any_schedule_matches_one_shot` guarantee).
  for (let i = 0; i < 5000; i += 1) {
    if (!t.pump_reflow()) {
      return
    }
  }
}

/** The engine's real absolute row for the single line containing `token`. */
function engineRowOf(t: AtermTerminal, token: string): number {
  const summary = createAtermSearchSummaryReader(t).read(token, true, false, 4)
  expect(summary).not.toBeNull()
  const rows = summary!.matches.map((m) => m.absRow)
  expect(rows.length).toBe(1) // tokens are unique per line
  return rows[0]
}

// A snapshot line short enough to occupy ONE physical row at width 40 (so the
// replay is 1:1 and the remap is exact there), but long enough to wrap at 20.
const SNAP_WIDTH = 40
const NARROW_WIDTH = 20
const lineFor = (tag: string): string => `Zmark ${tag} body text pad tail qq\r\n`
const SNAPSHOT_TAGS = ['AA', 'BB', 'CC', 'DD', 'EE', 'FF', 'GG', 'HH']

describe('remote row remap — real engine reflow (residual 4 / d3)', () => {
  it('reproduces the engine-assigned client row for every replayed match (exact, non-zero origin)', () => {
    const client = open(6, SNAP_WIDTH)
    // A PREFIX that is NOT part of the replayed snapshot: it establishes a real,
    // engine-chosen non-zero replay origin, so the remap's subtraction/addition
    // is load-bearing (origin 0 would collapse the arithmetic to identity).
    for (let i = 0; i < 5; i += 1) {
      client.process_str(`prefix filler line ${i}\r\n`)
    }
    // The replayed snapshot region begins here — its first row is the anchor row.
    for (const tag of SNAPSHOT_TAGS) {
      client.process_str(lineFor(tag))
    }
    drainReflow(client)

    // The engine's own row for the snapshot's FIRST line == the client replay
    // origin (read from the engine, never assumed).
    const replayOriginRow = engineRowOf(client, 'Zmark AA')
    // The host serialized the snapshot from an arbitrary stable base; row i of
    // the snapshot is stable host row HOST_ANCHOR + i.
    const HOST_ANCHOR = 100_000
    const replayedRowCount = SNAPSHOT_TAGS.length

    SNAPSHOT_TAGS.forEach((tag, snapshotRowIndex) => {
      const engineRow = engineRowOf(client, `Zmark ${tag}`)
      const matchHostRow = HOST_ANCHOR + snapshotRowIndex
      const remap = remapRemoteSearchRow({
        matchHostRow,
        responseAnchor: {
          hostRowAnchor: HOST_ANCHOR,
          anchorGen: 9,
          anchorHostCols: SNAP_WIDTH
        },
        replayedAnchor: { hostRowAnchor: HOST_ANCHOR, anchorGen: 9 },
        replayOriginRow,
        replayedRowCount,
        clientCols: SNAP_WIDTH // client displays at the snapshot width → exact
      })
      // The remap must land on the engine's REAL row — a ±1 slip in the offset
      // formula would miss the engine-assigned absRow and fail here.
      expect(remap).toEqual({
        kind: 'in-window',
        clientRow: engineRow,
        approximate: false
      })
    })
  })

  it('a real width-change reflow moves the physical rows; the remap honestly flags approximate', () => {
    const client = open(6, SNAP_WIDTH)
    for (let i = 0; i < 5; i += 1) {
      client.process_str(`prefix filler line ${i}\r\n`)
    }
    for (const tag of SNAPSHOT_TAGS) {
      client.process_str(lineFor(tag))
    }
    drainReflow(client)

    const HOST_ANCHOR = 100_000
    const replayOriginRow = engineRowOf(client, 'Zmark AA')
    const probeTag = 'FF'
    const probeIndex = SNAPSHOT_TAGS.indexOf(probeTag)
    const rowBefore = engineRowOf(client, `Zmark ${probeTag}`)

    // Drive an ACTUAL reflow through the engine: narrow the grid so every
    // snapshot line rewraps into two physical rows.
    client.resize(6, NARROW_WIDTH)
    drainReflow(client)
    const rowAfter = engineRowOf(client, `Zmark ${probeTag}`)

    // The reflow genuinely moved this match's physical row (proves a real rewrap
    // happened, not a no-op): rewrapping the whole history reassigns absolute
    // coordinates, so the pre- and post-reflow rows differ.
    expect(rowAfter).not.toBe(rowBefore)

    // With the client now displaying at a width the snapshot was NOT serialized
    // at, the fixed-origin remap can no longer be engine-exact — the contract is
    // to land on the nearest whole row and FLAG it approximate (never claim an
    // exact jump it cannot verify).
    const remap = remapRemoteSearchRow({
      matchHostRow: HOST_ANCHOR + probeIndex,
      responseAnchor: {
        hostRowAnchor: HOST_ANCHOR,
        anchorGen: 9,
        anchorHostCols: SNAP_WIDTH
      },
      replayedAnchor: { hostRowAnchor: HOST_ANCHOR, anchorGen: 9 },
      replayOriginRow,
      replayedRowCount: SNAPSHOT_TAGS.length,
      clientCols: NARROW_WIDTH
    })
    expect(remap.kind).toBe('in-window')
    expect(remap.kind === 'in-window' && remap.approximate).toBe(true)
    // And the honesty is warranted: the pre-reflow offset row is demonstrably
    // NOT where the engine now holds the content, so an "exact" claim would be a
    // wrong-row jump.
    expect(remap.kind === 'in-window' && remap.clientRow).not.toBe(rowAfter)
  })
})
