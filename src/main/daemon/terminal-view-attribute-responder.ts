/**
 * View-attribute query answers (docs/reference/terminal-query-authority.md
 * §View-attribute bridge): OSC 4/10/11/12 color reports and the DSR ?996n
 * color-scheme report. The aterm headless engine has no theme service, so
 * these are computed from the renderer's pushed attribute snapshot with
 * per-PTY OSC-SET mutations layered on top — mirroring exactly what the
 * renderer reports for a visible pane.
 *
 * Engine-independent: this used to hang off @xterm/headless parser handlers,
 * but aterm exposes no such parser, so the owning TerminalModelQueryResponder
 * scans the byte stream itself and calls these methods. Reply emission is
 * gated by that caller's per-chunk forwarding window, so seeded/replayed
 * bytes and delivered chunks never produce a reply.
 */
import {
  formatXColorRgbSpec,
  parseXColorSpec,
  TERMINAL_VIEW_ANSI_COLOR_COUNT,
  type TerminalViewAttributes,
  type TerminalViewRgb
} from '../../shared/terminal-view-attributes'

export type TerminalViewAttributeResponderDeps = {
  /** Last renderer push, or null before the first push. Null means SILENCE
   *  for every view-attribute query — a fabricated default would resurrect
   *  the default-black OSC-11 bug (design invariant 3). */
  getBaseAttributes: () => TerminalViewAttributes | null
  /** Must already be replay/forwarding-window gated by the caller. */
  emitReply: (reply: string) => void
}

export type TerminalViewAttributeResponder = {
  /** Handle an OSC that reached the responder: 4/10/11/12 (query or SET) and
   *  104/110/111/112 (restore). `body` is everything after the OSC ident. */
  handleOsc: (id: number, body: string) => void
  /** Answer DSR ?996n from background/foreground relative luminance. */
  handleColorSchemeQuery: () => void
  /** A changed renderer attribute push replaces the whole palette, exactly
   *  like xterm's ThemeService `_setTheme` overwrites OSC-SET-mutated colors
   *  on a visible pane's theme apply. Identical re-pushes are filtered in
   *  main's store and never reach this. */
  clearColorOverrides: () => void
}

type SpecialColorSlot = 'foreground' | 'background' | 'cursor'

// OSC 10/11/12 stack extra params onto consecutive slots (xterm's
// _setOrReportSpecialColor): `OSC 10;?;?` reports foreground then background.
const SPECIAL_COLOR_SLOTS: SpecialColorSlot[] = ['foreground', 'background', 'cursor']
const SPECIAL_COLOR_IDENTS: Record<SpecialColorSlot, string> = {
  foreground: '10',
  background: '11',
  cursor: '12'
}

function isValidColorIndex(value: number): boolean {
  return value >= 0 && value < TERMINAL_VIEW_ANSI_COLOR_COUNT
}

// Mirror of xterm's rgb.relativeLuminance2 (common/Color.ts, WCAG formula) —
// the math CoreBrowserTerminal._reportColorScheme answers ?996n with.
function relativeLuminance([r, g, b]: TerminalViewRgb): number {
  const linear = (channel: number): number => {
    const c = channel / 255
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4)
  }
  return linear(r) * 0.2126 + linear(g) * 0.7152 + linear(b) * 0.0722
}

export function installTerminalViewAttributeResponder(
  deps: TerminalViewAttributeResponderDeps
): TerminalViewAttributeResponder {
  // Why per-instance maps: SET mutations are per PTY (one responder per PTY);
  // they die with the emulator at teardown, like every other model state.
  const ansiOverrides = new Map<number, TerminalViewRgb>()
  const specialOverrides = new Map<SpecialColorSlot, TerminalViewRgb>()

  const reportColor = (ident: string, rgb: TerminalViewRgb): void => {
    // Why ST (not BEL) and 16-bit channels: byte-for-byte parity with the
    // renderer's reply (CoreBrowserTerminal._handleColorEvent).
    deps.emitReply(`\x1b]${ident};${formatXColorRgbSpec(rgb)}\x1b\\`)
  }

  const handleSpecialColor = (body: string, offset: number): void => {
    const slots = body.split(';')
    for (let i = 0; i < slots.length; ++i, ++offset) {
      if (offset >= SPECIAL_COLOR_SLOTS.length) {
        break
      }
      const slot = SPECIAL_COLOR_SLOTS[offset]
      if (slots[i] === '?') {
        const base = deps.getBaseAttributes()
        if (base) {
          reportColor(SPECIAL_COLOR_IDENTS[slot], specialOverrides.get(slot) ?? base[slot])
        }
      } else {
        const rgb = parseXColorSpec(slots[i])
        if (rgb) {
          specialOverrides.set(slot, rgb)
        }
      }
    }
  }

  const handleAnsiColor = (body: string): void => {
    const slots = body.split(';')
    while (slots.length > 1) {
      const idx = slots.shift() as string
      const spec = slots.shift() as string
      if (!/^\d+$/.test(idx)) {
        continue
      }
      const index = Number.parseInt(idx, 10)
      if (!isValidColorIndex(index)) {
        continue
      }
      if (spec === '?') {
        const base = deps.getBaseAttributes()
        if (base) {
          reportColor(`4;${index}`, ansiOverrides.get(index) ?? base.ansi[index])
        }
      } else {
        const rgb = parseXColorSpec(spec)
        if (rgb) {
          ansiOverrides.set(index, rgb)
        }
      }
    }
  }

  // OSC 104/110/111/112 restore the themed color — dropping the override falls
  // back to the pushed base, the model twin of ThemeService.restoreColor.
  const restoreAnsi = (body: string): void => {
    if (!body) {
      ansiOverrides.clear()
      return
    }
    for (const slot of body.split(';')) {
      if (/^\d+$/.test(slot)) {
        ansiOverrides.delete(Number.parseInt(slot, 10))
      }
    }
  }

  const handleOsc = (id: number, body: string): void => {
    switch (id) {
      case 4:
        handleAnsiColor(body)
        break
      case 10:
        handleSpecialColor(body, 0)
        break
      case 11:
        handleSpecialColor(body, 1)
        break
      case 12:
        handleSpecialColor(body, 2)
        break
      case 104:
        restoreAnsi(body)
        break
      case 110:
        specialOverrides.delete('foreground')
        break
      case 111:
        specialOverrides.delete('background')
        break
      case 112:
        specialOverrides.delete('cursor')
        break
      default:
        break
    }
  }

  const handleColorSchemeQuery = (): void => {
    const base = deps.getBaseAttributes()
    if (!base) {
      return
    }
    // Why luminance and not base.colorSchemeMode: a visible xterm answers
    // ?996n from the relative luminance of the CURRENT (OSC-SET-mutated)
    // background vs foreground (CoreBrowserTerminal._reportColorScheme), so a
    // dark terminal theme in a light app mode still answers dark.
    const background = specialOverrides.get('background') ?? base.background
    const foreground = specialOverrides.get('foreground') ?? base.foreground
    const dark = relativeLuminance(background) < relativeLuminance(foreground)
    deps.emitReply(`\x1b[?997;${dark ? 1 : 2}n`)
  }

  return {
    handleOsc,
    handleColorSchemeQuery,
    clearColorOverrides: () => {
      ansiOverrides.clear()
      specialOverrides.clear()
    }
  }
}
