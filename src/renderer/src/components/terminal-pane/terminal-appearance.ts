import type { IDisposable, IParser, ITheme } from '../../lib/pane-manager/aterm/terminal-types'
import { atermThemeColorsFromITheme } from '@/lib/pane-manager/aterm/aterm-theme-colors'
import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import type { GlobalSettings } from '../../../../shared/types'
import { mode2031SequenceFor } from '../../../../shared/terminal-color-scheme-protocol'
import { resolveTerminalFontWeights } from '../../lib/git-wasm/terminal-fonts'
import {
  getBuiltinTheme,
  resolvePaneStyleOptions,
  resolveEffectiveTerminalAppearance
} from '@/lib/terminal-theme'
import { buildFontFamily } from './layout-serialization'
import { captureScrollState, restoreScrollState, safeFit } from '@/lib/pane-manager/pane-tree-ops'
import {
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity,
  resolveTerminalCursorInactiveStyle
} from '@/lib/pane-manager/pane-terminal-options'
import { getFitOverrideForPty } from '@/lib/pane-manager/mobile-fit-overrides'
import type { PtyTransport } from './pty-transport'
import type { EffectiveMacOptionAsAlt } from '@/lib/keyboard-layout/detect-option-as-alt'

export { mode2031SequenceFor }

// Why Pick<IParser, ...> over a hand-rolled structural type: keeps the helper
// tied to xterm's canonical signature so any upstream tightening (added
// fields on IFunctionIdentifier, narrower param type) surfaces here instead
// of silently accepting a stale shape.
type Mode2031Parser = Pick<IParser, 'registerCsiHandler'>

type Mode2031HandlerDeps = {
  paneId: number
  parser: Mode2031Parser
  /** Called when a real (non-replayed) `CSI ?2031h` arrives, after the
   *  subscribe flag has been set. Kept as a callback so the lifecycle hook
   *  can keep its transport-aware `pushMode2031ForPane` closure intact. */
  onSubscribe: () => void
  isReplaying: () => boolean
  paneMode2031: Map<number, boolean>
  paneLastThemeMode: Map<number, 'dark' | 'light'>
}

// Why split out from the lifecycle hook: the CSI handlers are the defense
// against a restored xterm buffer pushing `\x1b[?997;1n` into the fresh zsh
// on cold restore (the "random characters on restart" bug). Keeping them in
// a pure function lets the tests drive a real xterm parser end-to-end so we
// catch regressions in the parser-path guard, not just a mock.
export function installMode2031Handlers(deps: Mode2031HandlerDeps): IDisposable[] {
  const hasMode2031 = (params: (number | number[])[]): boolean =>
    params.some((p) => (Array.isArray(p) ? p.includes(2031) : p === 2031))

  // Why return false from both handlers: we only observe mode 2031.
  // Returning false lets xterm's built-in DEC private mode handler
  // continue processing the same sequence, so compound sequences like
  // `CSI ?25;2031h` still update cursor visibility correctly.
  return [
    deps.parser.registerCsiHandler({ prefix: '?', final: 'h' }, (params) => {
      if (hasMode2031(params)) {
        // Why: a restored xterm buffer may contain `CSI ?2031h` emitted by
        // the previous session's TUI (e.g. Claude Code). Replaying that
        // buffer runs this handler, and without the guard we'd push
        // `CSI ?997;1n` via transport.sendInput into a fresh shell that has
        // no TUI consuming it — zsh then echoes the literal escape sequence
        // onto the prompt. The replay guard in pty-connection.ts only covers
        // xterm's own onData auto-replies, not handler-triggered sends, so
        // gate explicitly here. We also skip recording the subscribe bit:
        // the fresh shell is not actually subscribed, so a later theme flip
        // must not push either. A real TUI that starts up after restore will
        // re-emit `?2031h` itself and register normally.
        //
        // Why this broad guard is safe across all replay sources: the only
        // replay path that can carry raw `?2031h` is cold-restore scrollback
        // (pty-connection.ts), which is disk-replayed PTY output against a
        // fresh shell — the case this guard targets. Daemon snapshot payloads
        // (`rehydrateSequences + SerializeAddon.serialize()`) and persisted
        // scrollback (`SerializeAddon.serialize()`) never contain `?2031`:
        // SerializeAddon's _serializeModes whitelists only ?1h/?66h/?2004h/
        // [4h/?6h/?45h/?1004h/?7l/mouse modes/?25l, and buildRehydrateSequences
        // emits only ?1049h/?2004h/?1h/mouse reporting modes. If xterm ever
        // adds ?2031 to that whitelist, this guard would start suppressing
        // legitimate subscribes during snapshot reattach — revisit then.
        if (deps.isReplaying()) {
          return false
        }
        deps.paneMode2031.set(deps.paneId, true)
        deps.onSubscribe()
      }
      return false
    }),
    // Why no replay guard on the unsubscribe branch: clearing stale bookkeeping
    // is harmless. We only push CSI 997 on subscribe, never on unsubscribe, so
    // even if a cold-restore replay carries `?2031l`, this handler just deletes
    // map entries that a later real `?2031h` will re-populate normally.
    deps.parser.registerCsiHandler({ prefix: '?', final: 'l' }, (params) => {
      if (hasMode2031(params)) {
        deps.paneMode2031.delete(deps.paneId)
        deps.paneLastThemeMode.delete(deps.paneId)
      }
      return false
    })
  ]
}

