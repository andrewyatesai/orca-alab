import { readFile } from 'node:fs/promises'
import { execFile } from 'node:child_process'
import { app } from 'electron'
import {
  loadUserFallbackStack,
  normalizeUserFallbackFamilies
} from './terminal-user-fallback-stack'

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

// Non-Latin / complex scripts the region-aware CJK face does NOT cover (Arabic,
// Hebrew, Indic/Devanagari, Thai) plus a broad-coverage catch-all face. Each
// becomes one entry in the renderer's fallback chain, appended after the CJK
// face via add_fallback_font so e.g. Arabic still renders real glyphs.
export type FallbackScript = 'arabic' | 'hebrew' | 'devanagari' | 'thai' | 'unicode'

/** One additional fallback face actually found on the host, in chain order. */
export type FallbackChainEntry = {
  bytes: Uint8Array
  script: FallbackScript
}

export type TerminalFallbackFonts = {
  // User-configured fallback families (terminalFontFallbackFamilies), resolved
  // to face bytes, in the user's order. They precede the CJK face in the chain;
  // unresolvable families are skipped. Empty when the setting is unset/empty.
  user: { family: string; bytes: Uint8Array }[]
  // CJK follows the user stack (set_fallback_font when no user stack); `region`
  // surfaces which Han form was picked. Absent when no CJK face exists.
  cjk?: { bytes: Uint8Array; region: CjkRegion }
  emoji?: Uint8Array
  // Monochrome SYMBOL tier (set_symbol_font), consulted only after the primary +
  // fallback chain miss, so media/technical symbols (⏸⏹⏺) get a real glyph instead
  // of tofu. Absent when the host has no symbol face.
  symbol?: Uint8Array
  // Ordered non-Latin fallbacks appended after CJK (add_fallback_font). Only
  // faces that really resolved on this host appear; missing ones are skipped.
  chain: FallbackChainEntry[]
}

