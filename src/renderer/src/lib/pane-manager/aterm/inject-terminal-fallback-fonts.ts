import { register_font as registerCpuFont, type AtermTerminal } from './aterm_wasm.js'
import type { AtermGpuTerminal } from './aterm_gpu_web.js'

// The aterm renderer rasterizes glyphs itself from injected fonts and ships only
// JetBrains Mono, so non-Latin scripts render as .notdef tofu. The main process
// reads the local OS fallback fonts (a locale-aware CJK face, a colour-emoji face,
// and an ordered chain of broad/complex-script faces) and hands the bytes over IPC;
// here we push them into the engine so its existing fallback/colour paths render
// real glyphs. JetBrains Mono still covers Latin if these are absent or fail.
//
// CPU-module injection is HANDLE-based: the module registers the faces ONCE via
// `register_font` (one marshal per blob) and every pane after that seeds from
// 4-byte handles — the ~100–400MB faces are never re-copied across the JS/wasm
// boundary per pane (the transient copies fragment the linear memory into a
// per-pane high-water ratchet; wasm memory never shrinks). The GPU module keeps
// byte-based injection: its registered twins intermittently trap `memory access
// out of bounds` (an init interplay under investigation upstream).

// The CPU module's handle-injection surface.
type CpuFallbackFontInjectable = Pick<
  AtermTerminal,
  | 'set_fallback_font_registered'
  | 'add_fallback_font_registered'
  | 'set_emoji_font_registered'
  | 'set_symbol_font_registered'
>

// The GPU terminal's byte-injection surface.
type GpuFallbackFontInjectable = Pick<
  AtermGpuTerminal,
  'set_fallback_font' | 'add_fallback_font' | 'set_emoji_font' | 'set_symbol_font'
>

type TerminalFallbackFonts = Awaited<ReturnType<typeof window.api.fonts.getTerminalFallbackFonts>>

type FallbackFontHandles = {
  cjk: number | null
  chain: number[]
  emoji: number | null
  symbol: number | null
}

// Fetched once per renderer and shared across every pane's terminal — the OS
// fonts are immutable, large, and the IPC reads them off disk.
let fallbackFontsPromise: Promise<TerminalFallbackFonts> | null = null

function loadFallbackFonts(): Promise<TerminalFallbackFonts> {
  fallbackFontsPromise ??= window.api.fonts
    .getTerminalFallbackFonts()
    .catch(() => ({ chain: [] }) as TerminalFallbackFonts)
  return fallbackFontsPromise
}

// One registration for the CPU module. Safe to run lazily from an inject: the
// module is initialized by the time a live terminal reaches us.
let cpuHandlesPromise: Promise<FallbackFontHandles> | null = null

async function registerCpuFallbackFonts(): Promise<FallbackFontHandles> {
  const { cjk, emoji, symbol, chain } = await loadFallbackFonts()
  return {
    cjk: cjk ? registerCpuFont(new Uint8Array(cjk.bytes)) : null,
    chain: (chain ?? []).map((face) => registerCpuFont(new Uint8Array(face.bytes))),
    emoji: emoji ? registerCpuFont(new Uint8Array(emoji)) : null,
    symbol: symbol ? registerCpuFont(new Uint8Array(symbol)) : null
  }
}

/** Inject the local OS fallback faces into a freshly built aterm terminal so
 *  non-Latin scripts render real glyphs. Order matters: the locale-aware CJK face
 *  is the primary fallback (the set RESETS the chain to it), then each chain face
 *  (Arabic/Hebrew/Devanagari/Thai/broad-coverage) is APPENDED so a glyph the CJK
 *  face lacks falls through to a later face. Tolerant: a missing category or
 *  parse failure is swallowed per face (parsing happens at set-time; Latin still
 *  renders). For the GPU terminal, call this BEFORE `init()` so the engine
 *  re-applies the faces to the one it builds there (it also accepts injection
 *  after init). `engine` picks the module path: CPU seeds by registry handle,
 *  GPU by bytes (see the header note). */
export async function injectTerminalFallbackFonts(
  term: CpuFallbackFontInjectable | GpuFallbackFontInjectable,
  engine: 'cpu' | 'gpu'
): Promise<void> {
  if (engine === 'cpu') {
    const t = term as CpuFallbackFontInjectable
    const handles = await (cpuHandlesPromise ??= registerCpuFallbackFonts())
    if (handles.cjk != null) {
      try {
        // RESETS the fallback chain to this single face, so it must come first.
        t.set_fallback_font_registered(handles.cjk)
      } catch {
        // Unparseable CJK face — keep going; the chain + Latin still render.
      }
    }
    for (const face of handles.chain) {
      try {
        t.add_fallback_font_registered(face)
      } catch {
        // Unparseable chain face — skip it; later faces still apply.
      }
    }
    if (handles.emoji != null) {
      try {
        t.set_emoji_font_registered(handles.emoji)
      } catch {
        // Unparseable emoji face — keep going.
      }
    }
    // Symbol tier AFTER emoji (parity with native): the monochrome
    // media/technical glyphs (⏸⏹⏺) the primary + emoji faces miss.
    if (handles.symbol != null) {
      try {
        t.set_symbol_font_registered(handles.symbol)
      } catch {
        // Unparseable symbol face — keep going.
      }
    }
    return
  }
  const t = term as GpuFallbackFontInjectable
  const { cjk, emoji, symbol, chain } = await loadFallbackFonts()
  if (cjk) {
    try {
      // RESETS the fallback chain to this single face, so it must come first.
      t.set_fallback_font(new Uint8Array(cjk.bytes))
    } catch {
      // Unparseable CJK face — keep going; the chain + Latin still render.
    }
  }
  for (const face of chain ?? []) {
    try {
      t.add_fallback_font(new Uint8Array(face.bytes))
    } catch {
      // Unparseable chain face — skip it; later faces still apply.
    }
  }
  if (emoji) {
    try {
      t.set_emoji_font(new Uint8Array(emoji))
    } catch {
      // Unparseable emoji face — keep going.
    }
  }
  // Symbol tier AFTER emoji (parity with native): the monochrome media/technical
  // glyphs (⏸⏹⏺) the primary + emoji faces miss.
  if (symbol) {
    try {
      t.set_symbol_font(new Uint8Array(symbol))
    } catch {
      // Unparseable symbol face — keep going.
    }
  }
}
