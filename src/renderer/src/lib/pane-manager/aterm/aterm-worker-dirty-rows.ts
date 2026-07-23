import type { WorkerEngine } from './aterm-worker-engine-build'
import type { AtermWorkerGridRow } from './aterm-render-worker-protocol'
import {
  createAtermRowRangeReader,
  type AtermRowRangeExportEngine,
  type AtermRowRangeRecord
} from './aterm-worker-row-range-export'

// Per-visible-row change detection for the worker's buildState: emit only the rows whose
// text / wrap / len changed since the last frame, so a streaming pane clones just the 1-2
// rows that moved. Tracks three PARALLEL arrays (text by reference + flags) rather than a
// concatenated per-row signature string, so an unchanged row costs three compares — not a
// fresh full-row string allocation every frame. Extracted to keep the worker terminal
// under the line cap.
//
// P7 fling decoupling: while display_offset churns (scroll fling / streaming-while-
// scrolled), EVERY visible row changes every frame, making the per-row wasm export
// (row_text/row_len/row_is_wrapped per row) the dominant buildState cost. Sustained
// churn is rate-limited to one full export per ATERM_GRID_MIRROR_CHURN_SYNC_INTERVAL_MS;
// stale() tells the frame scheduler a render-free settle sync is owed. ONLY this mirror
// is throttled — offset/cursor/selection/search scalars stay live in every STATE.
//
// E9 batch export (feature-detected): when the pinned artifact exposes row_range_json,
// each un-throttled build costs ONE wasm-boundary crossing instead of 3 calls/row +
// cols calls per non-ASCII changed row; artifacts without it keep the per-row path.

// The optional E9 batch export intersects in so capable engines type-check without casts.
type DirtyRowEngine = Pick<
  WorkerEngine,
  'row_text' | 'row_is_wrapped' | 'row_len' | 'cell_is_wide'
> &
  AtermRowRangeExportEngine

/** Cap the full row export to one sync per this window while the offset churns (~20Hz). */
export const ATERM_GRID_MIRROR_CHURN_SYNC_INTERVAL_MS = 50
/** Settle-sync delay: must outlast one frame gap at 30Hz (~33ms) so it only fires once
 *  the fling truly stopped, while keeping worst-case mirror staleness bounded. */
export const ATERM_GRID_MIRROR_SETTLE_DELAY_MS = 48

export function createAtermDirtyRowTracker(
  e: DirtyRowEngine,
  nowMs: () => number = () => performance.now()
): {
  build: (rows: number, cols: number, displayOffset: number) => AtermWorkerGridRow[]
  /** True when the last build withheld the row export (throttled mid-churn) — the
   *  frame scheduler owes the mirror a settle sync. */
  stale: () => boolean
} {
  let lastText: string[] = []
  let lastWrapped: boolean[] = []
  let lastLen: number[] = []
  let lastOffset: number | null = null
  let lastFullScanMs = -Infinity
  let churnFrames = 0
  let stale = false
  // Cached all-'1' width string for all-narrow rows, rebuilt only on a cols change,
  // so the fast path allocates nothing per frame. Per-tracker (per pane) to
  // avoid thrash when the worker hosts panes of different widths.
  let asciiWidthsCols = -1
  let asciiWidths = ''
  const narrowWidths = (cols: number): string => {
    if (asciiWidthsCols !== cols) {
      asciiWidthsCols = cols
      asciiWidths = '1'.repeat(cols)
    }
    return asciiWidths
  }
  // E9 batch row-range reader: null reads fall back to the per-row exports below.
  const rowRange = createAtermRowRangeReader(e)

  return {
    stale: () => stale,
    build: (rows, cols, displayOffset) => {
      const offsetChurned = lastOffset !== null && displayOffset !== lastOffset
      lastOffset = displayOffset
      churnFrames = offsetChurned ? churnFrames + 1 : 0
      // Throttle only SUSTAINED churn (2+ consecutive offset-change frames) so a single
      // wheel notch mirrors instantly; a rows change is a resize, never throttled.
      if (
        churnFrames >= 2 &&
        rows === lastText.length &&
        nowMs() - lastFullScanMs < ATERM_GRID_MIRROR_CHURN_SYNC_INTERVAL_MS
      ) {
        stale = true
        return []
      }
      stale = false
      lastFullScanMs = nowMs()
      const dirty: AtermWorkerGridRow[] = []
      if (lastText.length !== rows) {
        lastText = Array.from({ length: rows }, () => '')
        lastWrapped = Array.from({ length: rows }, () => false)
        // -1 = no real row len, so every row reads as changed on the first frame.
        lastLen = Array.from({ length: rows }, () => -1)
      }
      // ONE wasm-boundary crossing for the whole visible grid when the pinned
      // artifact has the E9 export; null (absent/unavailable/skew) → per-row path.
      const batch: AtermRowRangeRecord[] | null = rowRange.read(0, rows, cols)
      for (let y = 0; y < rows; y++) {
        const rec = batch?.[y]
        const text = rec ? rec.text : (e.row_text(y) ?? '')
        const wrapped = rec ? rec.wrapped : e.row_is_wrapped(y) === true
        const len = rec ? rec.len : (e.row_len(y) ?? cols)
        if (text === lastText[y] && wrapped === lastWrapped[y] && len === lastLen[y]) {
          continue
        }
        lastText[y] = text
        lastWrapped[y] = wrapped
        lastLen[y] = len
        // Per-column width digit ('2' wide lead, '1' normal); only for changed rows.
        // Batch path: widths come from the export (omitted = all-narrow → cached
        // all-'1' string), so the per-cell walk never runs.
        // Per-row ASCII fast-path: an all-ASCII row can hold no wide cells, so skip
        // the per-cell cell_is_wide walk (cols wasm-boundary calls/row — the dominant
        // per-frame cost while scrolling varied content) and reuse the cached
        // all-'1' string. Mirrors aterm-facade-buffer's proven fast path; output
        // is byte-identical (all width-1 ⇒ 1:1 column mapping).
        let widths: string
        if (rec) {
          widths = rec.widths ?? narrowWidths(cols)
        } else {
          let allAscii = true
          for (let i = 0; i < text.length; i++) {
            if (text.charCodeAt(i) > 0x7f) {
              allAscii = false
              break
            }
          }
          if (allAscii) {
            widths = narrowWidths(cols)
          } else {
            let w = ''
            for (let x = 0; x < cols; x++) {
              w += e.cell_is_wide(y, x) === true ? '2' : '1'
            }
            widths = w
          }
        }
        dirty.push({ y, text, wrapped, len, widths })
      }
      return dirty
    }
  }
}
