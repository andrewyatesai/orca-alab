// Builds one aterm engine per pane inside the SHARED render worker (CPU or GPU) and
// normalizes the few CPU/GPU differences (process encoding, render/present,
// framebuffer size, search arity) behind one EngineHandle. The worker terminal
// (aterm-worker-terminal) drives reads/commands through this handle so it's
// engine-agnostic. Each wasm module (aterm_wasm / aterm_gpu_web) is instantiated
// ONCE for the whole worker — `init`/`gpuInit` are idempotent (wasm-bindgen caches
// the instance) — so every engine of a kind shares one linear memory and the
// engine-side content-keyed font intern registry dedupes the faces across panes.

import init, { AtermTerminal, register_font as registerCpuFont } from './aterm_wasm.js'
import wasmUrl from './aterm_wasm_bg.wasm?url'
import gpuInit, { AtermGpuTerminal, register_font as registerGpuFont } from './aterm_gpu_web.js'
import gpuWasmUrl from './aterm_gpu_web_bg.wasm?url'
import { seedAtermPalette, seedAtermReplyDefaults } from './aterm-theme-colors'
import type { AtermThemeColors } from './aterm-theme-colors'
import type { AtermWorkerFonts } from './aterm-render-worker-protocol'

/** The worker-resident immutable font faces (sent once per worker generation).
 *  Each wasm module registers them ONCE via `register_font` (one marshal per
 *  blob per module); every engine build after that passes 4-byte HANDLES — the
 *  ~100–400MB faces are never re-copied across the JS/wasm boundary per pane
 *  (the transient copies fragmented the linear memory into a ~183MB/pane
 *  high-water ratchet: wasm memory never shrinks). */
export type WorkerResidentFonts = Pick<
  AtermWorkerFonts,
  'primary' | 'fallbacks' | 'emoji' | 'symbol'
>

/** Handles into a wasm module's font registry (module-scoped: each module has
 *  its own linear memory, so CPU and GPU register independently). */
type RegisteredFontHandles = {
  primary: number
  fallbacks: number[]
  emoji: number | null
  symbol: number | null
}

// Once per module per worker generation (fonts are immutable within one).
let cpuFontHandles: RegisteredFontHandles | null = null
let gpuFontHandles: RegisteredFontHandles | null = null

// SINGLE-FLIGHT init per module: the glue's idempotency guard (`if (wasm !==
// undefined) return`) is race-able — overlapping FIRST init() calls (concurrent
// pane builds at worker boot) each instantiate the module and the second
// CLOBBERS the glue's module-level `wasm` binding. Engines built against the
// first instance then dereference into the second instance's memory: the
// worker's `memory access out of bounds` crashes, and module state (the font
// registry, the heap measurement) splits across instances. Memoizing the
// PROMISE guarantees one instance per module for the worker's lifetime.
let cpuInitPromise: ReturnType<typeof init> | null = null
let gpuInitPromise: ReturnType<typeof gpuInit> | null = null

function registerWorkerFonts(
  register: (bytes: Uint8Array) => number,
  fonts: WorkerResidentFonts
): RegisteredFontHandles {
  return {
    primary: register(fonts.primary),
    fallbacks: fonts.fallbacks.map((face) => register(face)),
    emoji: fonts.emoji ? register(fonts.emoji) : null,
    symbol: fonts.symbol ? register(fonts.symbol) : null
  }
}

/** The read + command surface BOTH engines expose identically; the worker terminal
 *  uses only this. `search` (arity differs) + render/process (encoding differs) are
 *  normalized on EngineHandle, not here. */
export type WorkerEngine = Pick<
  AtermTerminal,
  | 'cursor_x'
  | 'cursor_y'
  | 'cursor_style'
  | 'cursor_color'
  | 'cell_width'
  | 'cell_height'
  | 'base_y'
  | 'display_offset'
  | 'display_origin_absolute'
  | 'is_alt_screen'
  | 'bracketed_paste_mode'
  | 'is_mouse_tracking'
  | 'mouse_wants_motion'
  | 'mouse_wants_any_motion'
  | 'is_focus_event_mode'
  | 'is_color_scheme_updates_mode'
  | 'is_app_cursor_mode'
  | 'is_alternate_scroll'
  | 'keyboard_mode_bits'
  | 'row_text'
  | 'row_len'
  | 'row_is_wrapped'
  | 'cell_text'
  | 'cell_is_wide'
  | 'selection_text'
  | 'selection_range'
  | 'selection_start'
  | 'selection_extend'
  | 'selection_finish'
  | 'selection_clear'
  | 'selection_word'
  | 'selection_line'
  | 'link_at'
  | 'scroll_lines'
  | 'scroll_to_bottom'
  | 'scroll_to_top'
  | 'scroll_search_line_into_view'
  | 'search_display_origin'
  | 'serialize'
  | 'serialize_scrollback'
  | 'drain_bell'
  | 'take_osc_events'
  | 'take_response'
  | 'title'
  | 'resize'
  | 'set_px'
  | 'set_line_height'
  | 'set_ligatures'
  | 'set_scrollback_limit'
  | 'set_default_cursor_style'
  | 'set_color_scheme'
  | 'set_theme'
  | 'set_default_foreground'
  | 'set_default_background'
  | 'set_palette_color'
  | 'set_selection_fg'
  | 'set_selection_inactive'
  | 'set_selection_inactive_bg'
  | 'set_cursor_blink_phase'
  | 'set_cursor_hollow'
  | 'advance_effects'
  | 'is_effects_active'
  | 'effects_next_deadline_ms'
  | 'set_effects_focused'
  | 'set_sparkle_words_enabled'
  | 'set_sparkle_classes'
  | 'set_sparkle_reduced_motion'
  | 'set_cursor_glow'
  | 'set_fallback_font'
  | 'add_fallback_font'
  | 'set_emoji_font'
  | 'set_symbol_font'
  | 'set_primary_font'
  | 'set_bold_font'
  | 'set_cell_pixel_size'
  | 'authorize_clipboard_write'
  | 'revoke_clipboard_write'
  | 'authorize_notifications'
  | 'take_notifications'
  | 'encode_mouse_press'
  | 'encode_mouse_release'
  | 'encode_mouse_motion'
  | 'encode_mouse_wheel'
  | 'free'
