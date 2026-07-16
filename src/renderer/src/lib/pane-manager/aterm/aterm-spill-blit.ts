import type { AtermDeviceRect } from './aterm-chrome-box'
import { atermSpillOverlay, type AtermSpillOverlay } from './aterm-spill-overlay'
import type { SpillScratchReader } from './aterm-spill-pane-scratch'

// The IN-PROCESS spill read (stage 3): after each painted frame, copy the
// engine's chrome-band export (a packed straight-alpha RGBA strip buffer in
// wasm linear memory) into the pane's retained scratch and run the overlay's
// clear-union + intersect-expansion pass. The worker path stays dark here —
// its compositor is stage 4 and runs worker-side.

/** The engine spill surface both wasm bindings expose (v0.48+). The worker
 *  facade term has none of these, so `hasAtermSpillExports` excludes it. */
export type AtermSpillEngineReads = {
  /** Window chrome extents (device px); 0/0 = no band, nothing to read. */
  readonly chrome_pad?: number
  readonly chrome_head?: number
  spill_rev(): number
  spill_rect_count(): number
  spill_rects_ptr(): number
  spill_ptr(): number
  spill_len(): number
}

/** True when the pinned engine artifact exports the whole spill surface.
 *  Checked per method (not per version) so glue/blob skew fails closed. */
export function hasAtermSpillExports(term: object): boolean {
  const reads = term as Partial<Record<keyof AtermSpillEngineReads, unknown>>
  return (
    typeof reads.spill_rev === 'function' &&
    typeof reads.spill_rect_count === 'function' &&
    typeof reads.spill_rects_ptr === 'function' &&
    typeof reads.spill_ptr === 'function' &&
    typeof reads.spill_len === 'function'
  )
}

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
 *  the steady frame path: the per-strip ImageData and the dirty-rect list are
 *  retained and reused; only the wasm-memory views are rebuilt each read (they
 *  detach on wasm growth — the rgba_ptr rule from aterm-frame-painter). */
export function createAtermSpillBlit(deps: AtermSpillBlitDeps): () => void {
  const { term, memory } = deps
  const overlay = deps.overlay ?? atermSpillOverlay
  /** The last revision whose bytes actually reached the retained scratch. */
  let lastBlittedRev: number | null = null
  let retryRev: number | null = null
  let retriesLeft = 0
  /** Revision being read by the current pass (set before the reader runs). */
  let passRev = 0
  let readerRan = false
  // Retained per-strip ImageData (reallocated only when a strip's size changes)
  // and the reused dirty-rect records handed back to the overlay each pass.
  const stripImages: ImageData[] = []
  const dirtyPool: AtermDeviceRect[] = []
  const dirtyOut: AtermDeviceRect[] = []

  const pooledRect = (index: number): AtermDeviceRect => {
    let rect = dirtyPool[index]
    if (!rect) {
      rect = { x: 0, y: 0, width: 0, height: 0 }
      dirtyPool[index] = rect
    }
    return rect
  }

  const readSpill: SpillScratchReader = ({ ctx, strips }) => {
    // The engine packs the band as row-major strips in chromeStripRects order
    // (top incl. head, bottom, left, right; zero-area strips hold zero bytes),
    // so the measured slots must account for the export byte-for-byte. A
    // mismatch means the DOM measure hasn't caught up with an engine resize —
    // skip WITHOUT consuming the revision and let the bounded retry re-run.
    const len = term.spill_len()
    let expected = 0
    for (const slot of strips) {
      expected += slot.overlayRect.width * slot.overlayRect.height * 4
    }
    if (len === 0 || expected !== len) {
      return null
    }
    readerRan = true
    // Rebuilt EVERY read: wasm memory growth detaches earlier views.
    const view = new Uint8ClampedArray(memory.buffer, term.spill_ptr(), len)
    let offset = 0
    for (let i = 0; i < strips.length; i++) {
      const slot = strips[i]
      const width = slot.overlayRect.width
      const height = slot.overlayRect.height
      const bytes = width * height * 4
      let image = stripImages[i]
      if (!image || image.width !== width || image.height !== height) {
        image = new ImageData(width, height)
        stripImages[i] = image
      }
      image.data.set(view.subarray(offset, offset + bytes))
      // putImageData writes raw straight-alpha pixels (no compositing) — the
      // scratch is an exact copy; blending happens on the overlay drawImage.
      ctx.putImageData(image, slot.scratchOrigin.x, slot.scratchOrigin.y)
      offset += bytes
    }
    // Engine dirty rects are FRAME-absolute; the top strip starts at the frame
    // origin, so its overlay rect anchors the frame→overlay translation.
    const frameOrigin = strips[0].overlayRect
    dirtyOut.length = 0
    // The exported rects describe only the LAST render. They bound the clear
    // correctly only when no revision was missed since the previous blit;
    // otherwise (first blit, skipped frames) the whole band is the dirty set.
    const contiguous = lastBlittedRev !== null && passRev === lastBlittedRev + 1
    const count = contiguous ? term.spill_rect_count() : 0
    if (count > 0) {
      const rects = new Int32Array(memory.buffer, term.spill_rects_ptr(), count * 4)
      for (let j = 0; j < count; j++) {
        const rect = pooledRect(j)
        rect.x = frameOrigin.x + rects[j * 4]
        rect.y = frameOrigin.y + rects[j * 4 + 1]
        rect.width = rects[j * 4 + 2]
        rect.height = rects[j * 4 + 3]
        dirtyOut.push(rect)
      }
    } else {
      for (let i = 0; i < strips.length; i++) {
        const rect = pooledRect(i)
        const strip = strips[i].overlayRect
        rect.x = strip.x
        rect.y = strip.y
        rect.width = strip.width
        rect.height = strip.height
        dirtyOut.push(rect)
      }
    }
    return dirtyOut
  }

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
    if (rev === lastBlittedRev) {
      return
    }
    passRev = rev
    readerRan = false
    overlay.runSpillPassInProcess(paneKey, readSpill)
    if (readerRan) {
      lastBlittedRev = rev
      return
    }
    // The overlay skipped the read (unregistered/unmeasured/hidden pane, or the
    // byte-length mismatch above). Keep the revision unconsumed and re-arm a
    // bounded draw so a settled burn's final band lands once geometry catches up.
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
