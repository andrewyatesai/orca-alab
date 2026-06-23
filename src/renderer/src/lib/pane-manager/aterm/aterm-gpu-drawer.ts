import { loadAtermGpu } from './load-aterm-gpu'
import { injectTerminalFallbackFonts } from './inject-terminal-fallback-fonts'
import { MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermDrawerBuildConfig, AtermPainterBinding } from './aterm-drawer-config'
import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermGpuTerminal } from './aterm_gpu_web.js'

/** A GPU draw strategy that has loaded its engine, acquired a WebGL2 surface on
 *  the canvas, and only needs the controller's late-bound painter state. */
export type AtermGpuDrawerPending = {
  /** Engine handle typed as the CPU engine — `AtermGpuTerminal` mirrors its full
   *  state surface, so every input/search/reply handler binds to it unchanged. */
  term: AtermTerminal
  cellWidth: number
  cellHeight: number
  /** The WebGL adapter/backend WebGL handed us (for logging / e2e proof). */
  adapterInfo: string
  bindPainter: (binding: AtermPainterBinding) => AtermDrawStrategy
}

/** GPU strategy: `aterm-gpu-web`'s engine draws the grid straight into a WebGL2
 *  canvas surface (wgpu's WebGL backend) — no CPU readback on the present path.
 *  The canvas is webgl2-owned (a canvas can't ALSO be 2d), so search highlights
 *  paint on a SEPARATE stacked 2d overlay the controller creates
 *  (`needsSearchOverlay: true`).
 *
 *  `init(canvas)` is async (browsers can't block the main thread for GPU acquire)
 *  and throws if WebGL is unavailable — the caller then falls back to the CPU
 *  drawer. A `webglcontextlost` listener disposes the GPU path + swaps to CPU. */
export async function loadAtermGpuDrawer(
  config: AtermDrawerBuildConfig
): Promise<AtermGpuDrawerPending> {
  const { canvas, themeColors, fontPx } = config

  const { AtermGpuTerminal: AtermGpuTerminalCtor, fontBytes } = await loadAtermGpu()
  const gpuTerm: AtermGpuTerminal = new AtermGpuTerminalCtor(
    MIN_GRID_ROWS,
    MIN_GRID_COLS,
    fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  // Inject the local OS CJK + colour-emoji fallback faces BEFORE init so the
  // engine re-applies them to the GPU face it builds there (else CJK/emoji tofu).
  await injectTerminalFallbackFonts(gpuTerm)
  // ASYNC: acquire the GPU + create the WebGL2 surface on this canvas. Throws a
  // JS string if WebGL is unavailable; the caller catches → CPU fallback.
  await gpuTerm.init(canvas)

  const cellWidth = gpuTerm.cell_width
  const cellHeight = gpuTerm.cell_height
  // Structural cast: AtermGpuTerminal exposes the full AtermTerminal state
  // surface (scroll/selection/search/mouse/link/cursor/focus), so the input
  // handlers bind to it as the CPU engine type with no behavioral change.
  const term = gpuTerm as unknown as AtermTerminal

  return {
    term,
    cellWidth,
    cellHeight,
    adapterInfo: gpuTerm.adapter_info,
    bindPainter: (binding) => {
      let contextLost = false
      // A lost WebGL2 context can't draw; dispose the GPU path + swap to CPU.
      // Mirrors terminal-webgl-auto-policy's context-loss → fallback intent.
      const onLost = (event: Event): void => {
        event.preventDefault() // ask the browser to keep the canvas (for the swap)
        if (contextLost) {
          return
        }
        contextLost = true
        binding.onContextLoss()
      }
      canvas.addEventListener('webglcontextlost', onLost)

      const drawFrame = (): void => {
        if (binding.isDisposed() || contextLost || !binding.drawScheduler.isScheduled()) {
          return
        }
        binding.drawScheduler.consume()
        // Re-index the active search at most once per frame (coalesced from N PTY
        // chunks) so overlay highlights track current content (parity w/ CPU).
        if (binding.takeSearchRefresh() && binding.searchController.hasActiveQuery()) {
          binding.searchController.refresh()
        }
        try {
          // Present the engine grid straight into the WebGL2 swapchain.
          gpuTerm.render()
        } catch {
          // A draw error after a silent context drop: fall back to CPU.
          if (!contextLost) {
            contextLost = true
            binding.onContextLoss()
          }
          return
        }
        // wgpu sizes the canvas DRAWING buffer (canvas.width/height) to the
        // swapchain; set the CSS size so it displays at logical px (device/dpr),
        // mirroring the CPU painter. Read dpr live so a DPI move updates it.
        const dpr = binding.getDpr()
        if (canvas.width > 0 && canvas.height > 0) {
          canvas.style.width = `${canvas.width / dpr}px`
          canvas.style.height = `${canvas.height / dpr}px`
        }
      }

      return {
        term,
        getCanvas: () => canvas,
        // Grid canvas is webgl2-only; search highlights need a separate 2d overlay.
        needsSearchOverlay: true,
        drawFrame,
        resize: (rows, cols) => gpuTerm.resize(rows, cols),
        dispose: () => {
          canvas.removeEventListener('webglcontextlost', onLost)
          try {
            gpuTerm.free()
          } catch {
            /* ignore */
          }
        }
      }
    }
  }
}
