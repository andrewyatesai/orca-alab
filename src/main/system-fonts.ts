import { execFile } from 'child_process'
import { readFile } from 'fs/promises'

let cachedFonts: string[] | null = null
let fontsPromise: Promise<string[]> | null = null
const SYSTEM_FONT_LIST_TIMEOUT_MS = 15_000

export async function listSystemFontFamilies(): Promise<string[]> {
  if (cachedFonts) {
    return cachedFonts
  }
  if (fontsPromise) {
    return fontsPromise
  }

  fontsPromise = loadSystemFontFamilies()
    .then((fonts) => {
      cachedFonts = fonts.length > 0 ? fonts : fallbackFonts()
      return cachedFonts
    })
    .catch(() => {
      cachedFonts = fallbackFonts()
      return cachedFonts
    })
    .finally(() => {
      fontsPromise = null
    })

  return fontsPromise
}

export function warmSystemFontFamilies(): void {
  void listSystemFontFamilies()
}

function loadSystemFontFamilies(): Promise<string[]> {
  if (process.platform === 'darwin') {
    return listMacFonts()
  }
  if (process.platform === 'win32') {
    return listWindowsFonts()
  }
  return listLinuxFonts()
}

function listMacFonts(): Promise<string[]> {
  return execFileText('system_profiler', ['SPFontsDataType', '-json'], 32 * 1024 * 1024).then(
    (output) => {
      const parsed = JSON.parse(output) as {
        SPFontsDataType?: {
          typefaces?: {
            family?: string
          }[]
        }[]
      }

      return uniqueSorted(
        (parsed.SPFontsDataType ?? []).flatMap((font) =>
          (font.typefaces ?? []).map((typeface) => typeface.family)
        )
      )
    }
  )
}

function listLinuxFonts(): Promise<string[]> {
  return execFileText('fc-list', [':', 'family'], 8 * 1024 * 1024).then((output) =>
    uniqueSorted(
      output
        .split('\n')
        .flatMap((line) => line.split(','))
        .map((name) => name.trim())
        .filter(Boolean)
    )
  )
}

function listWindowsFonts(): Promise<string[]> {
  const script = `
Add-Type -AssemblyName System.Drawing
$fonts = New-Object System.Drawing.Text.InstalledFontCollection
$fonts.Families | ForEach-Object { $_.Name }
`

  return execFileText(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', script],
    8 * 1024 * 1024
  ).then((output) =>
    uniqueSorted(
      output
        .split('\n')
        .map((name) => name.trim())
        .filter(Boolean)
    )
  )
}

// Resolve a font FAMILY NAME to its primary (regular) face FILE BYTES for the aterm
// engine's set_primary_font. Returns null when unresolvable — the caller keeps the
// bundled default; this never throws. .ttc collections are deprioritized because the
// engine's glyph loader reads a single face.
let macFontPathIndex: Map<string, string> | null = null

export async function resolvePrimaryFontBytes(family: string): Promise<Uint8Array | null> {
  const name = family?.trim()
  if (!name) {
    return null
  }
  try {
    const path = await resolveFontFilePath(name)
    if (!path) {
      return null
    }
    return new Uint8Array(await readFile(path))
  } catch {
    return null
  }
}

function resolveFontFilePath(family: string): Promise<string | null> {
  if (process.platform === 'darwin') {
    return resolveMacFontPath(family)
  }
  if (process.platform === 'win32') {
    return resolveWindowsFontPath(family)
  }
  return resolveLinuxFontPath(family)
}

async function resolveMacFontPath(family: string): Promise<string | null> {
  macFontPathIndex ??= await buildMacFontPathIndex()
  return macFontPathIndex.get(family.toLowerCase()) ?? null
}

