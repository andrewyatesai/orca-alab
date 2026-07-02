// Rolling main-thread mirror of the worker engine's VISIBLE grid: each snapshot's
// dirty rows land here so the worker-backed term's row/cell reads stay synchronous
// (no round-trip). Cells are split lazily per row (buildAtermRowCells is O(row)).

import { buildAtermRowCells } from './aterm-worker-grid-cells'
import type { AtermWorkerGridRow } from './aterm-render-worker-protocol'

type MirroredGridRow = {
  text: string
  wrapped: boolean
  len: number
  widths: string
  cells?: string[]
}

export type AtermWorkerGridMirror = {
  applyDirtyRows: (dirtyRows: AtermWorkerGridRow[], rows: number) => void
  row: (y: number) => MirroredGridRow | undefined
  rowCells: (y: number, cols: number) => string[]
  clear: () => void
}

export function createAtermWorkerGridMirror(): AtermWorkerGridMirror {
  const grid = new Map<number, MirroredGridRow>()
  return {
    applyDirtyRows: (dirtyRows, rows) => {
      for (const row of dirtyRows) {
        grid.set(row.y, {
          text: row.text,
          wrapped: row.wrapped,
          len: row.len,
          widths: row.widths
        })
      }
      // Drop rows that scrolled out of the (possibly shrunk) viewport.
      if (grid.size > rows) {
        for (const y of grid.keys()) {
          if (y >= rows) {
            grid.delete(y)
          }
        }
      }
    },
    row: (y) => grid.get(y),
    rowCells: (y, cols) => {
      const row = grid.get(y)
      if (!row) {
        return []
      }
      if (!row.cells) {
        row.cells = buildAtermRowCells(row.text, row.widths, cols)
      }
      return row.cells
    },
    clear: () => grid.clear()
  }
}
