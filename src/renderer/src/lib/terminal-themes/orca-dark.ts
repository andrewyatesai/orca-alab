import type { ITheme } from '../pane-manager/aterm/terminal-types'
import type { TerminalThemeMap } from './types'

/** The Orca-native dark family's flagship name — exported so the future
 *  new-profile default flip / migration offer reference one constant. */
export const ORCA_DARK_THEME_NAME = 'Orca Dark'
export const ORCA_GRAPHITE_THEME_NAME = 'Orca Graphite'

// One ANSI ramp for the whole family, keyed to the app's Tailwind token family
// (main.css uses --color-blue-400/-300, --color-violet-400, etc.): normal = 400
// tier, bright = 300 tier, so terminal accents read as the same design system as
// the chrome. brightGreen intentionally equals --status-success (#86efac) and
// blue equals --terminal-pane-locate (blue-400). brightBlack is the dim tier
// (--ring, #737373); the engine's minimum-contrast floor guards its worst cases.
const ORCA_ANSI_RAMP: Omit<ITheme, 'background' | 'foreground' | 'black'> = {
  red: '#f87171',
  green: '#4ade80',
  yellow: '#facc15',
  blue: '#60a5fa',
  magenta: '#c084fc',
  cyan: '#22d3ee',
  white: '#d4d4d4',
  brightBlack: '#737373',
  brightRed: '#fca5a5',
  brightGreen: '#86efac',
  brightYellow: '#fde047',
  brightBlue: '#93c5fd',
  brightMagenta: '#d8b4fe',
  brightCyan: '#67e8f9',
  brightWhite: '#fafafa'
}

// Selection is slate-500: visibly bluer than the neutral-700 grays agent CLIs
// draw instruction blocks with (the exact blend-in the Ghostty default's raised
// selection fixed), and #fafafa text on it clears the 4.5:1 WCAG floor.
const ORCA_SELECTION = {
  selectionBackground: '#64748b',
  selectionForeground: '#fafafa'
}

export const ORCA_TERMINAL_THEMES: TerminalThemeMap = {
  // Flush with the app chrome: background is the dark-mode --background token,
  // foreground the --foreground token, so the terminal and the app read as one
  // surface instead of a floating widget rectangle.
  [ORCA_DARK_THEME_NAME]: {
    background: '#0a0a0a',
    foreground: '#fafafa',
    cursor: '#fafafa',
    cursorAccent: '#0a0a0a',
    ...ORCA_SELECTION,
    // ANSI black sits one step above the canvas (--card) so black-background
    // cells still separate from the seamless page background.
    black: '#171717',
    ...ORCA_ANSI_RAMP
  },

  // One elevation step up: background is the dark-mode --card/--sidebar token,
  // for users who want the terminal to read as a lifted panel, not the canvas.
  [ORCA_GRAPHITE_THEME_NAME]: {
    background: '#171717',
    foreground: '#fafafa',
    cursor: '#fafafa',
    cursorAccent: '#171717',
    ...ORCA_SELECTION,
    // One step above the graphite surface (--secondary), mirroring Orca Dark's
    // black-sits-above-background rule.
    black: '#262626',
    ...ORCA_ANSI_RAMP
  }
}
