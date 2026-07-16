import {
  createAtermSpillScratchReader,
  type AtermSpillEngineReads
} from './aterm-spill-engine-read'
import { atermSpillOverlay, type AtermSpillOverlay } from './aterm-spill-overlay'

// The IN-PROCESS spill pass (stage 3): after each painted frame, copy the
// engine's chrome-band export (a packed straight-alpha RGBA strip buffer in
// wasm linear memory) into the pane's retained scratch and run the overlay's
// clear-union + intersect-expansion pass. The strip/ImageData/rev read itself
// lives in aterm-spill-engine-read (shared with the stage-4 worker
// compositor); this module owns the rev gate + bounded retry orchestration.

export { hasAtermSpillExports, type AtermSpillEngineReads } from './aterm-spill-engine-read'

export type AtermSpillBlitDeps = {
  term: AtermSpillEngineReads
  /** The engine module's linear memory (aterm-wasm OR aterm-gpu-web — the GPU
   *  engine rasterizes the spill band on its CPU face, so the same read works). */
  memory: { readonly buffer: ArrayBufferLike }
  /** The pane's overlay registration key; unset until the attach-edge bind. */
  getPaneKey: () => string | undefined
  isDisposed: () => boolean
  /** Re-arm a draw when a pass could not land (geometry mid-change): the next
   *  frame retries so a settling burn's FINAL state still reaches the overlay. */
  scheduleDraw: () => void
  overlay?: Pick<AtermSpillOverlay, 'runSpillPassInProcess'>
}

// A skipped pass (unmeasured/mid-resize geometry) re-arms at most this many
// draws per revision — convergence normally takes one frame, and an engine that
// can never match its measured box must not turn the presenter into a rAF loop.
const MAX_SKIPPED_PASS_RETRIES = 3

/** Build the per-paint spill pass for one in-process pane. Zero allocations on
 *  the steady frame path (the shared reader retains its ImageData + rect pools). */
export function createAtermSpillBlit(deps: AtermSpillBlitDeps): () => void {
  const { term } = deps
  const overlay = deps.overlay ?? atermSpillOverlay
  const reader = createAtermSpillScratchReader(term, deps.memory)
  let retryRev: number | null = null
  let retriesLeft = 0

  return function spillBlit(): void {
    if (deps.isDisposed()) {
      return
    }
    const paneKey = deps.getPaneKey()
    if (paneKey === undefined || paneKey.length === 0) {
      return
    }
    // Chrome 0 = no band (spill_len is 0 by the engine's identity law).
    if ((term.chrome_pad ?? 0) <= 0 && (term.chrome_head ?? 0) <= 0) {
      return
    }
    const rev = term.spill_rev()
    if (rev === reader.lastBlittedRev()) {
      return
    }
    reader.beginPass(rev)
    overlay.runSpillPassInProcess(paneKey, reader.read)
    if (reader.consumedRev()) {
      return
    }
    // The overlay skipped the read (unregistered/unmeasured/hidden pane, or the
    // export-vs-measured byte-length mismatch). Keep the revision unconsumed and
    // re-arm a bounded draw so a settled burn's final band lands once geometry
    // catches up.
    if (retryRev !== rev) {
      retryRev = rev
      retriesLeft = MAX_SKIPPED_PASS_RETRIES
    }
    if (retriesLeft > 0) {
      retriesLeft--
      deps.scheduleDraw()
    }
  }
}
