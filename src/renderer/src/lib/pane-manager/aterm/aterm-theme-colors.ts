import type { ITheme } from '@xterm/xterm'
import { useAppStore } from '@/store'
import {
  getBuiltinTheme,
  getSystemPrefersDark,
  resolveEffectiveTerminalAppearance
} from '@/lib/terminal-theme'
import { composeActiveTerminalTheme } from '@/components/terminal-pane/terminal-appearance'
import { e2eConfig } from '@/lib/e2e-config'

/** 0x00RRGGBB color seeds for the aterm renderer's DEFAULT theme (fg/bg/cursor/
 *  selection-highlight) plus the 16 ANSI palette entries the theme specifies.
 *  Per-cell SGR truecolor flows through the grid separately. */
export type AtermThemeColors = {
  fg: number
  bg: number
  cursor: number
  selection: number
  /** Explicit foreground for SELECTED text (theme `selectionForeground`), 0x00RRGGBB,
   *  or null to keep the engine's WCAG contrast-floor default. */
  selectionForeground: number | null
  /** ANSI/indexed palette overrides (index 0–15) parsed from the theme; indices
   *  absent here keep the engine's built-in default. 0x00RRGGBB. */
  palette: { index: number; rgb: number }[]
}

// Engine defaults (aterm_render::Theme::default) — used when a color is absent
// or unparseable so the constructor always gets a sane value. An empty palette
// means "use the engine's built-in ANSI defaults".
const DEFAULT_COLORS: AtermThemeColors = {
  fg: 0xd0d0d0,
  bg: 0x111318,
  cursor: 0x50fa7b,
  selection: 0x264f78,
  selectionForeground: null,
  palette: []
}

// xterm ITheme's 16 ANSI colour fields → engine palette indices 0–15 (normal 0–7,
// bright 8–15). Resolved into AtermThemeColors.palette and seeded after construction.
const ANSI_THEME_KEYS: { key: keyof ITheme; index: number }[] = [
  { key: 'black', index: 0 },
  { key: 'red', index: 1 },
  { key: 'green', index: 2 },
  { key: 'yellow', index: 3 },
  { key: 'blue', index: 4 },
  { key: 'magenta', index: 5 },
  { key: 'cyan', index: 6 },
  { key: 'white', index: 7 },
  { key: 'brightBlack', index: 8 },
  { key: 'brightRed', index: 9 },
  { key: 'brightGreen', index: 10 },
  { key: 'brightYellow', index: 11 },
  { key: 'brightBlue', index: 12 },
  { key: 'brightMagenta', index: 13 },
  { key: 'brightCyan', index: 14 },
  { key: 'brightWhite', index: 15 }
]

/** Resolve the theme's 16 ANSI colours to engine palette overrides; skip any the
 *  theme doesn't specify (so the engine default stands for that index). */
function resolveAtermPalette(theme: ITheme): { index: number; rgb: number }[] {
  const out: { index: number; rgb: number }[] = []
  for (const { key, index } of ANSI_THEME_KEYS) {
    const rgb = cssColorToU32(theme[key] as string | undefined)
    if (rgb !== null) {
      out.push({ index, rgb })
    }
  }
  return out
}

/** Seed the engine's ANSI/indexed palette from the resolved theme so SGR-indexed
 *  cell colours (ls/git/prompts) render in the user's theme instead of the engine's
 *  built-in VGA defaults. Indices the theme omits keep the engine default. Works on
 *  both the CPU (AtermTerminal) and GPU (AtermGpuTerminal) engines — the palette
 *  lives on the shared grid. */
