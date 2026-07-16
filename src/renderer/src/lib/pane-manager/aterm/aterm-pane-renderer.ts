import { resolveAtermThemeColors } from './aterm-theme-colors'
import { buildAtermInputDom } from './aterm-input-dom'
import { loadAtermStrategy, type AtermPendingStrategy } from './aterm-strategy-select'
import { loadAtermCpuDrawer } from './aterm-cpu-drawer'
import { createStableAtermPaneController } from './aterm-pane-stable-controller'
import {
  wireAtermPane,
  type AtermSharedLateBindings,
  type AtermWiredPane
} from './aterm-pane-wiring'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermLinkContext } from './aterm-url-link-routing'
import type {
  AtermPaneController,
  AtermPaneControllerOptions,
  AtermPaneInputSink,
  AtermPanePasteSink,
  AtermPaneResizeSink
} from './aterm-pane-controller-types'
import {
  EMPTY_ATERM_RAIN_PULSE_BUFFER,
  bufferAtermRainPulse,
  resumeAtermRainPulses,
  type AtermRainPulse,
  type AtermRainPulseBuffer
} from '../../../../../shared/aterm-rain-signal'

export type { AtermLinkContext } from './aterm-url-link-routing'
export type {
  AtermPaneController,
  AtermPaneControllerOptions,
  AtermPaneInputSink,
  AtermPanePasteSink,
  AtermPaneResizeSink
} from './aterm-pane-controller-types'

// Font cell size in CSS pixels; multiplied by devicePixelRatio for the engine.
// Imported from the shared home (used at construction) + re-exported so existing
// importers are unaffected.
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'
export { ATERM_RENDERER_FONT_PX }

/** Build a grid `<canvas>` styled to fill the pane. The framebuffer is sized to
 *  device pixels (CSS = device/dpr), so at a correctly-reconciled dpr it maps 1:1
 *  to the panel and needs no resampling; we deliberately do NOT set
 *  `imageRendering: pixelated` — under any residual fractional-dpr mismatch
 *  nearest-neighbor drops/doubles whole glyph rows, which is worse than the
 *  browser's default smoothing. A fresh one is built per strategy because a
 *  webgl2-poisoned canvas can't be reused for the CPU 2d fallback. */
function buildAtermGridCanvas(themeColors: { bg: number }): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  canvas.dataset.testid = 'aterm-canvas' // e2e locator for the aterm-rendered pane
  canvas.style.width = '100%'
  canvas.style.height = '100%'
  canvas.style.display = 'block'
  canvas.style.outline = 'none'
  // E2E only: stamp the seeded default bg (per-pane) so the theme test can assert
  // the painted top-left pixel matches the configured theme background.
  if (e2eConfig.exposeStore) {
    const { bg } = themeColors
    canvas.dataset.atermBg = `${(bg >> 16) & 0xff},${(bg >> 8) & 0xff},${bg & 0xff}`
  }
  return canvas
}

/** Force-release a canvas's WebGL2 context so it returns to the browser's hard
 *  ~16-live-context budget immediately instead of lingering until GC. No-op for a
 *  2d (CPU) canvas — `getContext('webgl2')` returns null once a 2d context exists. */
function releaseGlContext(c: HTMLCanvasElement): void {
  try {
    c.getContext('webgl2')?.getExtension('WEBGL_lose_context')?.loseContext()
  } catch {
    /* ignore */
  }
}

const SWAP_BUFFER_CAP = 8_000_000

/** Bounded live data retained only while a lost GPU drawer is replaced. Output
 * stays ordered; semantic rain keeps the engine's strongest-pulse + turn latch. */
function createAtermRendererSwapGap(): {
  begin: () => void
  bufferOutput: (data: string) => boolean
  bufferRainPulse: (pulse: AtermRainPulse) => boolean
  drain: () => { output: string[]; rainPulses: AtermRainPulse[] }
  clear: () => void
} {
  let output: string[] | null = null
  let outputBytes = 0
  let rain: AtermRainPulseBuffer = EMPTY_ATERM_RAIN_PULSE_BUFFER
  const clear = (): void => {
    output = null
    outputBytes = 0
    rain = EMPTY_ATERM_RAIN_PULSE_BUFFER
  }
  return {
    begin: () => {
      output = []
      outputBytes = 0
      rain = EMPTY_ATERM_RAIN_PULSE_BUFFER
    },
    bufferOutput: (data) => {
      if (!output) {
        return false
      }
      if (outputBytes < SWAP_BUFFER_CAP) {
        output.push(data)
        outputBytes += data.length
      }
      return true
    },
    bufferRainPulse: (pulse) => {
      if (!output) {
        return false
      }
      rain = bufferAtermRainPulse(rain, pulse)
      return true
    },
    drain: () => {
      const buffered = { output: output ?? [], rainPulses: resumeAtermRainPulses(rain) }
      clear()
      return buffered
    },
    clear
  }
}

