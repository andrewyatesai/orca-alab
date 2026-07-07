import { register_font as registerCpuFont, type AtermTerminal } from './aterm_wasm.js'
import { register_font as registerGpuFont } from './aterm_gpu_web.js'

// The aterm renderer rasterizes glyphs itself from injected fonts and ships only
// JetBrains Mono, so non-Latin scripts render as .notdef tofu. The main process
// reads the local OS fallback fonts (a locale-aware CJK face, a colour-emoji face,
// and an ordered chain of broad/complex-script faces) and hands the bytes over IPC;
// here we push them into the engine so its existing fallback/colour paths render
// real glyphs. JetBrains Mono still covers Latin if these are absent or fail.
//
// Injection is HANDLE-based: each wasm module registers the faces ONCE via
// `register_font` (one marshal per blob per module — CPU and GPU are separate
// modules with separate linear memories) and every pane after that seeds from
// 4-byte handles, so the ~100–400MB faces are never re-copied across the
// JS/wasm boundary per pane (the transient copies fragment the never-shrinking
// linear memory into a per-pane high-water ratchet).

// The handle-injection surface — identical signatures on both modules' classes.
type FallbackFontInjectable = Pick<
  AtermTerminal,
  | 'set_fallback_font_registered'
  | 'add_fallback_font_registered'
  | 'set_emoji_font_registered'
  | 'set_symbol_font_registered'
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

// One registration per wasm module. Safe to run lazily from an inject: the
// pane's module is initialized by the time a live terminal reaches us.
let cpuHandlesPromise: Promise<FallbackFontHandles> | null = null
let gpuHandlesPromise: Promise<FallbackFontHandles> | null = null

async function registerFallbackFonts(
  register: (bytes: Uint8Array) => number
): Promise<FallbackFontHandles> {
  const { cjk, emoji, symbol, chain } = await loadFallbackFonts()
  return {
    cjk: cjk ? register(new Uint8Array(cjk.bytes)) : null,
    chain: (chain ?? []).map((face) => register(new Uint8Array(face.bytes))),
    emoji: emoji ? register(new Uint8Array(emoji)) : null,
    symbol: symbol ? register(new Uint8Array(symbol)) : null
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
 *  after init). `engine` picks the wasm module whose registry the handles live
 *  in. */
export async function injectTerminalFallbackFonts(
  term: FallbackFontInjectable,
  engine: 'cpu' | 'gpu'
): Promise<void> {
  const handles = await (engine === 'cpu'
    ? (cpuHandlesPromise ??= registerFallbackFonts(registerCpuFont))
    : (gpuHandlesPromise ??= registerFallbackFonts(registerGpuFont)))
  if (handles.cjk != null) {
    try {
      // RESETS the fallback chain to this single face, so it must come first.
      term.set_fallback_font_registered(handles.cjk)
    } catch {
      // Unparseable CJK face — keep going; the chain + Latin still render.
    }
  }
  for (const face of handles.chain) {
    try {
      term.add_fallback_font_registered(face)
    } catch {
      // Unparseable chain face — skip it; later faces still apply.
    }
  }
  if (handles.emoji != null) {
    try {
      term.set_emoji_font_registered(handles.emoji)
    } catch {
      // Unparseable emoji face — keep going.
    }
  }
  // Symbol tier AFTER emoji (parity with native): the monochrome media/technical
  // glyphs (⏸⏹⏺) the primary + emoji faces miss.
  if (handles.symbol != null) {
    try {
      term.set_symbol_font_registered(handles.symbol)
    } catch {
      // Unparseable symbol face — keep going.
    }
  }
}
