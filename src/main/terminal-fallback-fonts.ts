import { readFile } from 'fs/promises'
import { execFile } from 'child_process'
import { app } from 'electron'

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

// Han unification: Chinese/Japanese/Korean share Unicode code points but want
// different glyph SHAPES. A single zh-default fallback face shows Chinese forms to
// Japanese/Korean users (the "wrong-region glyphs" complaint). We pick the CJK face
// by the user's locale: prepend a region-preferred face + steer fontconfig's
// :lang query. Absent region faces fall through to the generic candidates, so this
// never regresses (a missing region face → the prior behaviour).
export type CjkRegion = 'ja' | 'ko' | 'zh-Hant' | 'zh-Hans'

/** Map a BCP-47-ish locale to its CJK region. Defaults to Simplified Chinese (the
 *  prior behaviour) for non-CJK or unknown locales. Pure + exported for testing. */
export function cjkRegionFromLocale(locale: string): CjkRegion {
  const l = locale.toLowerCase()
  if (l.startsWith('ja')) {
    return 'ja'
  }
  if (l.startsWith('ko')) {
    return 'ko'
  }
  if (l.startsWith('zh')) {
    // Traditional for Taiwan/Hong Kong/Macau or an explicit Hant subtag.
    return /(?:^|[-_])(?:tw|hk|mo|hant)\b/.test(l) ? 'zh-Hant' : 'zh-Hans'
  }
  return 'zh-Hans'
}

// fontconfig `:lang=` code per region (Linux Noto resolves the right regional face).
const FC_LANG: Record<CjkRegion, string> = {
  ja: 'ja',
  ko: 'ko',
  'zh-Hant': 'zh-tw',
  'zh-Hans': 'zh-cn'
}

// Region-preferred faces, prepended ahead of the generic candidates. Best-effort,
// stable OS paths; any that are absent just fall through (no regression).
const CJK_REGION_CANDIDATES: Record<
  CjkRegion,
  Partial<Record<NodeJS.Platform, readonly string[]>>
> = {
  ja: {
    darwin: ['/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc'],
    win32: ['C:/Windows/Fonts/YuGothM.ttc', 'C:/Windows/Fonts/msgothic.ttc'],
    linux: ['/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf']
  },
  ko: {
    darwin: ['/System/Library/Fonts/AppleSDGothicNeo.ttc'],
    win32: ['C:/Windows/Fonts/malgun.ttf'],
    linux: ['/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf']
  },
  'zh-Hant': {
    darwin: ['/System/Library/Fonts/PingFang.ttc'],
    win32: ['C:/Windows/Fonts/msjh.ttc'],
    linux: ['/usr/share/fonts/opentype/noto/NotoSansCJKtc-Regular.otf']
  },
  'zh-Hans': {} // generic candidates are already SC-first
}

/** The host UI locale (Electron), with an env fallback for non-Electron/test use. */
function osLocale(): string {
  try {
    const loc = app?.getLocale?.()
    if (loc) {
      return loc
    }
  } catch {
    // app not ready / not running under Electron — fall back to env locale.
  }
  return process.env.LC_ALL || process.env.LC_CTYPE || process.env.LANG || ''
}

let cached: TerminalFallbackFonts | null = null

function candidatesFor(table: Record<NodeJS.Platform, readonly string[]>): readonly string[] {
  return table[process.platform] ?? []
}

// Resolve a font file via fontconfig. The hardcoded /usr/share paths above miss on
// many distros (the font is installed but elsewhere), so on Linux we ask fc-match
// for the best file matching a query first. Best-effort: returns undefined if
// fontconfig is absent or errors, so the hardcoded candidates still apply.
function fcMatchFile(query: string): Promise<string | undefined> {
  return new Promise((resolve) => {
    execFile(
      'fc-match',
      ['-f', '%{file}', query],
      { encoding: 'utf8', timeout: 3000 },
      (error, stdout) => {
        if (error) {
          resolve(undefined)
          return
        }
        const file = stdout.trim()
        resolve(file.length > 0 ? file : undefined)
      }
    )
  })
}

// Linux-only fontconfig candidates, prepended ahead of the hardcoded paths.
async function linuxFcCandidates(query: string): Promise<string[]> {
  if (process.platform !== 'linux') {
    return []
  }
  const file = await fcMatchFile(query)
  return file ? [file] : []
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
  // Han-unification: pick the CJK face for the user's locale so JP/KR users see
  // their own glyph forms, not Chinese ones. On Linux, ask fontconfig for the
  // region's lang (Noto resolves the regional face); region-preferred OS faces are
  // tried before the generic candidates. Emoji is region-independent.
  const region = cjkRegionFromLocale(osLocale())
  const [cjkFc, emojiFc] = await Promise.all([
    linuxFcCandidates(`:lang=${FC_LANG[region]}`),
    linuxFcCandidates(':charset=1F600')
  ])
  const regionCjk = CJK_REGION_CANDIDATES[region][process.platform] ?? []
  const [cjk, emoji] = await Promise.all([
    readFirstExisting([...cjkFc, ...regionCjk, ...candidatesFor(CJK_CANDIDATES)]),
    readFirstExisting([...emojiFc, ...candidatesFor(EMOJI_CANDIDATES)])
  ])
  cached = { cjk, emoji }
  return cached
}
