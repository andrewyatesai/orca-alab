import type { ITheme } from '@xterm/xterm'
import { useAppStore } from '@/store'
import {
  getBuiltinTheme,
  getSystemPrefersDark,
  resolveEffectiveTerminalAppearance
} from '@/lib/terminal-theme'
import { composeActiveTerminalTheme } from '@/components/terminal-pane/terminal-appearance'

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
  return {
    fg: pick(theme.foreground, DEFAULT_COLORS.fg),
    bg: pick(theme.background, DEFAULT_COLORS.bg),
    cursor: pick(theme.cursor, DEFAULT_COLORS.cursor),
    selection: pick(theme.selectionBackground, DEFAULT_COLORS.selection)
  }
}