>

/** The per-pane engine + the normalized hot-path ops the worker terminal drives. */
export type EngineHandle = {
  kind: 'cpu' | 'gpu'
  engine: WorkerEngine
  /** Feed bytes (CPU: process_str; GPU: process(encode)). */
  process: (data: string) => void
  /** Render the current grid to the OffscreenCanvas (CPU: rasterize→2d blit; GPU:
   *  WebGL2 present, no readback). */
  render: () => void
  /** Device-pixel framebuffer size after the last render. */
  framebuffer: () => { width: number; height: number }
  /** Run the engine search; both CPU and GPU honor isRegex (3-arg parity). */
  search: (query: string, caseSensitive: boolean, isRegex: boolean) => Uint32Array
  dispose: () => void
}

/** Per-pane construction params the worker keeps so a GPU→CPU fallback can rebuild on
 *  the same canvas (it was transferred and can't be re-sent). Fonts are a REFERENCE
 *  to the worker-resident faces, never a per-pane copy. */
export type StoredInit = {
  fonts: WorkerResidentFonts
  rows: number
  cols: number
  fontPx: number
  lineHeight: number
  themeColors: AtermThemeColors
}

/** Font + theme seeding both engines share; byte-for-byte the main-thread drawers'
 *  setup so the worker engine matches what the main path would have produced. */
type SeedTarget = Pick<
  AtermTerminal,
  | 'cell_width'
  | 'cell_height'
  | 'set_fallback_font_registered'
  | 'add_fallback_font_registered'
  | 'set_emoji_font_registered'
  | 'set_symbol_font_registered'
  | 'set_palette_color'
  | 'set_selection_fg'
  | 'set_selection_inactive_bg'
  | 'set_default_foreground'
  | 'set_default_background'
  | 'set_cell_pixel_size'
  | 'set_line_height'
>

function seedEngine(t: SeedTarget, p: StoredInit, h: RegisteredFontHandles): void {
  // Handle-based injection: the module registered the faces once; per-pane seeds
  // pass 4-byte handles so no blob is re-marshaled per pane. CJK first RESETS the
  // chain to it; the rest append (parity with the main path). Each injection is
  // fault-tolerant (matches inject-terminal-fallback-fonts): parsing happens at
  // set-time, so an unparseable/unsupported OS face still throws a catchable JS
  // error here — swallow it rather than let one bad face abort the whole worker
  // engine build (the engine still renders Latin + whatever faces did parse).
  if (h.fallbacks.length > 0) {
    try {
      t.set_fallback_font_registered(h.fallbacks[0])
    } catch {
      /* unparseable CJK face — keep going */
    }
    for (let i = 1; i < h.fallbacks.length; i++) {
      try {
        t.add_fallback_font_registered(h.fallbacks[i])
      } catch {
        /* unparseable chain face — skip it */
      }
    }
  }
  // Colour-emoji face AFTER the monochrome fallback chain (parity with the in-process
  // inject-terminal-fallback-fonts ordering) so emoji render in colour, not tofu.
  if (h.emoji != null) {
    try {
      t.set_emoji_font_registered(h.emoji)
    } catch {
      /* unparseable emoji face — keep going */
    }
  }
  // Monochrome symbol tier AFTER emoji (parity with inject-terminal-fallback-fonts /
  // the native engine) so media/technical symbols get a real glyph, not tofu.
  if (h.symbol != null) {
    try {
      t.set_symbol_font_registered(h.symbol)
    } catch {
      /* unparseable symbol face */
    }
  }
  // Apply the user's line-height before metrics are read so the grid is sized to the
  // real cell box from frame 1.
  t.set_line_height(p.lineHeight)
  seedAtermPalette(t, p.themeColors)
  t.set_selection_fg(p.themeColors.selectionForeground ?? undefined)
  t.set_selection_inactive_bg(p.themeColors.selectionInactive ?? undefined)
  seedAtermReplyDefaults(t, p.themeColors, t.cell_width, t.cell_height)
}

