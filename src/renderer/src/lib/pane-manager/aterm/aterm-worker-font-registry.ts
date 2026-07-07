// The SHARED render worker's font registry (E1 lazy fonts): tracks which resident
// faces each wasm module (aterm_wasm CPU / aterm_gpu_web GPU — separate linear
// memories) has registered, registers new blobs incrementally as font classes are
// delivered, and applies class faces to engines by 4-byte HANDLE — the
// ~100–400MB faces are never re-copied across the JS/wasm boundary per pane
// (transient copies fragmented the never-shrinking linear memory into a
// ~183MB/pane high-water ratchet).

import { register_font as registerCpuFont, type AtermTerminal } from './aterm_wasm.js'
import { register_font as registerGpuFont } from './aterm_gpu_web.js'
import type { AtermWorkerFonts } from './aterm-render-worker-protocol'

/** The worker-resident font faces. E1 LAZY FONTS: at worker boot this holds the
 *  ~264KB primary only; the fallback/emoji/symbol classes are added when the
 *  manager delivers them on an engine glyph-miss signal. */
export type WorkerResidentFonts = Pick<
  AtermWorkerFonts,
  'primary' | 'fallbacks' | 'emoji' | 'symbol'
>

/** Handles into a wasm module's font registry (module-scoped: each module has
 *  its own linear memory, so CPU and GPU register independently). */
export type RegisteredFontHandles = {
  primary: number
  fallbacks: number[]
  emoji: number | null
  symbol: number | null
}

/** The font-slot injection surface — identical signatures on both modules. */
type FontFaceTarget = Pick<
  AtermTerminal,
  | 'set_fallback_font_registered'
  | 'add_fallback_font_registered'
  | 'set_emoji_font_registered'
  | 'set_symbol_font_registered'
>

// Once per module per worker generation; late-delivered classes append to the
// same record (registration is incremental — see ensureModuleHandles).
let cpuFontHandles: RegisteredFontHandles | null = null
let gpuFontHandles: RegisteredFontHandles | null = null

/** Register any resident faces this module hasn't registered yet (idempotent by
 *  slot: already-registered blobs are never re-marshaled). Called on every
 *  engine build AND on a late 'fontClass' delivery. */
function ensureModuleHandles(
  handles: RegisteredFontHandles,
  register: (bytes: Uint8Array) => number,
  fonts: WorkerResidentFonts
): RegisteredFontHandles {
  for (let i = handles.fallbacks.length; i < fonts.fallbacks.length; i++) {
    handles.fallbacks.push(register(fonts.fallbacks[i]))
  }
  if (handles.emoji == null && fonts.emoji) {
    handles.emoji = register(fonts.emoji)
  }
  if (handles.symbol == null && fonts.symbol) {
    handles.symbol = register(fonts.symbol)
  }
  return handles
}

/** The module's handle record, creating it (primary registration included) on the
 *  first engine build of that kind and growing it incrementally after. */
export function ensureFontHandles(
  kind: 'cpu' | 'gpu',
  fonts: WorkerResidentFonts
): RegisteredFontHandles {
  const register = kind === 'cpu' ? registerCpuFont : registerGpuFont
  const existing = kind === 'cpu' ? cpuFontHandles : gpuFontHandles
  const handles = ensureModuleHandles(
    existing ?? { primary: register(fonts.primary), fallbacks: [], emoji: null, symbol: null },
    register,
    fonts
  )
  if (kind === 'cpu') {
    cpuFontHandles = handles
  } else {
    gpuFontHandles = handles
  }
  return handles
}

// Handle-based injection: the module registered the faces once; per-engine
// applications pass 4-byte handles so no blob is re-marshaled per pane. CJK
// first RESETS the chain to it; the rest append (parity with the main path).
// Each injection is fault-tolerant (matches inject-terminal-fallback-fonts):
// parsing happens at set-time, so an unparseable/unsupported OS face still
// throws a catchable JS error here — swallow it rather than let one bad face
// abort the engine (it still renders Latin + whatever faces did parse).
export function applyTextFaces(t: FontFaceTarget, h: RegisteredFontHandles): void {
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
  // Monochrome symbol tier (media/technical glyphs ⏸⏹⏺) — same 'text' class:
  // the engine reports both through MISSING_FONT_CLASS_TEXT.
  if (h.symbol != null) {
    try {
      t.set_symbol_font_registered(h.symbol)
    } catch {
      /* unparseable symbol face */
    }
  }
}

export function applyEmojiFace(t: FontFaceTarget, h: RegisteredFontHandles): void {
  if (h.emoji != null) {
    try {
      t.set_emoji_font_registered(h.emoji)
    } catch {
      /* unparseable emoji face — keep going */
    }
  }
}

/** Late 'fontClass' delivery (E1): register any new resident faces on the module
 *  (if it already built engines), then apply the class's faces to one live
 *  engine — its previously-`.notdef` cells re-render on the next frame (the
 *  engine-side installers clear the per-char memos and force a full repaint). */
export function applyResidentFontClass(
  engine: unknown,
  kind: 'cpu' | 'gpu',
  cls: 'text' | 'emoji',
  fonts: WorkerResidentFonts
): void {
  const existing = kind === 'cpu' ? cpuFontHandles : gpuFontHandles
  if (!existing) {
    // No engine of this kind was ever built; its first build registers + seeds
    // the resident faces itself.
    return
  }
  const handles = ensureFontHandles(kind, fonts)
  const target = engine as FontFaceTarget
  if (cls === 'text') {
    applyTextFaces(target, handles)
  } else {
    applyEmojiFace(target, handles)
  }
}
