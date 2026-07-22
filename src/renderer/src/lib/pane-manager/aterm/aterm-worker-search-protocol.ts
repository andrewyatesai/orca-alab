// Search slice of the render-worker wire contract (types only), split out like the
// rain/spill/predict sub-protocols so the main contract file stays under its line
// cap. The worker runs find/next/prev/clear and reports count/activeIndex/rect/
// generation/markers in the STATE snapshot.

export type AtermWorkerSearchFind = {
  type: 'searchFind'
  query: string
  caseSensitive: boolean
  isRegex: boolean
  /** Main-thread request id, echoed back as STATE.searchGeneration so the label
   *  can flag still-pending results ("~N, searching…") instead of claiming the
   *  stale count is current. */
  generation: number
}
export type AtermWorkerSearchNext = { type: 'searchNext' }
export type AtermWorkerSearchPrev = { type: 'searchPrev' }
export type AtermWorkerSearchClear = { type: 'searchClear' }

/** Every search command (paneId-free; the manager's per-pane post stamps it). */
export type AtermWorkerSearchCommand =
  | AtermWorkerSearchFind
  | AtermWorkerSearchNext
  | AtermWorkerSearchPrev
  | AtermWorkerSearchClear
