import { register_font as registerCpuFont, type AtermTerminal } from './aterm_wasm.js'
import { register_font as registerGpuFont } from './aterm_gpu_web.js'

// LAZY OS fallback fonts for the IN-PROCESS aterm paths (E1): the engine reports
// which injectable face CLASS a `.notdef` miss needed (take_missing_font_classes,
// drained after each drawn frame) and only THAT class is fetched from the main
// process and injected — an ASCII-only session never pays the multi-hundred-MB
// CJK/emoji payload (~206MB per wasm module; Apple Color Emoji alone is 183MB).
// JetBrains Mono still covers Latin when a class is absent or fails.
//
// Injection is HANDLE-based: each wasm module registers a face ONCE via
// `register_font` (one marshal per blob per module — CPU and GPU are separate
// modules with separate linear memories) and every terminal after that applies
// 4-byte handles, so the faces are never re-copied across the JS/wasm boundary
// per pane (transient copies fragment the never-shrinking linear memory into a
// per-pane high-water ratchet).

// The injection surface — identical signatures on both modules' classes.
type FallbackFontInjectable = Pick<
  AtermTerminal,
  | 'set_fallback_font_registered'
  | 'add_fallback_font_registered'
  | 'set_emoji_font_registered'
  | 'set_symbol_font_registered'
  | 'take_missing_font_classes'
>

// The engine's MISSING_FONT_CLASS_* bits (aterm-render).
const MISSING_TEXT = 1
const MISSING_EMOJI = 2

type TextClassHandles = {
  user: number[]
  cjk: number | null
  chain: number[]
  symbol: number | null
}

function moduleRegister(engine: 'cpu' | 'gpu'): (bytes: Uint8Array) => number {
  return engine === 'cpu' ? registerCpuFont : registerGpuFont
}

// Per-module, per-class registration memos: the classed IPC read happens once per
// renderer per class (main also caches it), the registration once per wasm module.
const textHandles: Record<'cpu' | 'gpu', Promise<TextClassHandles> | null> = {
  cpu: null,
  gpu: null
}
const emojiHandles: Record<'cpu' | 'gpu', Promise<number | null> | null> = {
  cpu: null,
  gpu: null
}

async function registerTextClass(engine: 'cpu' | 'gpu'): Promise<TextClassHandles> {
  const register = moduleRegister(engine)
  const { user, cjk, symbol, chain } = await window.api.fonts.getTerminalFallbackFonts(['text'])
  return {
    user: (user ?? []).map((face) => register(new Uint8Array(face.bytes))),
    cjk: cjk ? register(new Uint8Array(cjk.bytes)) : null,
    chain: (chain ?? []).map((face) => register(new Uint8Array(face.bytes))),
    symbol: symbol ? register(new Uint8Array(symbol)) : null
  }
}

async function registerEmojiClass(engine: 'cpu' | 'gpu'): Promise<number | null> {
  const { emoji } = await window.api.fonts.getTerminalFallbackFonts(['emoji'])
  return emoji ? moduleRegister(engine)(new Uint8Array(emoji)) : null
}

/** Apply the monochrome 'text' class. Order matters: the user-configured stack
 *  leads (set RESETS the chain to its first face; the rest APPEND), then the
 *  locale-aware CJK face (set when no user stack — exactly the prior behavior —
 *  else appended), then each chain face (Arabic/Hebrew/Devanagari/Thai/broad-
 *  coverage) so a glyph an earlier face lacks falls through; the symbol tier
 *  (⏸⏹⏺) rides the same class. Tolerant per face: parsing happens at set-time,
 *  so an unparseable face throws a catchable error here — Latin + the other
 *  faces still render. */
function applyTextClass(term: FallbackFontInjectable, h: TextClassHandles): void {
  // Why: track whether a set landed so a face that fails to parse never leaves
  // the chain un-reset (the next face takes the set slot instead).
  let chainStarted = false
  const applyFallbackFace = (handle: number): void => {
    try {
      if (chainStarted) {
        term.add_fallback_font_registered(handle)
      } else {
        term.set_fallback_font_registered(handle)
        chainStarted = true
      }
    } catch {
      // Unparseable face — skip it; later faces still apply.
    }
  }
  for (const face of h.user) {
    applyFallbackFace(face)
  }
  if (h.cjk != null) {
    applyFallbackFace(h.cjk)
  }
  for (const face of h.chain) {
    applyFallbackFace(face)
  }
  if (h.symbol != null) {
    try {
      term.set_symbol_font_registered(h.symbol)
    } catch {
      // Unparseable symbol face — keep going.
    }
  }
}

export type LazyFallbackFontInjector = {
  /** Post-frame poll: drain the engine's missing-font class bits and inject any
   *  class not yet requested. Cheap — one wasm call returning a u8. */
  poll: () => void
}

/**
 * Lazy fallback-font injection for one in-process terminal. Call `poll()` after
 * each drawn frame; a reported miss fetches + registers ONLY that face class and
 * applies it — the engine's installers clear the per-char memos and force a full
 * repaint, so previously-`.notdef` cells resolve on the redraw `requestRedraw`
 * kicks. Classes are latched per terminal (the engine re-fires a bit when the
 * injected faces still miss a char; re-requesting would loop). For the GPU
 * terminal the registered setters also fill its retention slots, so faces
 * survive a later `init()` rebuild.
 */
export function createLazyFallbackFontInjector(opts: {
  term: FallbackFontInjectable
  engine: 'cpu' | 'gpu'
  /** Kick one redraw after a class lands (an idle pane has no other tick). */
  requestRedraw: () => void
  isDisposed?: () => boolean
}): LazyFallbackFontInjector {
  let requested = 0
  const inject = async (cls: 'text' | 'emoji'): Promise<void> => {
    try {
      if (cls === 'text') {
        const handles = await (textHandles[opts.engine] ??= registerTextClass(opts.engine))
        if (opts.isDisposed?.()) {
          return
        }
        applyTextClass(opts.term, handles)
      } else {
        const emoji = await (emojiHandles[opts.engine] ??= registerEmojiClass(opts.engine))
        if (opts.isDisposed?.() || emoji == null) {
          return
        }
        try {
          opts.term.set_emoji_font_registered(emoji)
        } catch {
          // Unparseable emoji face — keep going.
        }
      }
      opts.requestRedraw()
    } catch (err) {
      // Latched (per terminal): rendering stays Latin-correct with `.notdef` for
      // the missed scripts, same as a host that has no such font.
      console.warn(`[aterm] lazy fallback-font inject failed (class=${cls})`, err)
    }
  }
  return {
    poll: () => {
      let bits = 0
      try {
        bits = opts.term.take_missing_font_classes()
      } catch {
        return
      }
      const fresh = bits & ~requested
      if (!fresh) {
        return
      }
      requested |= fresh
      if (fresh & MISSING_TEXT) {
        void inject('text')
      }
      if (fresh & MISSING_EMOJI) {
        void inject('emoji')
      }
    }
  }
}