export function seedAtermPalette(
  term: { set_palette_color: (index: number, r: number, g: number, b: number) => void },
  colors: AtermThemeColors
): void {
  for (const { index, rgb } of colors.palette) {
    term.set_palette_color(index, (rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff)
  }
}

/** Seed the engine state its OWN query replies report, so aterm can be the
 *  authoritative responder (OSC 10/11 colour + CSI 14t/16t pixel-size): the
 *  default fg/bg the engine reports for OSC 10/11, and the real device-pixel cell
 *  size for the window/cell-size reports (the engine has no canvas to measure). */
export function seedAtermReplyDefaults(
  term: {
    set_default_foreground: (r: number, g: number, b: number) => void
    set_default_background: (r: number, g: number, b: number) => void
    set_cell_pixel_size: (width: number, height: number) => void
  },
  colors: AtermThemeColors,
  cellWidth: number,
  cellHeight: number
): void {
  term.set_default_foreground((colors.fg >> 16) & 0xff, (colors.fg >> 8) & 0xff, colors.fg & 0xff)
  term.set_default_background((colors.bg >> 16) & 0xff, (colors.bg >> 8) & 0xff, colors.bg & 0xff)
  term.set_cell_pixel_size(Math.max(1, Math.round(cellWidth)), Math.max(1, Math.round(cellHeight)))
}

/** Apply a full theme change to a live engine IN PLACE (no pane rebuild): the
 *  renderer fg/bg/cursor/selection, the 16 ANSI palette, and the reply-default
 *  colours. The caller schedules a redraw. */
export function applyAtermLiveTheme(
  term: {
    set_theme: (fg: number, bg: number, cursor: number, selection: number) => void
    set_selection_fg: (fg: number | undefined) => void
    set_palette_color: (index: number, r: number, g: number, b: number) => void
    set_default_foreground: (r: number, g: number, b: number) => void
    set_default_background: (r: number, g: number, b: number) => void
    set_cell_pixel_size: (width: number, height: number) => void
  },
  colors: AtermThemeColors,
  cellWidth: number,
  cellHeight: number
): void {
  term.set_theme(colors.fg, colors.bg, colors.cursor, colors.selection)
  // null → undefined: keep the engine's WCAG contrast-floor default when the theme
  // sets no explicit selectionForeground.
  term.set_selection_fg(colors.selectionForeground ?? undefined)
  seedAtermPalette(term, colors)
  seedAtermReplyDefaults(term, colors, cellWidth, cellHeight)
}

/** Parse a CSS color (`#rgb`, `#rrggbb`, `rgb()/rgba()`) to 0x00RRGGBB.
 *  Returns null when the value isn't a form we recognize so the caller can
 *  fall back to the engine default. Alpha is dropped — aterm seeds opaque
 *  default colors and applies its own selection blend. */
export function cssColorToU32(value: string | undefined): number | null {
  if (!value) {
    return null
  }
  const trimmed = value.trim()
  const hex = trimmed.startsWith('#') ? trimmed.slice(1) : null
  if (hex) {
    const expanded =
      hex.length === 3
        ? hex
            .split('')
            .map((c) => c + c)
            .join('')
        : hex
    if (expanded.length === 6 && /^[0-9a-fA-F]{6}$/.test(expanded)) {
      return parseInt(expanded, 16)
    }
    // #rrggbbaa — keep the rgb triplet, drop alpha.
    if (expanded.length === 8 && /^[0-9a-fA-F]{8}$/.test(expanded)) {
      return parseInt(expanded.slice(0, 6), 16)
    }
    return null
  }
  const rgb = trimmed.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i)
  if (rgb) {
    const r = Math.min(255, parseInt(rgb[1], 10))
    const g = Math.min(255, parseInt(rgb[2], 10))
    const b = Math.min(255, parseInt(rgb[3], 10))
    return (r << 16) | (g << 8) | b
  }
  return null
}

function pick(value: string | undefined, fallback: number): number {
  return cssColorToU32(value) ?? fallback
}

// WCAG AA body-text floor; xterm's DOM/canvas renderers default
// minimumContrastRatio to this so fg-on-bg stays readable on light themes.
const MIN_CONTRAST_RATIO = 4.5

