// Startup probes for the worker spill compositor (the aterm-gpu-probe pattern:
// cheap, cached, reset-able for tests). Two questions from the plan's risk
// list, both answered as honestly as the platform allows:
//
//  (a) Do two OffscreenCanvases transferred to the SAME worker commit in the
//      same rendering update (pane canvas + overlay canvas)? There is NO web
//      API that can observe worker commit scheduling from the main thread, so
//      this is UNPROVABLE by construction — per plan risk 1 we ship anyway
//      (failure mode is a cosmetic ≤1-frame seam shear on a soft-alpha ring,
//      not feature loss) and expose the verdict on a diagnostic instead of
//      pretending to know. Re-examine at Electron bumps.
//
//  (b) Does the scratch→overlay drawImage stay GPU-accelerated? Canvas-2d
//      acceleration is not queryable either; best-effort = prove the op is
//      SUPPORTED (OffscreenCanvas 2d → 2d drawImage works at all) and record
//      acceleration as unproven. Bounded cost even in software (strips are
//      ~0.28MP/pane worst case, off-main), so supported is the ship gate.

export type AtermSpillWorkerProbeResult = {
  /** OffscreenCanvas + transferControlToOffscreen + a 2d OffscreenCanvas
   *  context all exist — the worker overlay path is attemptable. */
  available: boolean
  /** Same-rendering-update commit coherence across two OffscreenCanvases:
   *  null = unprovable from the main thread (see header) — shipped anyway. */
  dualCanvasCommitCoherent: boolean | null
  /** OffscreenCanvas→OffscreenCanvas 2d drawImage executes without throwing. */
  scratchBlitSupported: boolean
  /** Whether that drawImage stays on the GPU: not queryable — null. */
  scratchBlitAccelerated: boolean | null
  detail: string
}

let cached: AtermSpillWorkerProbeResult | null = null

export function resetAtermSpillWorkerProbe(): void {
  cached = null
}

export function probeAtermSpillWorkerCompositing(): AtermSpillWorkerProbeResult {
  if (cached) {
    return cached
  }
  let result: AtermSpillWorkerProbeResult
  try {
    const supported =
      typeof OffscreenCanvas === 'function' &&
      typeof HTMLCanvasElement !== 'undefined' &&
      typeof HTMLCanvasElement.prototype.transferControlToOffscreen === 'function'
    if (!supported) {
      result = {
        available: false,
        dualCanvasCommitCoherent: null,
        scratchBlitSupported: false,
        scratchBlitAccelerated: null,
        detail: 'OffscreenCanvas/transferControlToOffscreen unavailable'
      }
    } else {
      // Best-effort (b): a throwaway scratch→overlay blit on this thread — the
      // worker uses the same primitives on the same engine version.
      const scratch = new OffscreenCanvas(8, 8)
      const overlay = new OffscreenCanvas(8, 8)
      const scratchCtx = scratch.getContext('2d')
      const overlayCtx = overlay.getContext('2d')
      let blitOk = false
      if (scratchCtx && overlayCtx) {
        scratchCtx.putImageData(new ImageData(8, 8), 0, 0)
        overlayCtx.drawImage(scratch, 0, 0)
        blitOk = true
      }
      result = {
        available: blitOk,
        dualCanvasCommitCoherent: null,
        scratchBlitSupported: blitOk,
        scratchBlitAccelerated: null,
        detail: blitOk
          ? 'dual-canvas commit coherence unprovable from main (risk 1: ship, ≤1-frame shear); 2d acceleration not queryable'
          : 'OffscreenCanvas 2d context unavailable'
      }
    }
  } catch (err) {
    result = {
      available: false,
      dualCanvasCommitCoherent: null,
      scratchBlitSupported: false,
      scratchBlitAccelerated: null,
      detail: `probe threw: ${String(err)}`
    }
  }
  cached = result
  // Diagnostic surface (e2e + support bundles read it off the window).
  if (typeof window !== 'undefined') {
    const w = window as unknown as { __atermSpillWorkerProbe?: AtermSpillWorkerProbeResult }
    w.__atermSpillWorkerProbe = result
  }
  return result
}
