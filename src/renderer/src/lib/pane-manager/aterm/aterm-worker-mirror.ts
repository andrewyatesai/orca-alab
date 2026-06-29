import { loadAterm } from './load-aterm'
import { injectTerminalFallbackFonts } from './inject-terminal-fallback-fonts'
import { seedAtermPalette, seedAtermReplyDefaults } from './aterm-theme-colors'
import { MIN_GRID_COLS, MIN_GRID_ROWS } from './aterm-grid-size'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermPendingStrategy } from './aterm-strategy-select'
import type { AtermDrawerBuildConfig, AtermPainterBinding } from './aterm-drawer-config'
import type { AtermDrawStrategy } from './aterm-draw-strategy'
import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermWorkerRequest, AtermWorkerState } from './aterm-render-worker-protocol'

// OPT-IN, default-OFF render mirror (plan §9, stage 2a). The renderer main thread
// keeps a REAL aterm engine so the facade's SYNCHRONOUS queries (serialize/
// selection/rowText/search/cursor) work unchanged; a worker holds a SECOND engine
// that owns the pane's transferred OffscreenCanvas and does the rasterize+blit
// off-main. Both engines are fed the same PTY bytes in order, and the controller
// stays untouched by handing it a Proxy `term` that reads from the main engine and
// mirrors the render-affecting mutations to the worker.

/** Fetch the OS fallback faces as raw bytes for the WORKER engine. The main engine
 *  gets its own copy via injectTerminalFallbackFonts; the worker has no window.api,
 *  so the bytes are sent (and transferred) to it. CJK must be first — the worker's
 *  set_fallback_font RESETS the chain to it — then the script chain. Tolerant: any
 *  failure → [] (JetBrains Mono still covers Latin). */
async function fetchWorkerFallbackFonts(): Promise<Uint8Array[]> {
  try {
    const { cjk, chain } = await window.api.fonts.getTerminalFallbackFonts()
    const faces: Uint8Array[] = []
    if (cjk) {
      faces.push(new Uint8Array(cjk.bytes))
    }
    for (const face of chain ?? []) {
      faces.push(new Uint8Array(face.bytes))
    }
    return faces
  } catch {
    return []
  }
}

/** Build the MAIN engine: byte-for-byte the same setup as aterm-cpu-drawer so its
 *  query surface (serialize/selection/rowText/...) is identical. It NEVER renders —
 *  the worker owns the canvas — it only parses bytes to keep the queries correct. */
async function buildMainEngine(config: AtermDrawerBuildConfig): Promise<AtermTerminal> {
  const { themeColors, fontPx } = config
  const { AtermTerminal: AtermTerminalCtor, fontBytes } = await loadAterm()
  const term: AtermTerminal = new AtermTerminalCtor(
    MIN_GRID_ROWS,
    MIN_GRID_COLS,
    fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  await injectTerminalFallbackFonts(term)
  seedAtermPalette(term, themeColors)
  term.set_selection_fg(themeColors.selectionForeground ?? undefined)
  term.set_selection_inactive_bg(themeColors.selectionInactive ?? undefined)
  seedAtermReplyDefaults(term, themeColors, term.cell_width, term.cell_height)
  return term
}

export async function loadAtermWorkerMirror(
  config: AtermDrawerBuildConfig
): Promise<AtermPendingStrategy> {
  const { canvas, themeColors, fontPx } = config
  const mainEngine = await buildMainEngine(config)
  const cellWidth = mainEngine.cell_width
  const cellHeight = mainEngine.cell_height

  // Vite (renderer worker:{format:'es'}) bundles the worker from this URL.
  const worker = new Worker(new URL('./aterm-render-worker.ts', import.meta.url), {
    type: 'module'
  })
  const post = (msg: AtermWorkerRequest, transfer?: Transferable[]): void => {
    if (transfer) {
      worker.postMessage(msg, transfer)
    } else {
      worker.postMessage(msg)
    }
  }

  // Keep the latest worker snapshot for a later stage to feed back into the facade;
  // stage 2a only needs the render itself to happen off-main. Exposed under the e2e
  // flag so the spec can prove the off-main render without main-thread canvas
  // readback (the canvas is transferred, so getContext/toDataURL no longer work).
  worker.addEventListener('message', (event: MessageEvent<AtermWorkerState>) => {
    if (e2eConfig.exposeStore) {
      window.__atermWorkerRenderState = event.data
    }
  })

  // Hand the canvas to the worker; from here ONLY the worker may draw to it
  // (getContext on the main side now throws), which is why the main engine renders
  // nothing.
  const offscreen = canvas.transferControlToOffscreen()
  // Copy the SHARED primary font before transferring its buffer so the cached
  // fontBytes other panes reuse isn't detached.
  const { fontBytes } = await loadAterm()
  const fontBytesCopy = fontBytes.slice()
  const fallbackFonts = await fetchWorkerFallbackFonts()
  post(
    {
      type: 'init',
      canvas: offscreen,
      fontBytes: fontBytesCopy,
      fallbackFonts,
      // The main engine starts at MIN grid; the controller's strategy.resize sizes
      // the real grid, which the proxy mirrors to the worker before the first draw.
      rows: MIN_GRID_ROWS,
      cols: MIN_GRID_COLS,
      fontPx,
      themeColors
    },
    [offscreen, fontBytesCopy.buffer, ...fallbackFonts.map((f) => f.buffer)]
  )

  // The PROXY term: reads forward to the main engine (the sync source of truth); the
  // mutators that change RENDERED state also drive the worker so both engines stay
  // in lockstep. Theme/line-height/cursor/search mutations are NOT mirrored yet
  // (stage 2b) — they only affect the main engine's queries here.
  const term = new Proxy(mainEngine, {
    get(target, prop) {
      switch (prop) {
        case 'process_str':
          return (s: string): void => {
            target.process_str(s)
            post({ type: 'process', data: s })
          }
        case 'render':
          // The worker owns the canvas, so DON'T render on main; ask the worker to.
          return (): void => post({ type: 'draw' })
        case 'resize':
          return (rows: number, cols: number): void => {
            target.resize(rows, cols)
            post({ type: 'resize', rows, cols })
          }
        case 'set_px':
          return (px: number): void => {
            target.set_px(px)
            post({ type: 'setPx', px })
          }
        case 'scroll_lines':
          return (delta: number): void => {
            target.scroll_lines(delta)
            post({ type: 'scrollLines', delta })
          }
        case 'scroll_to_bottom':
          return (): void => {
            target.scroll_to_bottom()
            post({ type: 'scrollToBottom' })
          }
        default: {
          // Bind methods to the REAL target (not the proxy) so wasm-bindgen's
          // internal `this.__wbg_ptr` access works and reads never re-enter the proxy.
          const value = Reflect.get(target, prop, target)
          return typeof value === 'function' ? value.bind(target) : value
        }
      }
    }
  })

  const bindPainter = (_binding: AtermPainterBinding): AtermDrawStrategy => ({
    term,
    getCanvas: () => canvas,
    // Search/selection/link OVERLAYS are NOT rendered in this stage — the worker
    // blits ONLY the engine framebuffer; overlay mirroring is stage 2b. So the
    // binding's search/link getters are intentionally unused here.
    needsSearchOverlay: false,
    drawFrame: () => post({ type: 'draw' }),
    resize: (rows, cols) => term.resize(rows, cols),
    dispose: () => {
      post({ type: 'dispose' })
      worker.terminate()
      try {
        mainEngine.free()
      } catch {
        /* ignore */
      }
    }
  })

  return { kind: 'cpu', term, cellWidth, cellHeight, adapterInfo: null, bindPainter }
}