/** sRGB relative luminance (WCAG) of a 0x00RRGGBB color. */
function relativeLuminance(rgb: number): number {
  const channel = (c: number): number => {
    const n = c / 255
    return n <= 0.03928 ? n / 12.92 : Math.pow((n + 0.055) / 1.055, 2.4)
  }
  const r = channel((rgb >> 16) & 0xff)
  const g = channel((rgb >> 8) & 0xff)
  const b = channel(rgb & 0xff)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG contrast ratio between two 0x00RRGGBB colors (>= 1). */
export function contrastRatio(a: number, b: number): number {
  const la = relativeLuminance(a)
  const lb = relativeLuminance(b)
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

/** Linearly blend `fg` toward `target` (both 0x00RRGGBB) by `t` in [0,1]. */
function blendToward(fg: number, target: number, t: number): number {
  const mix = (shift: number): number => {
    const f = (fg >> shift) & 0xff
    const g = (target >> shift) & 0xff
    return Math.round(f + (g - f) * t) & 0xff
  }
  return (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/** Ensure the SEEDED default fg meets ~4.5:1 against the default bg so body text
 *  stays readable on light themes — the floor xterm gets from minimumContrastRatio.
 *  Per-cell SGR colors are NOT adjusted here (the CPU renderer seeds them directly
 *  in the engine); a per-cell contrast floor is engine-side / Phase-next. We only
 *  nudge the default fg toward black or white (whichever the bg contrasts with) and
 *  pick the smallest blend that clears the floor, preserving hue where possible. */
export function enforceDefaultContrast(fg: number, bg: number): number {
  if (contrastRatio(fg, bg) >= MIN_CONTRAST_RATIO) {
    return fg
  }
  // Push toward the extreme that's further from the bg in luminance.
  const target = relativeLuminance(bg) > 0.5 ? 0x000000 : 0xffffff
  for (let t = 0.1; t <= 1.0001; t += 0.1) {
    const candidate = blendToward(fg, target, t)
    if (contrastRatio(candidate, bg) >= MIN_CONTRAST_RATIO) {
      return candidate
    }
  }
  return target
}

/** Read orca's active terminal theme from the store and reduce it to the four
 *  color seeds the aterm renderer needs. Applied at pane construction; a live
 *  theme change recreates the pane (Phase 1 scope). */
export function resolveAtermThemeColors(): AtermThemeColors {
  const settings = useAppStore.getState().settings
  if (!settings) {
    return DEFAULT_COLORS
  }
  const appearance = resolveEffectiveTerminalAppearance(settings, getSystemPrefersDark())
  const baseTheme: ITheme | null = appearance.theme ?? getBuiltinTheme(appearance.themeName)
  const theme = composeActiveTerminalTheme(baseTheme, settings)
  if (!theme) {
    return DEFAULT_COLORS
  }
  return atermThemeColorsFromITheme(theme)
}

/** Reduce a composed xterm ITheme to the aterm renderer's colour seeds. Shared by
 *  resolveAtermThemeColors (store path) and the live re-theme path
 *  (applyTerminalAppearance), so both produce identical colours. */
export function atermThemeColorsFromITheme(theme: ITheme): AtermThemeColors {
  const bg = pick(theme.background, DEFAULT_COLORS.bg)
  return {
    // Floor the default fg's contrast against bg so body text is readable on
    // light themes (MINOR b); per-cell SGR contrast is engine-side / Phase-next.
    fg: enforceDefaultContrast(pick(theme.foreground, DEFAULT_COLORS.fg), bg),
    bg,
    cursor: pick(theme.cursor, DEFAULT_COLORS.cursor),
    selection: pick(theme.selectionBackground, DEFAULT_COLORS.selection),
    // Explicit selected-text fg if the theme sets one; null keeps the engine's
    // WCAG contrast-floor default (cssColorToU32 returns null when absent/unparseable).
    selectionForeground: cssColorToU32(theme.selectionForeground),
    // The 16 ANSI palette colours so SGR-indexed cell colours match the theme.
    palette: resolveAtermPalette(theme)
  }
}

// E2E only: expose the configured theme bg resolved through the SAME pipeline the
// renderer seeds from (resolveEffectiveTerminalAppearance → composeActiveTerminalTheme,
// read from the store). The phase1 test compares the painted canvas pixel against
// THIS — an independent resolution — so a renderer that paints a non-theme bg
// fails, instead of comparing against a value the renderer echoed onto itself.
if (e2eConfig.exposeStore && typeof window !== 'undefined') {
  ;(
    window as unknown as { __resolveAtermThemeBg?: () => [number, number, number] }
  ).__resolveAtermThemeBg = () => {
    const { bg } = resolveAtermThemeColors()
    return [(bg >> 16) & 0xff, (bg >> 8) & 0xff, bg & 0xff]
  }
  // E2E only: the resolved ANSI palette (index → [r,g,b]) the renderer seeds into
  // the engine, so the palette spec can assert a rendered SGR-indexed block matches
  // the THEME colour (proving the seed reached pixels), not the engine VGA default.
  ;(
    window as unknown as {
      __resolveAtermThemePalette?: () => { index: number; rgb: [number, number, number] }[]
    }
  ).__resolveAtermThemePalette = () =>
    resolveAtermThemeColors().palette.map(({ index, rgb }) => ({
      index,
      rgb: [(rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff]
    }))
}
