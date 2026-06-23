import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermGpuTerminal } from './aterm_gpu_web.js'

// The aterm renderer rasterizes glyphs itself from injected fonts and ships only
// JetBrains Mono, so CJK + emoji render as .notdef tofu. The main process reads
// the local OS fallback fonts (CJK + colour emoji) and hands the bytes over IPC;
// here we push them into the engine so its existing fallback/colour paths render
// real glyphs. JetBrains Mono still covers Latin if these are absent or fail.

// The minimal engine surface both the CPU and GPU terminals expose for font
// injection (both bindings carry set_fallback_font / set_emoji_font).
type FallbackFontInjectable = Pick<AtermTerminal, 'set_fallback_font' | 'set_emoji_font'>

// Fetched once per renderer and shared across every pane's terminal — the OS
// fonts are immutable, large, and the IPC reads them off disk.
let fallbackFontsPromise: Promise<{ cjk?: Uint8Array; emoji?: Uint8Array }> | null = null

function loadFallbackFonts(): Promise<{ cjk?: Uint8Array; emoji?: Uint8Array }> {
  fallbackFontsPromise ??= window.api.fonts
    .getTerminalFallbackFonts()
    .catch(() => ({}) as { cjk?: Uint8Array; emoji?: Uint8Array })
  return fallbackFontsPromise
}

/** Inject the local OS CJK + colour-emoji fallback faces into a freshly built
 *  aterm terminal so non-Latin scripts render real glyphs. Tolerant: a missing
 *  category or a parse failure is swallowed (Latin still renders). For the GPU
 *  terminal, call this BEFORE `init()` so the engine re-applies the bytes to the
 *  face it builds there (it also accepts injection after init). */
export async function injectTerminalFallbackFonts(
  term: FallbackFontInjectable | AtermGpuTerminal
): Promise<void> {
  const { cjk, emoji } = await loadFallbackFonts()
  if (cjk) {
    try {
      term.set_fallback_font(new Uint8Array(cjk))
    } catch {
      // Unparseable CJK face — keep going; Latin still renders.
    }
  }
  if (emoji) {
    try {
      term.set_emoji_font(new Uint8Array(emoji))
    } catch {
      // Unparseable emoji face — keep going.
    }
  }
}
