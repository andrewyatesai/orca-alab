// The budgeted-search step boundary between the wasm bindings and the worker: both
// engine modules (aterm_wasm / aterm_gpu_web) return a wasm-owned step object per
// slice; the copy here frees it immediately so slices never pin linear memory.
// Split from the engine build to keep that file under the line cap.

/** One budgeted search slice, copied to a plain JS object (the wasm step object
 *  is freed before returning so per-slice results never pin linear memory). */
export type EngineBudgetedSearchStep = {
  /** Match DELTA for this slice as flat [absLine, startCol, len] triplets —
   *  append across slices; `reset` starts a fresh accumulation. */
  matches: Uint32Array
  /** True once every retained row was indexed + verified. */
  complete: boolean
  /** Resume token for the next slice; undefined once complete. (bigint per the
   *  v0.58 wasm API; never crosses the worker protocol.) */
  cursor: bigint | undefined
  /** True when this step starts a new logical result stream: drop previously
   *  accumulated deltas. On a RESUMED cursor it means the engine restarted
   *  (content changed mid-search and the stale cursor started over). */
  reset: boolean
  /** True when eviction or the engine match cap truncated the results. */
  incompleteIndex: boolean
  /** Rows scanned so far (restarts reset it). */
  rowsFed: number
  /** Total rows this search will scan. */
  totalRows: number
}

/** The getter surface both modules' `BudgetedSearchResult` steps expose (v0.58
 *  delta contract; the pin gate keeps blobs and this glue in lockstep). */
export type WasmBudgetedStep = {
  matches: Uint32Array
  complete: boolean
  cursor: bigint | undefined
  reset: boolean
  incomplete_index: boolean
  rows_fed: number
  total_rows: number
  free: () => void
}

/** Copy a wasm budgeted-search step to a plain object and FREE the wasm side
 *  immediately — slices arrive many times per search, so leaving them to the
 *  finalization registry would pin linear memory across the whole run. */
export function copyBudgetedStep(step: WasmBudgetedStep): EngineBudgetedSearchStep {
  const out: EngineBudgetedSearchStep = {
    matches: step.matches,
    complete: step.complete,
    cursor: step.cursor,
    reset: step.reset,
    incompleteIndex: step.incomplete_index,
    rowsFed: step.rows_fed,
    totalRows: step.total_rows
  }
  step.free()
  return out
}