// The wasm modules' linear memories (one per module; all engines of a kind share
// it — fonts intern module-wide). Exposed so the worker's state message can report
// the true wasm footprint (the E1 font-dedup gate measures marginal heap per pane;
// process RSS cannot resolve that signal against GC noise).
let cpuWasmMemory: WebAssembly.Memory | null = null
let gpuWasmMemory: WebAssembly.Memory | null = null

export function workerWasmHeapBytes(): number {
  return (cpuWasmMemory?.buffer.byteLength ?? 0) + (gpuWasmMemory?.buffer.byteLength ?? 0)
}

// TEMP DIAGNOSTIC
/** CPU engine: rasterize → zero-copy 2d blit (identical to the main-thread painter). */
export async function buildCpuEngine(
  p: StoredInit,
  canvas: OffscreenCanvas
): Promise<EngineHandle> {
  const out = await (cpuInitPromise ??= init(wasmUrl))
  const memory = out.memory
  cpuWasmMemory = memory
  // Register the worker-resident faces ONCE per module; this and every later
  // pane build constructs + seeds from 4-byte handles (no per-pane blob marshal).
  cpuFontHandles ??= registerWorkerFonts(registerCpuFont, p.fonts)
  const t = AtermTerminal.new_registered(
    p.rows,
    p.cols,
    cpuFontHandles.primary,
    p.fontPx,
    p.themeColors.fg,
    p.themeColors.bg,
    p.themeColors.cursor,
    p.themeColors.selection
  )
  seedEngine(t, p, cpuFontHandles)
  const canvasCtx = canvas.getContext('2d')
  if (!canvasCtx) {
    t.free()
    throw new Error('OffscreenCanvas 2d context unavailable')
  }
  let width = 0
  let height = 0
  const render = (): void => {
    t.render()
    width = t.width
    height = t.height
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width
      canvas.height = height
    }
    const view = new Uint8ClampedArray(memory.buffer, t.rgba_ptr(), width * height * 4)
    canvasCtx.putImageData(new ImageData(view, width, height), 0, 0)
  }
  return {
    kind: 'cpu',
    engine: t,
    process: (data) => t.process_str(data),
    render,
    framebuffer: () => ({ width, height }),
    search: (q, cs, regex) => t.search(q, cs, regex),
    dispose: () => {
      try {
        t.free()
      } catch {
        /* ignore */
      }
    }
  }
}

// GPU process has no string entry; one encoder avoids a per-chunk alloc.
const textEncoder = new TextEncoder()

/** GPU engine: WebGL2 present straight to the swapchain — NO rgba blit. `init_offscreen`
 *  is async and MUST finish before any render; it throws if WebGL is unavailable in the
 *  worker → caller posts an init error so the main side falls back to a CPU worker. */
export async function buildGpuEngine(
  p: StoredInit,
  canvas: OffscreenCanvas
): Promise<EngineHandle> {
  const gpuOut = await (gpuInitPromise ??= gpuInit(gpuWasmUrl))
  gpuWasmMemory = gpuOut.memory
  const rows = p.rows
  const cols = p.cols
  // The GPU module has its own linear memory — its own one-time registration.
  gpuFontHandles ??= registerWorkerFonts(registerGpuFont, p.fonts)
  const t = AtermGpuTerminal.new_registered(
    p.rows,
    p.cols,
    gpuFontHandles.primary,
    p.fontPx,
    p.themeColors.fg,
    p.themeColors.bg,
    p.themeColors.cursor,
    p.themeColors.selection
  )
  // Seed BEFORE init_offscreen so the engine re-applies fonts/theme to the GPU face it
  // builds there (matches aterm-gpu-drawer's seed-then-init ordering).
  seedEngine(t as unknown as SeedTarget, p, gpuFontHandles)
  try {
    await t.init_offscreen(canvas)
  } catch (err) {
    try {
      t.free()
    } catch {
      /* ignore */
    }
    throw err
  }
  const engine = t as unknown as WorkerEngine
  return {
    kind: 'gpu',
    engine,
    process: (data) => t.process(textEncoder.encode(data)),
    render: () => t.render(),
    // The presented swapchain canvas carries the framebuffer size; fall back to grid-
    // derived device px before the first present sizes it.
    framebuffer: () => ({
      width: canvas.width || Math.round(cols * engine.cell_width),
      height: canvas.height || Math.round(rows * engine.cell_height)
    }),
    // GPU search now forwards isRegex (parity with the CPU binding after the aterm
    // 3-arg widening), so regex search works on the default GPU worker path.
    search: (q, cs, regex) => t.search(q, cs, regex),
    dispose: () => {
      try {
        t.free()
      } catch {
        /* ignore */
      }
    }
  }
}
