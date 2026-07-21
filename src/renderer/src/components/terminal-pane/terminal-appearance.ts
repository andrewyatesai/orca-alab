import type { IDisposable, IParser, ITheme } from '../../lib/pane-manager/aterm/terminal-types'
import { atermThemeColorsFromITheme } from '@/lib/pane-manager/aterm/aterm-theme-colors'
import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import type { GlobalSettings } from '../../../../shared/types'
import { resolveTerminalFontWeights } from '../../lib/git-wasm/terminal-fonts'
import {
  getBuiltinTheme,
  resolvePaneStyleOptions,
  resolveEffectiveTerminalAppearance
} from '@/lib/terminal-theme'
import { buildFontFamily } from './layout-serialization'
import { guardParserHandler } from './terminal-parser-handler-guard'
import { safeFit, safeFitAndThen } from '@/lib/pane-manager/pane-tree-ops'
import {
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity,
  resolveTerminalCursorInactiveStyle
} from '@/lib/pane-manager/pane-terminal-options'
import { getFitOverrideForPty } from '@/lib/pane-manager/mobile-fit-overrides'
import type { PtyTransport } from './pty-transport'
import type { EffectiveMacOptionAsAlt } from '@/lib/keyboard-layout/detect-option-as-alt'
import type { TerminalViewAttributes } from '../../../../shared/terminal-view-attributes'
import { publishTerminalViewAttributes } from './terminal-view-attributes-publisher'
import { normalizeTerminalLineHeight } from '../../../../shared/terminal-line-height-settings'
import { maybePushMode2031Flip } from './terminal-mode-2031-replies'

// Why Pick over a hand-rolled type: stays tied to xterm's canonical signature so upstream tightening surfaces here.
type Mode2031Parser = Pick<IParser, 'registerCsiHandler'>

type Mode2031HandlerDeps = {
  paneId: number
  parser: Mode2031Parser
  /** Called when a real (non-replayed) `CSI ?2031h` arrives, after the subscribe flag is set.
   *  A callback so the lifecycle hook keeps its transport-aware `pushMode2031ForPane` closure. */
  onSubscribe: () => void
  isReplaying: () => boolean
  paneMode2031: Map<number, boolean>
  paneLastThemeMode: Map<number, 'dark' | 'light'>
}

// Why a pure function: lets tests drive a real xterm parser end-to-end against the "random characters on restart" guard.
export function installMode2031Handlers(deps: Mode2031HandlerDeps): IDisposable[] {
  const hasMode2031 = (params: (number | number[])[]): boolean =>
    params.some((p) => (Array.isArray(p) ? p.includes(2031) : p === 2031))

  // Why return false: we only observe mode 2031; false lets xterm's built-in DEC handler still process compound sequences.
  return [
    deps.parser.registerCsiHandler(
      { prefix: '?', final: 'h' },
      guardParserHandler('csi-mode2031-subscribe', (params) => {
        if (hasMode2031(params)) {
          // Why gate on isReplaying: a restored buffer's replayed `?2031h` would push `?997;1n` into a fresh shell with no
          // TUI, which echoes it as literal text; pty-connection's guard covers only xterm auto-replies, not handler sends.
          // Return early (before recording the subscribe bit) so a later theme flip won't push into a shell that isn't subscribed.
          if (deps.isReplaying()) {
            return false
          }
          deps.paneMode2031.set(deps.paneId, true)
          deps.onSubscribe()
        }
        return false
      })
    ),
    // Why no replay guard here: we only push CSI 997 on subscribe; unsubscribe just clears map entries, so replay is harmless.
    deps.parser.registerCsiHandler(
      { prefix: '?', final: 'l' },
      guardParserHandler('csi-mode2031-unsubscribe', (params) => {
        if (hasMode2031(params)) {
          deps.paneMode2031.delete(deps.paneId)
          deps.paneLastThemeMode.delete(deps.paneId)
        }
        return false
      })
    )
  ]
}

