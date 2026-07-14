import type { WorkerEngine } from './aterm-worker-engine-build'
import type { AtermWorkerGridRow } from './aterm-render-worker-protocol'

// Per-visible-row change detection for the worker's buildState: emit only the rows whose
// text / wrap / len changed since the last frame, so a streaming pane clones just the 1-2
// rows that moved. Tracks three PARALLEL arrays (text by reference + flags) rather than a
// concatenated per-row signature string, so an unchanged row costs three compares — not a
// fresh full-row string allocation every frame. Extracted to keep the worker terminal
// under the line cap.

type DirtyRowEngine = Pick<WorkerEngine, 'row_text' | 'row_is_wrapped' | 'row_len' | 'cell_is_wide'>

export function createAtermDirtyRowTracker(e: DirtyRowEngine): {
  build: (rows: number, cols: number) => AtermWorkerGridRow[]
} {
  let lastText: string[] = []
  let lastWrapped: boolean[] = []
  let lastLen: number[] = []
  // Cached all-'1' width string for ASCII rows, rebuilt only on a cols change,
  // so the fast path allocates nothing per frame. Per-tracker (per pane) to
  // avoid thrash when the worker hosts panes of different widths.
  let asciiWidthsCols = -1
  let asciiWidths = ''

  return {
    build: (rows, cols) => {
      const dirty: AtermWorkerGridRow[] = []
      if (lastText.length !== rows) {
        lastText = Array.from({ length: rows }, () => '')
        lastWrapped = Array.from({ length: rows }, () => false)
        // -1 = no real row len, so every row reads as changed on the first frame.
        lastLen = Array.from({ length: rows }, () => -1)
      }
      for (let y = 0; y < rows; y++) {
        const text = e.row_text(y) ?? ''
        const wrapped = e.row_is_wrapped(y) === true
        const len = e.row_len(y) ?? cols
        if (text === lastText[y] && wrapped === lastWrapped[y] && len === lastLen[y]) {
          continue
        }
        lastText[y] = text
        lastWrapped[y] = wrapped
        lastLen[y] = len
        // Per-column width digit ('2' wide lead, '1' normal); only for changed rows.
        // ASCII fast-path: an all-ASCII row can hold no wide cells, so skip the
        // per-cell cell_is_wide walk (cols wasm-boundary calls/row — the dominant
        // per-frame cost while scrolling varied content) and reuse the cached
        // all-'1' string. Mirrors aterm-facade-buffer's proven fast path; output
        // is byte-identical (all width-1 ⇒ 1:1 column mapping).
        let widths: string
        let allAscii = true
        for (let i = 0; i < text.length; i++) {
          if (text.charCodeAt(i) > 0x7f) {
            allAscii = false
            break
          }
        }
        if (allAscii) {
          if (asciiWidthsCols !== cols) {
            asciiWidthsCols = cols
            asciiWidths = '1'.repeat(cols)
          }
          widths = asciiWidths
        } else {
          let w = ''
          for (let x = 0; x < cols; x++) {
            w += e.cell_is_wide(y, x) === true ? '2' : '1'
          }
          widths = w
        }
        dirty.push({ y, text, wrapped, len, widths })
      }
      return dirty
    }
  }
}
