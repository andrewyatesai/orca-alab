// Feature detection for the W4A search-index release binding — it lands with a
// future aterm pin, so the glue detects it at engine build and degrades honestly
// when absent (eviction falls back to budgeted-cursor cancel alone).
//
// Deliberately NO E-1 `search_summary` modeling here: the fed design's E-6 rev
// makes the summary export budgeted (row budget + resumable cursor), so glue for
// it is written against the REAL export when it lands — a speculative unbudgeted
// wrapper would hand the federated path an E-6-defeating call the day the export
// appeared (Wave-4A/5 gate finding).

/** Detect the warm-index release binding (W4A "release-on-close/idle eviction");
 *  undefined on pins without it — §4 eviction then relies on budgeted-cursor
 *  cancel, which frees in-flight (not completed-warm) index state. */
export function detectEngineSearchIndexRelease(engine: unknown): (() => void) | undefined {
  const candidate = engine as { search_index_release?: () => void }
  if (typeof candidate.search_index_release !== 'function') {
    return undefined
  }
  return () => {
    try {
      candidate.search_index_release!()
    } catch {
      /* ignore — eviction is best-effort on skewed artifacts */
    }
  }
}