// First-existing wins per category. macOS .ttc collections and Apple Color Emoji
// (sbix) are read whole; the engine selects face 0.
const CJK_CANDIDATES: Record<NodeJS.Platform, readonly string[]> = {
  darwin: ['/System/Library/Fonts/PingFang.ttc', '/System/Library/Fonts/Hiragino Sans GB.ttc'],
  win32: [
    'C:/Windows/Fonts/msyh.ttc',
    'C:/Windows/Fonts/simsun.ttc',
    'C:/Windows/Fonts/msgothic.ttc'
  ],
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

// Monochrome symbol faces, most-preferred first — mirrors the native engine's
// SYMBOL_FALLBACK_CANDIDATES (aterm-render). STIX Two Math / Noto Sans Symbols 2 carry
// the media/technical glyphs (⏸⏹⏺ U+23F8..23FA) the primary + emoji faces miss. First
// existing wins; absent on a host → no symbol tier (JetBrains Mono still covers Latin).
const SYMBOL_CANDIDATES: Record<NodeJS.Platform, readonly string[]> = {
  darwin: [
    '/System/Library/Fonts/Supplemental/STIXTwoMath.otf',
    '/System/Library/Fonts/Apple Symbols.ttf'
  ],
  win32: ['C:/Windows/Fonts/seguisym.ttf'],
  linux: [
    '/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf',
    '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'
  ]
} as unknown as Record<NodeJS.Platform, readonly string[]>

// Non-Latin fallback faces, in chain order. First-existing wins per script;
// stable OS paths mirroring the CJK/emoji candidate style (PingFang-era macOS
// system fonts, Noto on Linux, the Windows font dir). Missing files fall through
// (the script just has no fallback face on that host — no regression, no fake).
// On Linux we also ask fontconfig (:lang) for each script before these paths.
const NON_LATIN_CANDIDATES: Record<
  FallbackScript,
  Partial<Record<NodeJS.Platform, readonly string[]>>
> = {
  arabic: {
    darwin: [
      '/System/Library/Fonts/Supplemental/GeezaPro.ttc',
      '/System/Library/Fonts/Supplemental/Geeza Pro.ttf',
      '/Library/Fonts/Arial Unicode.ttf'
    ],
    win32: ['C:/Windows/Fonts/segoeui.ttf', 'C:/Windows/Fonts/tahoma.ttf'],
    linux: [
      '/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf',
      '/usr/share/fonts/noto/NotoSansArabic-Regular.ttf',
      '/usr/share/fonts/google-noto/NotoSansArabic-Regular.ttf'
    ]
  },
  hebrew: {
    darwin: [
      '/System/Library/Fonts/Supplemental/Arial Hebrew.ttc',
      '/System/Library/Fonts/Supplemental/ArialHB.ttc',
      '/Library/Fonts/Arial Unicode.ttf'
    ],
    win32: ['C:/Windows/Fonts/segoeui.ttf', 'C:/Windows/Fonts/david.ttf'],
    linux: [
      '/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf',
      '/usr/share/fonts/noto/NotoSansHebrew-Regular.ttf',
      '/usr/share/fonts/google-noto/NotoSansHebrew-Regular.ttf'
    ]
  },
  devanagari: {
    darwin: [
      '/System/Library/Fonts/Supplemental/DevanagariMT.ttc',
      '/System/Library/Fonts/Kohinoor.ttc',
      '/Library/Fonts/Arial Unicode.ttf'
    ],
    win32: ['C:/Windows/Fonts/Nirmala.ttf', 'C:/Windows/Fonts/mangal.ttf'],
    linux: [
      '/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf',
      '/usr/share/fonts/noto/NotoSansDevanagari-Regular.ttf',
      '/usr/share/fonts/google-noto/NotoSansDevanagari-Regular.ttf'
    ]
  },
  thai: {
    darwin: [
      '/System/Library/Fonts/Supplemental/Ayuthaya.ttf',
      '/System/Library/Fonts/Thonburi.ttc',
      '/Library/Fonts/Arial Unicode.ttf'
    ],
    win32: ['C:/Windows/Fonts/leelawui.ttf', 'C:/Windows/Fonts/tahoma.ttf'],
    linux: [
      '/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf',
      '/usr/share/fonts/noto/NotoSansThai-Regular.ttf',
      '/usr/share/fonts/google-noto/NotoSansThai-Regular.ttf'
    ]
  },
  // Broad-coverage catch-all (Arial Unicode MS / Noto Sans) as the final link so
  // scripts none of the above cover still resolve to a real glyph when present.
  unicode: {
    darwin: ['/Library/Fonts/Arial Unicode.ttf', '/System/Library/Fonts/Helvetica.ttc'],
    win32: ['C:/Windows/Fonts/ARIALUNI.TTF', 'C:/Windows/Fonts/segoeui.ttf'],
    linux: [
      '/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf',
      '/usr/share/fonts/noto/NotoSans-Regular.ttf',
      '/usr/share/fonts/google-noto/NotoSans-Regular.ttf'
    ]
  }
}

// fontconfig `:lang=` codes per script (Linux Noto resolves the right face). The
// catch-all has no single lang; a Latin charset probe asks for a broad face.
const NON_LATIN_FC: Record<FallbackScript, string> = {
  arabic: ':lang=ar',
  hebrew: ':lang=he',
  devanagari: ':lang=hi',
  thai: ':lang=th',
  unicode: ':lang=en'
}

// Chain order: complex scripts first, broad catch-all last (lowest priority).
const FALLBACK_SCRIPT_ORDER: readonly FallbackScript[] = [
  'arabic',
  'hebrew',
  'devanagari',
  'thai',
  'unicode'
]

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

/** The injectable face classes the aterm engine reports misses for (E1 lazy
 *  fonts): 'text' = the mono faces (CJK + script chain + symbol), 'emoji' =
 *  the colour emoji face. Kept as separate loads because emoji dominates the
 *  payload (Apple Color Emoji is ~183MB) and most sessions never render one. */
export type FallbackFontClass = 'text' | 'emoji'

// The OS half of the text class, plus the resolved file paths it shipped so the
// user half can be de-duped against it at assembly time.
type OsTextFallbackFonts = Omit<TerminalFallbackFonts, 'emoji' | 'user'> & {
  usedPaths: Set<string>
}

// Per-class caches (promises, so concurrent requests share one disk read). The
// emoji read only ever happens when a renderer actually reports an emoji miss.
let cachedText: Promise<OsTextFallbackFonts> | null = null
let cachedEmoji: Promise<Uint8Array | undefined> | null = null

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
// unreadable file just moves to the next candidate. Returns the resolved path
// alongside the bytes so callers can de-dup faces by path; undefined when none of
// the platform's candidates are present (the renderer keeps JetBrains Mono).
async function readFirstExistingWithPath(
  candidates: readonly string[]
): Promise<{ path: string; bytes: Uint8Array } | undefined> {
  for (const path of candidates) {
    try {
      const buf = await readFile(path)
      if (buf.length > 0) {
        return { path, bytes: new Uint8Array(buf) }
      }
    } catch {
      // Missing/unreadable candidate — try the next one.
    }
  }
  return undefined
}

async function readFirstExisting(candidates: readonly string[]): Promise<Uint8Array | undefined> {
  return (await readFirstExistingWithPath(candidates))?.bytes
}

function nonLatinCandidatesFor(script: FallbackScript): readonly string[] {
  return NON_LATIN_CANDIDATES[script][process.platform] ?? []
}

// Discover the non-Latin fallback chain: for each script (in priority order) read
// the first candidate that actually exists, skipping scripts with no host face,
// and de-dup by resolved path so the same file is never shipped twice (e.g. when
// several scripts resolve to one broad-coverage face like Arial Unicode).
async function discoverChain(usedPaths: Set<string>): Promise<FallbackChainEntry[]> {
  const chain: FallbackChainEntry[] = []
  for (const script of FALLBACK_SCRIPT_ORDER) {
    const fc = await linuxFcCandidates(NON_LATIN_FC[script])
    const found = await readFirstExistingWithPath([...fc, ...nonLatinCandidatesFor(script)])
    if (!found || usedPaths.has(found.path)) {
      continue
    }
    usedPaths.add(found.path)
    chain.push({ bytes: found.bytes, script })
  }
  return chain
}

async function loadTextFonts(): Promise<OsTextFallbackFonts> {
  // Han-unification: pick the CJK face for the user's locale so JP/KR users see
  // their own glyph forms, not Chinese ones. On Linux, ask fontconfig for the
  // region's lang (Noto resolves the regional face); region-preferred OS faces are
  // tried before the generic candidates. Symbol is region-independent (the
  // symbol charset probe asks for U+23F8 ⏸, a media glyph the primary faces miss).
  const region = cjkRegionFromLocale(osLocale())
  const [cjkFc, symbolFc] = await Promise.all([
    linuxFcCandidates(`:lang=${FC_LANG[region]}`),
    linuxFcCandidates(':charset=23F8')
  ])
  const regionCjk = CJK_REGION_CANDIDATES[region][process.platform] ?? []
  const [cjkFound, symbol] = await Promise.all([
    readFirstExistingWithPath([...cjkFc, ...regionCjk, ...candidatesFor(CJK_CANDIDATES)]),
    readFirstExisting([...symbolFc, ...candidatesFor(SYMBOL_CANDIDATES)])
  ])
  // De-dup the chain against the CJK face so a face that doubles as both (e.g. a
  // pan-Unicode Noto) is never shipped twice.
  const usedPaths = new Set<string>()
  if (cjkFound) {
    usedPaths.add(cjkFound.path)
  }
  const chain = await discoverChain(usedPaths)
  return {
    cjk: cjkFound ? { bytes: cjkFound.bytes, region } : undefined,
    symbol,
    chain,
    usedPaths
  }
}

async function loadEmojiFont(): Promise<Uint8Array | undefined> {
  const emojiFc = await linuxFcCandidates(':charset=1F600')
  return readFirstExisting([...emojiFc, ...candidatesFor(EMOJI_CANDIDATES)])
}

/**
 * Read the host fallback fonts for the requested face classes (both when
 * `classes` is omitted — the legacy eager shape). Class results are cached for
 * the process lifetime; the ~183MB emoji face is only ever read once a
 * renderer actually reports an emoji glyph miss (E1 lazy fonts).
 * `userFamilies` (terminalFontFallbackFamilies) resolve into the `user` field of
 * the text class, ordered, before the CJK face.
 */
export async function getTerminalFallbackFonts(
  classes?: readonly FallbackFontClass[],
  userFamilies?: readonly string[]
): Promise<TerminalFallbackFonts> {
  const wantText = !classes || classes.includes('text')
  const wantEmoji = !classes || classes.includes('emoji')
  const families = normalizeUserFallbackFamilies(userFamilies)
  const [text, emoji, userFaces] = await Promise.all([
    wantText ? (cachedText ??= loadTextFonts()) : undefined,
    wantEmoji ? (cachedEmoji ??= loadEmojiFont()) : undefined,
    wantText && families.length > 0 ? loadUserFallbackStack(families) : []
  ])
  // De-dup user faces against the OS half by resolved path so a family that
  // doubles as the CJK/chain face (e.g. Microsoft YaHei) is never shipped twice.
  const user = userFaces
    .filter((face) => !face.path || !text?.usedPaths.has(face.path))
    .map(({ family, bytes }) => ({ family, bytes }))
  return { user, cjk: text?.cjk, symbol: text?.symbol, chain: text?.chain ?? [], emoji }
}
