import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import type { ITheme } from '../pane-manager/aterm/terminal-types'
import { ORCA_DARK_THEME_NAME, ORCA_GRAPHITE_THEME_NAME, ORCA_TERMINAL_THEMES } from './orca-dark'
import { TERMINAL_THEME_CATALOG } from './index'

const HEX_COLOR = /^#[0-9a-f]{6}$/

// Self-contained WCAG relative-luminance contrast (the aterm-theme-colors
// implementation drags in the app store; the math is small enough to own here).
function channelToLinear(byte: number): number {
  const c = byte / 255
  return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)
}

function luminance(hex: string): number {
  const value = Number.parseInt(hex.slice(1), 16)
  const r = channelToLinear((value >> 16) & 0xff)
  const g = channelToLinear((value >> 8) & 0xff)
  const b = channelToLinear(value & 0xff)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrast(a: string, b: string): number {
  const la = luminance(a)
  const lb = luminance(b)
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

function readDarkModeTokenBlock(): string {
  const cssPath = fileURLToPath(new URL('../../assets/main.css', import.meta.url))
  const css = readFileSync(cssPath, 'utf8')
  const match = /\.dark\s*\{([^}]*)\}/.exec(css)
  if (!match) {
    throw new Error('main.css .dark token block not found')
  }
  return match[1]
}

const FAMILY = [ORCA_DARK_THEME_NAME, ORCA_GRAPHITE_THEME_NAME] as const

// The colored text tiers users actually read; black/brightBlack are handled
// separately (near-bg cell backgrounds / the dim tier).
const READABLE_KEYS: (keyof ITheme)[] = [
  'foreground',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
  'brightRed',
  'brightGreen',
  'brightYellow',
  'brightBlue',
  'brightMagenta',
  'brightCyan',
  'brightWhite'
]

describe('Orca-native dark theme family', () => {
  it('registers both family members in the terminal theme catalog', () => {
    for (const name of FAMILY) {
      expect(TERMINAL_THEME_CATALOG[name]).toBe(ORCA_TERMINAL_THEMES[name])
    }
  })

  it('uses only parseable #rrggbb values', () => {
    for (const name of FAMILY) {
      for (const [key, value] of Object.entries(ORCA_TERMINAL_THEMES[name])) {
        expect(value, `${name}.${key}`).toMatch(HEX_COLOR)
      }
    }
  })

  it('keys the family to the app dark-mode tokens in main.css', () => {
    const darkBlock = readDarkModeTokenBlock()
    const background = /--background:\s*(#[0-9a-fA-F]{6})/.exec(darkBlock)?.[1]
    const card = /--card:\s*(#[0-9a-fA-F]{6})/.exec(darkBlock)?.[1]
    // Orca Dark is flush with the app canvas; Graphite is one elevation step up.
    expect(ORCA_TERMINAL_THEMES[ORCA_DARK_THEME_NAME].background).toBe(background)
    expect(ORCA_TERMINAL_THEMES[ORCA_GRAPHITE_THEME_NAME].background).toBe(card)
  })

  it('keeps every readable color at or above 4.5:1 on its background', () => {
    for (const name of FAMILY) {
      const theme = ORCA_TERMINAL_THEMES[name]
      for (const key of READABLE_KEYS) {
        const color = theme[key] as string
        expect(
          contrast(color, theme.background as string),
          `${name}.${key} vs background`
        ).toBeGreaterThanOrEqual(4.5)
      }
    }
  })

  it('keeps the dim tier (brightBlack) legible on both backgrounds', () => {
    for (const name of FAMILY) {
      const theme = ORCA_TERMINAL_THEMES[name]
      // Dim-but-readable: stricter than typical terminal defaults; the engine's
      // minimum-contrast floor guards the pathological per-cell cases.
      expect(
        contrast(theme.brightBlack as string, theme.background as string),
        `${name}.brightBlack`
      ).toBeGreaterThanOrEqual(3.5)
    }
  })

  it('keeps the selection visible and its text readable', () => {
    for (const name of FAMILY) {
      const theme = ORCA_TERMINAL_THEMES[name]
      expect(
        contrast(theme.selectionBackground as string, theme.background as string),
        `${name} selection vs background`
      ).toBeGreaterThanOrEqual(3)
      expect(
        contrast(theme.selectionForeground as string, theme.selectionBackground as string),
        `${name} selected text`
      ).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('makes the cursor read as the foreground over a background-matched accent', () => {
    for (const name of FAMILY) {
      const theme = ORCA_TERMINAL_THEMES[name]
      expect(theme.cursor).toBe(theme.foreground)
      expect(theme.cursorAccent).toBe(theme.background)
    }
  })
})
