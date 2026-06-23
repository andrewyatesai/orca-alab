import { readFile } from 'fs/promises'

// The aterm canvas/WebGL renderers rasterize glyphs themselves from injected
// font bytes and ship only JetBrains Mono, so CJK and emoji render as .notdef
// tofu. The renderer can't read the local filesystem, so the main process reads
// the host OS fallback fonts and hands the bytes over IPC. These paths mirror the
// native aterm engine's font-discovery candidates (PingFang/Apple Color Emoji on
// macOS, Noto on Linux, MS YaHei/Segoe UI Emoji on Windows) so the byte format
// (incl. .ttc collections + Apple's sbix color emoji) is one the engine accepts.
//
// This is always the LOCAL host's fonts for LOCAL rendering — even an SSH session
// rasterizes on the local machine, so reading local fonts is correct.

export type TerminalFallbackFonts = {
  cjk?: Uint8Array
  emoji?: Uint8Array
}

// First-existing wins per category. macOS .ttc collections and Apple Color Emoji
// (sbix) are read whole; the engine selects face 0.
const CJK_CANDIDATES: Record<NodeJS.Platform, readonly string[]> = {
  darwin: ['/System/Library/Fonts/PingFang.ttc', '/System/Library/Fonts/Hiragino Sans GB.ttc'],
  win32: ['C:/Windows/Fonts/msyh.ttc', 'C:/Windows/Fonts/simsun.ttc', 'C:/Windows/Fonts/msgothic.ttc'],
  linux: [
    '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf',
    '/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc'
  ]
} as unknown as Record<NodeJS.Platform, readonly string[]>

const EMOJI_CANDIDATES: Record<NodeJS.Platform, readonly string[]> = {
  darwin: ['/System/Library/Fonts/Apple Color Emoji.ttc'],
  win32: ['C:/Windows/Fonts/seguiemj.ttf'],
  linux: [
    '/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf',
    '/usr/share/fonts/noto/NotoColorEmoji.ttf',
    '/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf',
    '/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf'
  ]
} as unknown as Record<NodeJS.Platform, readonly string[]>

let cached: TerminalFallbackFonts | null = null

function candidatesFor(table: Record<NodeJS.Platform, readonly string[]>): readonly string[] {
  return table[process.platform] ?? []
}

// Read the first candidate that exists and parses as readable bytes; a missing or
// unreadable file just moves to the next candidate. Returns undefined when none
// of the platform's candidates are present (the renderer keeps JetBrains Mono).
async function readFirstExisting(candidates: readonly string[]): Promise<Uint8Array | undefined> {
  for (const path of candidates) {
    try {
      const buf = await readFile(path)
      if (buf.length > 0) {
        return new Uint8Array(buf)
      }
    } catch {
      // Missing/unreadable candidate — try the next one.
    }
  }
  return undefined
}

export async function getTerminalFallbackFonts(): Promise<TerminalFallbackFonts> {
  if (cached) {
    return cached
  }
  const [cjk, emoji] = await Promise.all([
    readFirstExisting(candidatesFor(CJK_CANDIDATES)),
    readFirstExisting(candidatesFor(EMOJI_CANDIDATES))
  ])
  cached = { cjk, emoji }
  return cached
}
