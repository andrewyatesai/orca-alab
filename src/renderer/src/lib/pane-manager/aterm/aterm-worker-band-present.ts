import type { AtermTerminal } from './aterm_wasm.js'

/** Dirty-band present for the worker's CPU engine (audit E3): size the canvas,
 *  then blit only the engine's exported damage bands out of the persistent RGBA
 *  buffer. This canvas carries ONLY engine pixels — search/link/prediction
 *  overlays live on the main thread's separate stacked canvas — so band blits
 *  are unconditionally safe here, and zero bands (a byte-identical frame) skip
 *  the canvas entirely. Call right after `t.render()`; the rgba/band pointers
 *  are read synchronously per the rgba_ptr discipline. */
export function presentCpuFrameBands(
  canvasCtx: OffscreenCanvasRenderingContext2D,
  canvas: OffscreenCanvas,
  t: AtermTerminal,
  memory: WebAssembly.Memory,
  width: number,
  height: number
): void {
  // A size assign clears the backing store, so it always forces a full blit.
  let resized = false
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width
    canvas.height = height
    resized = true
  }
  // `?.()` tolerates a pre-band wasm artifact (falls back to the full blit).
  const bandCount: number | undefined = t.present_band_count?.()
  if (!resized && bandCount === 0) {
    return
  }
  const view = new Uint8ClampedArray(memory.buffer, t.rgba_ptr(), width * height * 4)
  const frame = new ImageData(view, width, height)
  if (resized || bandCount === undefined) {
    canvasCtx.putImageData(frame, 0, 0)
    return
  }
  // Packed x,y,w,h quads, frame-absolute device px.
  const bands = new Int32Array(memory.buffer, t.present_bands_ptr(), bandCount * 4)
  for (let i = 0; i < bands.length; i += 4) {
    canvasCtx.putImageData(frame, 0, 0, bands[i], bands[i + 1], bands[i + 2], bands[i + 3])
  }
}
