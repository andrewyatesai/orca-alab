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
 *  selection-highlight). Per-cell SGR colors flow through the grid separately. */
export type AtermThemeColors = {
  fg: number
  bg: number
  cursor: number
  selection: number
}

// Engine defaults (aterm_render::Theme::default) — used when a color is absent
// or unparseable so the constructor always gets a sane value.
const DEFAULT_COLORS: AtermThemeColors = {
  fg: 0xd0d0d0,
  bg: 0x111318,
  cursor: 0x50fa7b,
  selection: 0x264f78
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
  const bg = pick(theme.background, DEFAULT_COLORS.bg)
  return {
    // Floor the default fg's contrast against bg so body text is readable on
    // light themes (MINOR b); per-cell SGR contrast is engine-side / Phase-next.
    fg: enforceDefaultContrast(pick(theme.foreground, DEFAULT_COLORS.fg), bg),
    bg,
    cursor: pick(theme.cursor, DEFAULT_COLORS.cursor),
    selection: pick(theme.selectionBackground, DEFAULT_COLORS.selection)
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
}