async function buildMacFontPathIndex(): Promise<Map<string, string>> {
  const out = await execFileText('system_profiler', ['SPFontsDataType', '-json'], 32 * 1024 * 1024)
  const parsed = JSON.parse(out) as {
    SPFontsDataType?: { path?: string; typefaces?: { family?: string; style?: string }[] }[]
  }
  const byFamily = new Map<string, { path: string; style: string }[]>()
  for (const file of parsed.SPFontsDataType ?? []) {
    const path = file.path
    if (!path) {
      continue
    }
    for (const tf of file.typefaces ?? []) {
      const fam = tf.family?.trim()
      if (!fam) {
        continue
      }
      const key = fam.toLowerCase()
      const list = byFamily.get(key) ?? []
      list.push({ path, style: (tf.style ?? '').trim() })
      byFamily.set(key, list)
    }
  }
  const isRegular = (s: string): boolean => /^(regular|book|roman)$/i.test(s)
  const isTtc = (p: string): boolean => p.toLowerCase().endsWith('.ttc')
  const index = new Map<string, string>()
  for (const [key, list] of byFamily) {
    // Prefer a Regular style in a single-face (.ttf/.otf) file the engine can load.
    const pick =
      list.find((e) => isRegular(e.style) && !isTtc(e.path)) ??
      list.find((e) => isRegular(e.style)) ??
      list.find((e) => !isTtc(e.path)) ??
      list[0]
    index.set(key, pick.path)
  }
  return index
}

function resolveLinuxFontPath(family: string): Promise<string | null> {
  return execFileText('fc-match', ['-f', '%{file}', `${family}:style=Regular`], 64 * 1024)
    .then((out) => out.trim() || null)
    .catch(() => null)
}

function resolveWindowsFontPath(family: string): Promise<string | null> {
  // Best-effort: the Fonts registry maps "<Family> (TrueType)" → a file (usually a
  // bare name under %WINDIR%\Fonts). Pick the first value whose name starts with the
  // family; absent a match (or PowerShell), null → bundled default.
  const script = [
    "$key = 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts'",
    `$fam = ${JSON.stringify(family)}`,
    '$p = (Get-ItemProperty $key).PSObject.Properties |',
    "  Where-Object { $_.Name -like ($fam + '*') } |",
    '  Select-Object -First 1 -ExpandProperty Value',
    "if ($p) { if ([System.IO.Path]::IsPathRooted($p)) { $p } else { Join-Path $env:WINDIR ('Fonts\\' + $p) } }"
  ].join('\n')
  return execFileText(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', script],
    64 * 1024
  )
    .then((out) => out.trim() || null)
    .catch(() => null)
}

function execFileText(command: string, args: string[], maxBuffer: number): Promise<string> {
  return new Promise((resolve, reject) => {
    let settled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const child = execFile(command, args, { encoding: 'utf8', maxBuffer }, (error, stdout) => {
      if (settled) {
        return
      }
      settled = true
      if (timer) {
        clearTimeout(timer)
      }
      if (error) {
        reject(error)
        return
      }
      resolve(stdout)
    })
    if (!settled) {
      timer = setTimeout(() => {
        if (settled) {
          return
        }
        settled = true
        // Why: font discovery is a startup convenience; a stuck OS font tool
        // should fall back instead of keeping settings IPC pending forever.
        child.kill()
        reject(new Error(`Timed out listing system fonts with ${command}`))
      }, SYSTEM_FONT_LIST_TIMEOUT_MS)
      if (typeof timer === 'object' && 'unref' in timer) {
        timer.unref()
      }
    }
  })
}

function uniqueSorted(values: (string | undefined)[]): string[] {
  return Array.from(
    new Set(
      values
        .map((value) => value?.trim() ?? '')
        .filter((value) => value.length > 0 && !value.startsWith('.'))
    )
  ).sort((a, b) => a.localeCompare(b))
}

function fallbackFonts(): string[] {
  if (process.platform === 'darwin') {
    return ['SF Mono', 'Menlo', 'Monaco', 'JetBrains Mono', 'Fira Code']
  }
  if (process.platform === 'win32') {
    return ['Cascadia Mono', 'Consolas', 'Lucida Console', 'JetBrains Mono', 'Fira Code']
  }
  return [
    'JetBrains Mono',
    'Fira Code',
    'DejaVu Sans Mono',
    'Liberation Mono',
    'Ubuntu Mono',
    'Noto Sans Mono'
  ]
}
