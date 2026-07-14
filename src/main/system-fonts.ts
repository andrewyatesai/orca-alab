import { execFile } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import { selectTerminalFontFaces, type FontFaceCandidate } from './terminal-font-face-selection'

let cachedFonts: string[] | null = null
let fontsPromise: Promise<string[]> | null = null
const SYSTEM_FONT_LIST_TIMEOUT_MS = 15_000
// Why: large macOS font catalogs can make system_profiler exceed 15s even
// when it is healthy; keep the longer wait scoped to that slow command.
const MAC_SYSTEM_FONT_LIST_TIMEOUT_MS = 45_000

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
  return execFileText(
    'system_profiler',
    ['SPFontsDataType', '-json'],
    32 * 1024 * 1024,
    MAC_SYSTEM_FONT_LIST_TIMEOUT_MS
  ).then((output) => {
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
  })
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

// Resolve a font FAMILY NAME to face FILE BYTES for the aterm engine: the face
// closest to the user's numeric weight (set_primary_font) plus, when the family
// ships a real heavier style, its bold face (set_bold_font). Null members mean
// unresolvable — the caller keeps the bundled default / synthetic embolden; this
// never throws.
let macFontFaceIndex: Map<string, FontFaceCandidate[]> | null = null

export type TerminalFontFaceBytes = {
  primary: Uint8Array | null
  bold: Uint8Array | null
}

export async function resolveTerminalFontFaceBytes(
  family: string,
  fontWeight?: number
): Promise<TerminalFontFaceBytes> {
  const name = family?.trim()
  if (!name) {
    return { primary: null, bold: null }
  }
  try {
    const faces = selectTerminalFontFaces(await listFamilyFaces(name), fontWeight)
    const primary = faces.primary ? await readFontFileBytes(faces.primary.path) : null
    // No primary bytes → keep the bundled family whole; a foreign bold face would
    // mismatch the bundled primary's metrics.
    const bold = primary && faces.bold ? await readFontFileBytes(faces.bold.path) : null
    return { primary, bold }
  } catch {
    return { primary: null, bold: null }
  }
}

async function readFontFileBytes(path: string): Promise<Uint8Array | null> {
  try {
    const buf = await readFile(path)
    return buf.length > 0 ? new Uint8Array(buf) : null
  } catch {
    return null
  }
}

function listFamilyFaces(family: string): Promise<FontFaceCandidate[]> {
  if (process.platform === 'darwin') {
    return listMacFamilyFaces(family)
  }
  if (process.platform === 'win32') {
    return listWindowsFamilyFaces(family)
  }
  return listLinuxFamilyFaces(family)
}

async function listMacFamilyFaces(family: string): Promise<FontFaceCandidate[]> {
  macFontFaceIndex ??= await buildMacFontFaceIndex()
  return macFontFaceIndex.get(family.toLowerCase()) ?? []
}

async function buildMacFontFaceIndex(): Promise<Map<string, FontFaceCandidate[]>> {
  // Same slow system_profiler call as listMacFonts — give it the longer macOS timeout.
  const out = await execFileText(
    'system_profiler',
    ['SPFontsDataType', '-json'],
    32 * 1024 * 1024,
    MAC_SYSTEM_FONT_LIST_TIMEOUT_MS
  )
  const parsed = JSON.parse(out) as {
    SPFontsDataType?: { path?: string; typefaces?: { family?: string; style?: string }[] }[]
  }
  const byFamily = new Map<string, FontFaceCandidate[]>()
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
  return byFamily
}

// fc-list enumerates the faces the family REALLY ships (file + style); fc-match
// alone always "succeeds" with a best-effort substitute, which would fake a bold
// style the family doesn't have.
async function listLinuxFamilyFaces(family: string): Promise<FontFaceCandidate[]> {
  const out = await execFileText(
    'fc-list',
    ['-f', '%{file}\t%{style}\n', family],
    1024 * 1024
  ).catch(() => '')
  const faces: FontFaceCandidate[] = []
  for (const line of out.split('\n')) {
    const [file, style = ''] = line.split('\t')
    if (file?.trim()) {
      // Multi-locale style lists ("Bold,Negreta") put the English name first.
      faces.push({ path: file.trim(), style: style.split(',')[0].trim() })
    }
  }
  if (faces.length > 0) {
    return faces
  }
  // fc-list absent/empty: fall back to fc-match's best-effort Regular (bold stays
  // synthetic — there is no evidence the family ships one).
  const matched = await execFileText(
    'fc-match',
    ['-f', '%{file}', `${family}:style=Regular`],
    64 * 1024
  )
    .then((out2) => out2.trim() || null)
    .catch(() => null)
  return matched ? [{ path: matched, style: 'Regular' }] : []
}

async function listWindowsFamilyFaces(family: string): Promise<FontFaceCandidate[]> {
  // The Fonts registry maps "<Family> <Style> (TrueType)" → a file (usually a bare
  // name under %WINDIR%\Fonts); the name suffix after the family is the style.
  const script = [
    "$key = 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts'",
    `$fam = ${JSON.stringify(family)}`,
    '(Get-ItemProperty $key).PSObject.Properties |',
    "  Where-Object { $_.Name -like ($fam + '*') } |",
    '  ForEach-Object {',
    '    $p = $_.Value',
    "    if (-not [System.IO.Path]::IsPathRooted($p)) { $p = Join-Path $env:WINDIR ('Fonts\\' + $p) }",
    '    "$($_.Name)`t$p"',
    '  }'
  ].join('\n')
  const out = await execFileText(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', script],
    1024 * 1024
  ).catch(() => '')
  const faces: FontFaceCandidate[] = []
  for (const line of out.split('\n')) {
    const [name, file] = line.split('\t')
    if (name?.trim() && file?.trim()) {
      faces.push({ path: file.trim(), style: windowsRegistryStyle(name.trim(), family) })
    }
  }
  return faces
}

/** Style portion of a Fonts-registry value name: the family prefix and the
 *  trailing "(TrueType)"/"(OpenType)" marker stripped off. */
function windowsRegistryStyle(registryName: string, family: string): string {
  const fam = family.trim()
  let style = registryName.replace(/\s*\((?:TrueType|OpenType)\)\s*$/i, '').trim()
  if (style.toLowerCase().startsWith(fam.toLowerCase())) {
    style = style.slice(fam.length).trim()
  }
  return style
}

function execFileText(
  command: string,
  args: string[],
  maxBuffer: number,
  timeoutMs = SYSTEM_FONT_LIST_TIMEOUT_MS
): Promise<string> {
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
      }, timeoutMs)
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
