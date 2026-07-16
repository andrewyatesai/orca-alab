// The WORKER-SCOPED font-delivery contract (E1 lazy fonts), split from the main
// render-worker protocol to keep that file under the line budget. Fonts are sent
// ONCE per worker generation and kept resident so pane inits never re-ship the
// multi-MB faces; fallback CLASSES stream in later, on demand.

/** The font faces every engine in this worker seeds from, sent ONCE per worker
 *  generation BEFORE the first pane init. E1 LAZY FONTS: at boot this carries the
 *  ~264KB primary ONLY — the multi-hundred-MB OS fallback classes arrive later via
 *  'fontClass', and only when an engine actually reports a glyph miss for them
 *  (the 'missingFontClasses' worker event). The worker keeps faces resident so
 *  per-pane inits carry no font bytes at all; the engine-side content-keyed intern
 *  registry then dedupes the bytes across engines within each wasm module. */
export type AtermWorkerFonts = {
  type: 'fonts'
  /** JetBrains-Mono bytes — the engines' built-in primary face. */
  primary: Uint8Array
  /** Optional CJK + non-Latin fallback faces (same bytes the main path injects via
   *  set_fallback_font/add_fallback_font — the MONOCHROME glyph path). */
  fallbacks: Uint8Array[]
  /** Optional OS colour-emoji face (set_emoji_font — the sbix/COLR colour path). Kept
   *  separate from `fallbacks` because the fallback chain renders monochrome. */
  emoji?: Uint8Array
  /** Optional monochrome SYMBOL face (set_symbol_font — the media/technical-glyph tier,
   *  ⏸⏹⏺). Consulted after the fallback chain misses; parity with the native engine. */
  symbol?: Uint8Array
}

/** The injectable font classes the aterm engine reports misses for (mirrors the
 *  engine's MISSING_FONT_CLASS_TEXT/EMOJI bits): 'text' = the monochrome faces
 *  (CJK + script chain + symbol), 'emoji' = the colour emoji face. */
export type AtermFontClass = 'text' | 'emoji'

/** A lazily delivered font CLASS (E1): posted by the manager after the worker's
 *  'missingFontClasses' event, once per class per generation. The worker registers
 *  the faces on every live wasm module and applies them to every live engine —
 *  previously `.notdef` cells re-render through the new faces on the next frame. */
export type AtermWorkerFontClass = {
  type: 'fontClass'
  class: AtermFontClass
  /** 'text': CJK-first + script chain (set_fallback_font, then add_fallback_font). */
  fallbacks?: Uint8Array[]
  /** 'text': the monochrome symbol tier (set_symbol_font). */
  symbol?: Uint8Array
  /** 'emoji': the colour face (set_emoji_font). */
  emoji?: Uint8Array
}