// Why extracted: lets the settings preview compose the same theme without depending on PaneManager. Keep pure.
export function composeActiveTerminalTheme(
  baseTheme: ITheme | null,
  settings: Pick<GlobalSettings, 'terminalColorOverrides'>
): ITheme | null {
  if (!baseTheme) {
    return null
  }
  // Why transparent ruler border: scrollbar.width enables xterm's overview ruler, whose border would paint a bright line.
  // Why raised slider alpha: xterm's default (~0.2) is nearly invisible on dark bg. Before the spread so explicit theme wins.
  let theme: ITheme = {
    overviewRulerBorder: 'transparent',
    scrollbarSliderBackground: 'rgba(180, 180, 185, 0.4)',
    scrollbarSliderHoverBackground: 'rgba(180, 180, 185, 0.6)',
    scrollbarSliderActiveBackground: 'rgba(180, 180, 185, 0.8)',
    ...baseTheme
  }
  // Why: merge Ghostty color overrides atop the base theme so individual colors can be tweaked without losing the rest.
  if (settings.terminalColorOverrides) {
    theme = { ...theme, ...settings.terminalColorOverrides }
  }
  // terminalBackgroundOpacity / terminalCursorOpacity are intentionally NOT
  // composed into rgba() here: the engine applies them itself via
  // set_background_opacity/set_cursor_opacity (wired in applyAtermEngineSettings),
  // and the theme seed drops alpha (see aterm-theme-colors).
  return theme
}

/** Publishes composed terminal appearance at app start so hidden-at-launch PTYs can query OSC 10/11
 *  before any pane mounts (terminal-query-authority.md §Phase 6). Returns whether a publish went out. */
export function publishTerminalViewAttributesAtAppStart(
  settings: GlobalSettings | null | undefined,
  systemPrefersDark: boolean,
  send?: (attributes: TerminalViewAttributes) => boolean
): boolean {
  if (!settings) {
    return false
  }
  const appearance = resolveEffectiveTerminalAppearance(settings, systemPrefersDark)
  const baseTheme: ITheme | null = appearance.theme ?? getBuiltinTheme(appearance.themeName)
  const theme = composeActiveTerminalTheme(baseTheme, settings)
  return send !== undefined
    ? publishTerminalViewAttributes(theme, appearance.mode, settings, send)
    : publishTerminalViewAttributes(theme, appearance.mode, settings)
}

// Value equality over composed ITheme objects (flat string slots plus the extendedAnsi array); gates the options.theme write.
function composedTerminalThemesEqual(a: ITheme | undefined, b: ITheme): boolean {
  if (!a) {
    return false
  }
  if (a === b) {
    return true
  }
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  for (const key of keys) {
    if (key === 'extendedAnsi') {
      continue
    }
    if (a[key as keyof ITheme] !== b[key as keyof ITheme]) {
      return false
    }
  }
  const extA = a.extendedAnsi
  const extB = b.extendedAnsi
  if (!extA || !extB) {
    return extA === extB
  }
  return extA.length === extB.length && extA.every((value, i) => value === extB[i])
}

