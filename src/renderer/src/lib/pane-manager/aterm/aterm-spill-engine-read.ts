import type { AtermDeviceRect } from './aterm-chrome-box'
import type { SpillScratchReader } from './aterm-spill-pane-scratch'

// The engine spill-export read: copy the packed straight-alpha RGBA strip
// buffer out of wasm linear memory into a pane's retained scratch and derive
// the overlay-space dirty rects. Extracted from the stage-3 in-process blit so
// the stage-4 worker compositor consumes the IDENTICAL strip/ImageData/rev
// logic instead of duplicating it. Zero allocations on the steady frame path:
// per-strip ImageData and the dirty-rect records are retained and reused; only
// the wasm-memory views are rebuilt each read (they detach on wasm growth —
// the rgba_ptr rule from aterm-frame-painter).

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

export type AtermSpillScratchReader = {
  /** Arm a pass for `rev`; `read` consumes it only when bytes reach the scratch. */
  beginPass: (rev: number) => void
  /** True once the armed pass's bytes were copied into the scratch. */
  consumedRev: () => boolean
  /** The last revision whose bytes actually reached the retained scratch. */
  lastBlittedRev: () => number | null
  read: SpillScratchReader
}

/** Build the stateful scratch-refresh reader for one engine. The caller arms a
 *  revision (beginPass), hands `read` to the compositor pass, then checks
 *  consumedRev(): false = geometry/export mismatch, keep the rev unconsumed and
 *  retry once geometry catches up. */
export function createAtermSpillScratchReader(
  term: Pick<
    AtermSpillEngineReads,
    'spill_len' | 'spill_ptr' | 'spill_rect_count' | 'spill_rects_ptr'
  >,
  memory: { readonly buffer: ArrayBufferLike }
): AtermSpillScratchReader {
  let lastBlittedRev: number | null = null
  let passRev = 0
  let consumed = false
  // Retained per-strip ImageData (reallocated only when a strip's size changes)
  // and the reused dirty-rect records handed back to the pass each read.
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

  const read: SpillScratchReader = ({ ctx, strips }) => {
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
    consumed = true
    lastBlittedRev = passRev
    return dirtyOut
  }

  return {
    beginPass: (rev) => {
      passRev = rev
      consumed = false
    },
    consumedRev: () => consumed,
    lastBlittedRev: () => lastBlittedRev,
    read
  }
}
