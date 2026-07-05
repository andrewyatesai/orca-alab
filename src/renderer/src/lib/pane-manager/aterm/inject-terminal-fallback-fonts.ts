import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermGpuTerminal } from './aterm_gpu_web.js'

// The aterm renderer rasterizes glyphs itself from injected fonts and ships only
// JetBrains Mono, so non-Latin scripts render as .notdef tofu. The main process
// reads the local OS fallback fonts (a locale-aware CJK face, a colour-emoji face,
// and an ordered chain of broad/complex-script faces) and hands the bytes over IPC;
// here we push them into the engine so its existing fallback/colour paths render
// real glyphs. JetBrains Mono still covers Latin if these are absent or fail.

// The minimal engine surface both the CPU and GPU terminals expose for font
// injection: set the primary fallback (CJK), append chain faces, set the emoji face,
// set the monochrome symbol face.
type FallbackFontInjectable = Pick<
  AtermTerminal,
  'set_fallback_font' | 'add_fallback_font' | 'set_emoji_font' | 'set_symbol_font'
>

type TerminalFallbackFonts = Awaited<ReturnType<typeof window.api.fonts.getTerminalFallbackFonts>>

// Fetched once per renderer and shared across every pane's terminal — the OS
// fonts are immutable, large, and the IPC reads them off disk.
let fallbackFontsPromise: Promise<TerminalFallbackFonts> | null = null

function loadFallbackFonts(): Promise<TerminalFallbackFonts> {
  fallbackFontsPromise ??= window.api.fonts
    .getTerminalFallbackFonts()
    .catch(() => ({ chain: [] }) as TerminalFallbackFonts)
  return fallbackFontsPromise
}

/** Inject the local OS fallback faces into a freshly built aterm terminal so
 *  non-Latin scripts render real glyphs. Order matters: the locale-aware CJK face
 *  is the primary fallback (`set_fallback_font` RESETS the chain to it), then each
 *  chain face (Arabic/Hebrew/Devanagari/Thai/broad-coverage) is APPENDED via
 *  `add_fallback_font` so a glyph the CJK face lacks falls through to a later face.
 *  Tolerant: a missing category or parse failure is swallowed (Latin still
 *  renders). For the GPU terminal, call this BEFORE `init()` so the engine
 *  re-applies the bytes to the face it builds there (it also accepts injection
 *  after init). */
export async function injectTerminalFallbackFonts(
  term: FallbackFontInjectable | AtermGpuTerminal
): Promise<void> {
  const { cjk, emoji, symbol, chain } = await loadFallbackFonts()
  if (cjk) {
    try {
      // RESETS the fallback chain to this single face, so it must come first.
      term.set_fallback_font(new Uint8Array(cjk.bytes))
    } catch {
      // Unparseable CJK face — keep going; the chain + Latin still render.
    }
  }
  for (const face of chain ?? []) {
    try {
      term.add_fallback_font(new Uint8Array(face.bytes))
    } catch {
      // Unparseable chain face — skip it; later faces still apply.
    }
  }
  if (emoji) {
    try {
      term.set_emoji_font(new Uint8Array(emoji))
    } catch {
      // Unparseable emoji face — keep going.
    }
  }
  // Symbol tier AFTER emoji (parity with native): the monochrome media/technical
  // glyphs (⏸⏹⏺) the primary + emoji faces miss.
  if (symbol) {
    try {
      term.set_symbol_font(new Uint8Array(symbol))
    } catch {
      // Unparseable symbol face — keep going.
    }
  }
}