export function applyTerminalAppearance(
  manager: PaneManager,
  settings: GlobalSettings,
  systemPrefersDark: boolean,
  paneFontSizes: Map<number, number>,
  paneTransports: Map<number, PtyTransport>,
  effectiveMacOptionAsAlt: EffectiveMacOptionAsAlt,
  paneMode2031: Map<number, boolean>,
  paneLastThemeMode: Map<number, 'dark' | 'light'>
): void {
  const appearance = resolveEffectiveTerminalAppearance(settings, systemPrefersDark)
  const paneStyles = resolvePaneStyleOptions(settings)
  const baseTheme: ITheme | null = appearance.theme ?? getBuiltinTheme(appearance.themeName)
  const theme = composeActiveTerminalTheme(baseTheme, settings)
  // Publish composed appearance to main's hidden-PTY query responder — the only point it exists; deduped in the publisher.
  publishTerminalViewAttributes(theme, appearance.mode, settings)
  const paneBackground = theme?.background ?? '#000000'

  const terminalFontWeights = resolveTerminalFontWeights(settings.terminalFontWeight)

  for (const pane of manager.getPanes()) {
    // Why value-gated: writing options.theme rebuilds the palette, discarding TUI OSC 4/10/11/12 mutations; skip on no-op change.
    if (theme && !composedTerminalThemesEqual(pane.terminal.options.theme, theme)) {
      pane.terminal.options.theme = theme
      // aterm panes don't read xterm's options.theme — re-theme the canvas engine
      // in place so a theme change applies to OPEN aterm panes (preserving
      // scrollback) instead of only new ones.
      pane.atermController?.updateTheme(atermThemeColorsFromITheme(theme))
    }
    const cursorStyle = settings.terminalCursorStyle ?? 'block'
    pane.terminal.options.cursorStyle = cursorStyle
    pane.terminal.options.cursorInactiveStyle = resolveTerminalCursorInactiveStyle(cursorStyle)
    pane.terminal.options.cursorBlink = settings.terminalCursorBlink
    const paneSize = paneFontSizes.get(pane.id)
    pane.terminal.options.fontSize = paneSize ?? settings.terminalFontSize
    pane.terminal.options.fontFamily = buildFontFamily(settings.terminalFontFamily)
    pane.terminal.options.fontWeight = terminalFontWeights.fontWeight
    pane.terminal.options.fontWeightBold = terminalFontWeights.fontWeightBold
    pane.terminal.options.scrollSensitivity = normalizeTerminalScrollSensitivity(
      settings.terminalScrollSensitivity
    )
    pane.terminal.options.fastScrollSensitivity = normalizeTerminalFastScrollSensitivity(
      settings.terminalFastScrollSensitivity
    )
    // Why only 'true': 'left'/'right' are handled in the keydown policy, which needs Option composable at the xterm level.
    pane.terminal.options.macOptionIsMeta = effectiveMacOptionAsAlt === 'true'
    pane.terminal.options.lineHeight = normalizeTerminalLineHeight(settings.terminalLineHeight)
    // Live-apply the aterm engine settings that aren't read per-frame (ligatures,
    // scrollback depth, default cursor style) so toggling them updates this OPEN pane,
    // not just the next one (re-reads the live settings; no-op on a non-aterm pane).
    // Supersedes upstream's setPaneLigaturesEnabled (aterm has no ligatures addon).
    pane.atermController?.reapplyEngineSettings()
    const transport = paneTransports.get(pane.id)
    // Why: PTY is already at phone dimensions under a mobile-fit override — don't resize it back to desktop.
    const appearancePtyId = transport?.getPtyId()
    if (transport?.isConnected() && (!appearancePtyId || !getFitOverrideForPty(appearancePtyId))) {
      maybePushMode2031Flip(pane.id, appearance.mode, transport, paneMode2031, paneLastThemeMode)
      safeFitAndThen(pane, 'appearance-pty-resize', () => {
        const currentTransport = paneTransports.get(pane.id)
        if (
          currentTransport !== transport ||
          !transport.isConnected() ||
          transport.getPtyId() !== appearancePtyId
        ) {
          return
        }
        transport.resize(pane.terminal.cols, pane.terminal.rows)
      })
    } else {
      safeFit(pane)
    }
  }

  manager.setPaneStyleOptions({
    splitBackground: paneBackground,
    paneBackground,
    inactivePaneOpacity: paneStyles.inactivePaneOpacity,
    activePaneOpacity: paneStyles.activePaneOpacity,
    opacityTransitionMs: paneStyles.opacityTransitionMs,
    dividerThicknessPx: paneStyles.dividerThicknessPx,
    focusFollowsMouse: paneStyles.focusFollowsMouse,
    paddingX: settings.terminalPaddingX,
    paddingY: settings.terminalPaddingY
  })
}