export async function createAtermPaneController(
  container: HTMLElement,
  onInput: AtermPaneInputSink,
  onResize: AtermPaneResizeSink,
  onPaste: AtermPanePasteSink,
  linkContext?: AtermLinkContext,
  controllerOptions?: AtermPaneControllerOptions
): Promise<AtermPaneController> {
  // Seed default fg/bg/cursor/selection from orca's active terminal theme.
  const themeColors = resolveAtermThemeColors()
  const dpr = window.devicePixelRatio || 1
  // Base CSS font px from the user's terminalFontSize (else the engine default);
  // scaled to device px so glyphs rasterize at full Retina resolution.
  const baseFontPx = controllerOptions?.getFontPx?.() ?? ATERM_RENDERER_FONT_PX
  const fontPx = Math.round(baseFontPx * dpr)

  // Mirror xterm's DOM so the app's focus/paste/IME/clipboard logic keeps working.
  let canvas = buildAtermGridCanvas(themeColors)
  const inputDom = buildAtermInputDom(canvas)
  container.appendChild(inputDom.wrapper)

  // Openers are late-bound by the lifecycle AFTER creation; held here so a GPU→CPU
  // context-loss rebuild carries them onto the replacement wiring.
  const shared: AtermSharedLateBindings = {
    fileLinkOpener: null,
    activeLinkContext: linkContext,
    linkProviderSource: null,
    spillPaneKey: null
  }

  // Choose the strategy: GPU when opted-in + a webgl2 context is creatable, else
  // CPU (the default + fallback). GPU init failure already falls back inside
  // loadAtermStrategy, so this never leaves a pane without a renderer.
  const pending = await loadAtermStrategy({
    canvas,
    themeColors,
    fontPx,
    // Thread the user's terminalLineHeight so the worker engine builds its first cell box
    // at the real line-height -- else the initial off-main snapshot reports a line-height-1
    // cell, the first computeGrid over-counts rows, and a spurious first-open SIGWINCH
    // fires. In-process drawers ignore it (the wiring's set_line_height applies it there).
    lineHeight: controllerOptions?.getLineHeight?.() ?? 1,
    // If the worker attempt poisons the transferred canvas, rebuild a fresh one in
    // place so the in-process fallback has a usable surface (else the pane is dead).
    rebuildCanvas: () => {
      const fresh = buildAtermGridCanvas(themeColors)
      canvas.parentElement?.replaceChild(fresh, canvas)
      canvas = fresh
      return fresh
    }
  })
  if (e2eConfig.exposeStore) {
    // e2e-only GPU-vs-CPU frame-time benchmark. Self-contained (builds fresh CPU
    // + GPU engines on throwaway canvases), so it's exposed whenever the aterm
    // renderer is up — independent of which path THIS pane took — letting the perf
    // spec time both paths back-to-back at several grid sizes.
    window.__atermGpuCpuBench = async (sizes, frames) => {
      const { benchAtermGpuVsCpu } = await import('./aterm-gpu-cpu-bench')
      return benchAtermGpuVsCpu({ sizes, frames, fontPx, themeColors })
    }
    // e2e-only keystroke-latency benchmark: render-half (single-cell
    // process→render→present) median/p95 for the aterm CPU + GPU paths, plus a
    // head-to-head per-frame table vs a real off-screen xterm + WebGL addon (the
    // renderer Orca replaced). Self-contained (throwaway engines/canvases), so it's
    // exposed whenever the aterm renderer is up, independent of this pane's path.
    window.__atermLatencyBench = async (sizes, iterations, warmup, frames) => {
      const { benchAtermLatency } = await import('./aterm-latency-bench')
      return benchAtermLatency({ sizes, iterations, warmup, frames, fontPx, themeColors })
    }
    // e2e-only per-pane memory bench: wasm-heap growth per live engine (grid +
    // scrollback + framebuffer + atlas); fonts are interned/shared, so excluded.
    window.__atermMemoryBench = async (cols, rows, scrollbackLines, panes) => {
      const { benchAtermMemory } = await import('./aterm-memory-bench')
      return benchAtermMemory({ cols, rows, scrollbackLines, panes, fontPx, themeColors })
    }
  }
  if (pending.kind === 'gpu' && e2eConfig.exposeStore) {
    // e2e proof hooks: the wgpu WebGL adapter string + a GPU==CPU parity probe.
    // The live-canvas pixels are read directly via gl.readPixels in the spec; we
    // do NOT expose render_offscreen() because its buffer-map readback uses a
    // blocking device.poll(Wait) that deadlocks on WebGL2 (no synchronous poll).
    window.__atermGpuAdapterInfo = pending.adapterInfo ?? undefined
    // GPU==CPU parity probe over a fresh pair of engines on a throwaway canvas.
    window.__atermGpuVsCpuCompare = async (bytesAsLatin1, rows, cols) => {
      const { compareAtermGpuVsCpu } = await import('./aterm-gpu-cpu-compare')
      const bytes = Uint8Array.from(bytesAsLatin1, (ch) => ch.charCodeAt(0))
      const probeCanvas = document.createElement('canvas')
      return compareAtermGpuVsCpu({ rows, cols, fontPx, themeColors, bytes, canvas: probeCanvas })
    }
  }

  let wired: AtermWiredPane
  // Set by dispose() so an in-flight context-loss swap that resolves AFTER teardown
  // doesn't build a fresh (never-torn-down) engine + listeners.
  let controllerDisposed = false
  // Selection-mutation subscribers live HERE (not in the wiring) so a GPU→CPU
  // context-loss rebuild keeps the facade's onSelectionChange bridge alive; each
  // fresh wiring re-forwards into this set (see forwardSelectionMutations).
  const selectionMutationListeners = new Set<() => void>()
  const forwardSelectionMutations = (next: AtermWiredPane): void =>
    next.controller.onSelectionMutation(() =>
      selectionMutationListeners.forEach((listener) => listener())
    )

  // Context-loss rebuild: a lost WebGL2 context can't draw, so tear down the GPU
  // wiring + its poisoned canvas, build a fresh canvas, load the CPU drawer, and
  // re-wire. The returned controller delegates to `wired`, so swapping it here
  // transparently moves the live pane onto the CPU path (mirrors the auto-policy
  // fallback). Guarded so a second loss event during teardown is a no-op.
  let swapping = false
  // PTY bytes that arrive during the async GPU→CPU swap (teardown is synchronous,
  // the new wiring resolves a tick later). While non-null the stable controller
  // buffers output here instead of dropping it on the torn-down wiring; the new
  // engine replays it in order once ready, so no live output is lost mid-swap.
  const swapGap = createAtermRendererSwapGap()
  const swapToCpu = (seedAnsi?: string): void => {
    if (swapping || controllerDisposed) {
      return
    }
    swapping = true
    swapGap.begin()
    console.warn('[aterm] draw path lost; swapping pane to the in-process CPU renderer')
    wired.teardown()
    if (e2eConfig.exposeStore) {
      // The GPU path is gone; drop its e2e proof hooks so they can't be probed.
      window.__atermGpuAdapterInfo = undefined
      window.__atermGpuVsCpuCompare = undefined
    }
    // Don't loseContext() the old canvas here: its GL context is ALREADY lost (that
    // triggered this swap), and re-touching it was observed to break the recovered
    // pane's rendering. The browser reclaims the dead context on its own.
    const freshCanvas = buildAtermGridCanvas(themeColors)
    // The canvas lives inside the xterm-screen div; swap it there in place so the
    // helper textarea + DOM shim are untouched.
    canvas.parentElement?.replaceChild(freshCanvas, canvas)
    canvas = freshCanvas
    void loadAtermCpuDrawer({ canvas, themeColors, fontPx })
      .then((cpu) => {
        // The pane was disposed while the CPU drawer loaded: free the just-built
        // engine and drop the canvas rather than wiring a pane nothing tears down.
        if (controllerDisposed) {
          try {
            cpu.term.free()
          } catch {
            /* ignore */
          }
          freshCanvas.remove()
          swapGap.clear()
          return
        }
        const nextPending: AtermPendingStrategy = {
          kind: 'cpu',
          term: cpu.term,
          cellWidth: cpu.cellWidth,
          cellHeight: cpu.cellHeight,
          memory: cpu.memory,
          adapterInfo: null,
          bindPainter: cpu.bindPainter
        }
        wired = wireAtermPane({
          pending: nextPending,
          canvas,
          container,
          element: inputDom.wrapper,
          textarea: inputDom.textarea,
          liveRegion: inputDom.liveRegion,
          themeColors,
          inputSink: onInput,
          resizeSink: onResize,
          pasteSink: onPaste,
          controllerOptions,
          shared,
          onContextLoss: () => undefined // CPU never loses a GL context
        })
        forwardSelectionMutations(wired)
        // A worker-crash swap seeds the fresh engine from the dead engine's last
        // serialized state — aterm's OWN replayable ANSI (unlike the xterm shim
        // buffer below), so the grid repaints instead of staying blank.
        if (seedAnsi) {
          wired.controller.process(seedAnsi)
        }
        // Flush PTY output that arrived during the swap into the fresh engine, in
        // arrival order, BEFORE clearing the buffer so subsequent live bytes follow.
        const buffered = swapGap.drain()
        for (const chunk of buffered.output) {
          wired.controller.process(chunk)
        }
        for (const pulse of buffered.rainPulses) {
          wired.controller.noteMatrixRainPulse(pulse)
        }
        // We deliberately do NOT replay the xterm shim's SerializeAddon buffer into
        // the fresh engine — its reconstructed output isn't guaranteed compatible with
        // aterm's parser and was observed to corrupt the recovered state. So a pane
        // with NO output during/after the swap stays blank until the next PTY write
        // repaints it (self-healing); the gap's live output above is preserved.
        // Paint the fresh engine now so the recovered pane shows its current state
        // even if no buffered/live bytes followed the swap.
        wired.controller.scheduleDraw()
        swapping = false
      })
      .catch((err) => {
        // The CPU-drawer load (first wasm/font fetch on a GPU pane) rejected — e.g.
        // a transient/offline asset failure. Release the buffered output rather than
        // holding it forever (the unbounded-leak guard the buffer needs); the pane
        // can't recover here, but it must not grow the heap. swapping stays true so a
        // repeat context-loss event doesn't re-enter a known-broken load.
        console.error('[aterm] CPU renderer load failed after context loss', err)
        swapGap.clear()
      })
  }

  wired = wireAtermPane({
    pending,
    canvas,
    container,
    element: inputDom.wrapper,
    textarea: inputDom.textarea,
    liveRegion: inputDom.liveRegion,
    themeColors,
    inputSink: onInput,
    resizeSink: onResize,
    pasteSink: onPaste,
    linkContext,
    controllerOptions,
    shared,
    onContextLoss: swapToCpu
  })
  forwardSelectionMutations(wired)

  // Stable controller: every method delegates to the CURRENT wiring so a
  // context-loss swap is invisible to the caller (which holds this object).
  return createStableAtermPaneController({
    // Buffer output during a GPU→CPU swap (the old wiring is torn down and the new
    // one resolves a tick later); the new engine replays it. Else delegate live.
    process: (data) => {
      if (controllerDisposed) {
        return
      }
      if (swapGap.bufferOutput(data)) {
        return
      }
      wired.controller.process(data)
    },
    noteMatrixRainPulse: (pulse) => {
      if (controllerDisposed) {
        return
      }
      if (swapping && swapGap.bufferRainPulse(pulse)) {
        return
      }
      wired.controller.noteMatrixRainPulse(pulse)
    },
    onSelectionMutation: (handler) => void selectionMutationListeners.add(handler),
    current: () => wired.controller,
    element: inputDom.wrapper,
    textarea: inputDom.textarea,
    dispose: () => {
      controllerDisposed = true
      // Release any output buffered mid-swap so a pane disposed during a GPU→CPU
      // swap doesn't retain it (the process() gate above also drops once disposed).
      swapGap.clear()
      inputDom.wrapper.remove()
      wired.teardown()
      // Release the live canvas's WebGL2 context (no-op on the CPU path) so closing
      // panes don't accumulate against the browser's GL-context budget.
      releaseGlContext(canvas)
    }
  })
}