// Gate on actual mode flip so font/size/opacity tweaks — which also re-run
// applyTerminalAppearance — don't spam subscribed TUIs with CSI 997. The
// subscribe/last-mode maps are mutated in place so callers share state with
// the lifecycle hook's seed path.
export function maybePushMode2031Flip(
  paneId: number,
  mode: 'dark' | 'light',
  transport: Pick<PtyTransport, 'isConnected' | 'sendInput'>,
  paneMode2031: Map<number, boolean>,
  paneLastThemeMode: Map<number, 'dark' | 'light'>
): boolean {
  if (!transport.isConnected()) {
    return false
  }
  if (!paneMode2031.get(paneId)) {
    return false
  }
  if (paneLastThemeMode.get(paneId) === mode) {
    return false
  }
  if (!transport.sendInput(mode2031SequenceFor(mode))) {
    return false
  }
  paneLastThemeMode.set(paneId, mode)
  return true
}

// Why: extracted from applyTerminalAppearance so the settings preview can
// derive the same composed theme without depending on PaneManager. Keep
// pure — no DOM, no manager, no side effects.
export function composeActiveTerminalTheme(
  baseTheme: ITheme | null,
  settings: Pick<GlobalSettings, 'terminalColorOverrides'>
): ITheme | null {
  if (!baseTheme) {
    return null
  }
  // Why: setting scrollbar.width enables xterm's overview ruler, whose border
  // defaults to the foreground color and paints a bright vertical line beside
  // the scrollbar. We only want the slimmer scrollbar, not the ruler chrome.
  // Why: xterm's default slider alpha (~0.2) is nearly invisible on dark
  // backgrounds; raise the contrast so the thumb reads. Placed before the
  // spread so an explicit theme value still wins.
  let theme: ITheme = {
    overviewRulerBorder: 'transparent',
    scrollbarSliderBackground: 'rgba(180, 180, 185, 0.4)',
    scrollbarSliderHoverBackground: 'rgba(180, 180, 185, 0.6)',
    scrollbarSliderActiveBackground: 'rgba(180, 180, 185, 0.8)',
    ...baseTheme
  }
  // Why: merge user-imported Ghostty color overrides on top of the resolved
  // base theme so individual colors can be tweaked without losing the rest.
  if (settings.terminalColorOverrides) {
    theme = { ...theme, ...settings.terminalColorOverrides }
  }
  // terminalBackgroundOpacity / terminalCursorOpacity are intentionally NOT
  // composed into rgba() here: the engine applies them itself via
  // set_background_opacity/set_cursor_opacity (wired in applyAtermEngineSettings),
  // and the theme seed drops alpha (see aterm-theme-colors).
  return theme
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
  const paneBackground = theme?.background ?? '#000000'

  const terminalFontWeights = resolveTerminalFontWeights(settings.terminalFontWeight)

  for (const pane of manager.getPanes()) {
    if (theme) {
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
    // Why: xterm's macOptionIsMeta only flips on the 'true' mode. 'left' and
    // 'right' are handled in the keydown policy (terminal-shortcut-policy),
    // which needs Option to stay composable at the xterm level for the
    // non-Meta side. Treating only 'true' as Meta here matches the pre-
    // detection behavior; the detection layer simply decides *what* value
    // `effectiveMacOptionAsAlt` carries.
    pane.terminal.options.macOptionIsMeta = effectiveMacOptionAsAlt === 'true'
    pane.terminal.options.lineHeight = settings.terminalLineHeight
    // Live-apply the aterm engine settings that aren't read per-frame (ligatures,
    // scrollback depth, default cursor style) so toggling them updates this OPEN pane,
    // not just the next one (re-reads the live settings; no-op on a non-aterm pane).
    pane.atermController?.reapplyEngineSettings()
    try {
      const state = captureScrollState(pane.terminal)
      safeFit(pane)
      restoreScrollState(pane.terminal, state)
    } catch {
      /* ignore */
    }
    const transport = paneTransports.get(pane.id)
    // Why: skip PTY resize when a mobile-fit override is active — the PTY
    // is already at the correct phone dimensions and must not be resized
    // back to desktop dimensions by an appearance change.
    const appearancePtyId = transport?.getPtyId()
    if (transport?.isConnected() && (!appearancePtyId || !getFitOverrideForPty(appearancePtyId))) {
      transport.resize(pane.terminal.cols, pane.terminal.rows)
      maybePushMode2031Flip(pane.id, appearance.mode, transport, paneMode2031, paneLastThemeMode)
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
