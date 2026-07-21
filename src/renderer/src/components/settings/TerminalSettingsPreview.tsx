import { useEffect, useMemo, useRef, useState } from 'react'
import { Moon, Sun } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { composeActiveTerminalTheme } from '@/components/terminal-pane/terminal-appearance'
import { atermThemeColorsFromITheme } from '@/lib/pane-manager/aterm/aterm-theme-colors'
import { clampNumber, resolveEffectiveTerminalAppearance } from '@/lib/terminal-theme'
import { PREVIEW_BUFFER } from './terminal-preview-content'
import {
  createTerminalPreviewAtermEngine,
  type TerminalPreviewAtermEngine
} from './terminal-preview-aterm-engine'
import { SettingsSwitch } from './SettingsFormControls'
import type { GlobalSettings } from '../../../../shared/types'
import { translate } from '@/i18n/i18n'

// Why: pinned so PREVIEW_BUFFER never wraps; 36 cols fits the 32-char longest line + margin on the aterm canvas in the
// xl right-column at the default 14px font (larger fonts clip past the box edge, which beats wrapping mid-content).
const PREVIEW_COLS = 36
const PREVIEW_ROWS = 15

// Why: real aterm canvas on the left, color-only stub pane on the right; 40px is wide enough to read the inactive-pane
// opacity dim, narrow enough not to crowd content.
const STUB_PANE_PX = 40

type PreviewMode = 'dark' | 'light'

// DECSCUSR sequence for the user's cursor style/blink so the engine renders the
// chosen shape on the trailing prompt. aterm tracks cursor_style from this.
function cursorStyleSequence(style: GlobalSettings['terminalCursorStyle'], blink: boolean): string {
  const base = style === 'bar' ? 5 : style === 'underline' ? 3 : 1
  // DECSCUSR: odd = blinking, even = steady (1/2 block, 3/4 underline, 5/6 bar).
  const code = blink ? base : base + 1
  return `\x1b[${code} q`
}

type TerminalSettingsPreviewProps = {
  title: string
  description?: string
  settings: GlobalSettings
  systemPrefersDark: boolean
  /** Set by the font picker while the user hovers a dropdown option. Currently
   *  inert in the preview: aterm renders with its own baked renderer font (+ OS
   *  fallbacks), so a font-family hover can't be previewed. Kept on the API for
   *  the picker wiring and so font SIZE hover still works via `settings`. */
  previewFontFamily?: string | null
  /** Force the preview into this mode regardless of app settings; hides the in-header theme toggle when set. */
  modeOverride?: PreviewMode
  /** Render a Moon/Sun header toggle to flip the preview theme without changing the app theme. Ignored when `modeOverride` is set. */
  showThemeToggle?: boolean
}

function resolveAppMode(
  settings: Pick<GlobalSettings, 'theme'>,
  systemPrefersDark: boolean
): PreviewMode {
  if (settings.theme === 'system') {
    return systemPrefersDark ? 'dark' : 'light'
  }
  return settings.theme
}

export function TerminalSettingsPreview({
  title,
  description,
  settings,
  systemPrefersDark,
  // previewFontFamily is intentionally not consumed — see the prop's doc comment
  // (aterm uses a fixed renderer font, so a font-family hover can't be previewed).
  modeOverride,
  showThemeToggle
}: TerminalSettingsPreviewProps): React.JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const engineRef = useRef<TerminalPreviewAtermEngine | null>(null)
  const skipInitialFontRef = useRef(false)
  const skipInitialThemeRef = useRef(false)

  // Why: lazy-init from the active app theme; after mount the toggle is independent of later app-theme changes.
  const [togglePreviewMode, setTogglePreviewMode] = useState<PreviewMode>(() =>
    resolveAppMode(settings, systemPrefersDark)
  )
  const [previewPaneDividerVisible, setPreviewPaneDividerVisible] = useState(false)

  // Why: recomputed each render so plain previews (no override/toggle) track live app-theme changes.
  const effectiveMode: PreviewMode =
    modeOverride ??
    (showThemeToggle ? togglePreviewMode : resolveAppMode(settings, systemPrefersDark))

  // Why: reuse the live-pane resolver so divider color, theme palette, and dark/light variant rules stay in lockstep.
  // Why: list resolveEffectiveTerminalAppearance's inputs explicitly so unrelated changes (font, cursor) don't re-derive.
  const appearance = useMemo(
    () =>
      resolveEffectiveTerminalAppearance({ ...settings, theme: effectiveMode }, systemPrefersDark),
    // oxlint-disable-next-line react-hooks/exhaustive-deps
    [
      effectiveMode,
      settings.terminalThemeDark,
      settings.terminalThemeLight,
      settings.terminalCustomThemes,
      settings.terminalUseSeparateLightTheme,
      settings.terminalDividerColorDark,
      settings.terminalDividerColorLight,
      systemPrefersDark
    ]
  )

  // Why: list composeActiveTerminalTheme inputs explicitly so font/cursor changes don't trigger a buffer rewrite.
  const composedTheme = useMemo(
    () => composeActiveTerminalTheme(appearance.theme, settings),
    // oxlint-disable-next-line react-hooks/exhaustive-deps
    [
      appearance,
      settings.terminalColorOverrides,
      settings.terminalBackgroundOpacity,
      settings.terminalCursorOpacity
    ]
  )

  const dividerThicknessPx = clampNumber(settings.terminalDividerThicknessPx, 1, 32)
  const inactivePaneOpacity = clampNumber(settings.terminalInactivePaneOpacity, 0, 1)
  const paneBackground = composedTheme?.background ?? '#000'

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) {
      return
    }
    skipInitialFontRef.current = true
    skipInitialThemeRef.current = true
    let cancelled = false
    // Seed the buffer with the user's cursor style (DECSCUSR) so the trailing
    // prompt shows the chosen shape; aterm reads cursor_style from the stream.
    const buffer =
      cursorStyleSequence(settings.terminalCursorStyle, settings.terminalCursorBlink) +
      PREVIEW_BUFFER
    const themeColors = composedTheme
      ? atermThemeColorsFromITheme(composedTheme)
      : atermThemeColorsFromITheme({})
    void createTerminalPreviewAtermEngine(
      {
        canvas,
        cols: PREVIEW_COLS,
        rows: PREVIEW_ROWS,
        fontPx: settings.terminalFontSize,
        themeColors,
        buffer
      },
      () => cancelled
    ).then((engine) => {
      if (!engine) {
        return
      }
      if (cancelled) {
        engine.dispose()
        return
      }
      engineRef.current = engine
    })

    return () => {
      cancelled = true
      engineRef.current?.dispose()
      engineRef.current = null
    }
    // Why empty deps: mount effect runs once; later setting changes flow through the dedicated font/theme effects below.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Font size / cursor changes re-rasterize + re-feed the engine in place.
  useEffect(() => {
    const engine = engineRef.current
    if (!engine) {
      return
    }
    if (skipInitialFontRef.current) {
      skipInitialFontRef.current = false
      return
    }
    const buffer =
      cursorStyleSequence(settings.terminalCursorStyle, settings.terminalCursorBlink) +
      PREVIEW_BUFFER
    engine.applyFontAndBuffer(settings.terminalFontSize, buffer)
  }, [settings.terminalFontSize, settings.terminalCursorStyle, settings.terminalCursorBlink])

  // Theme changes re-theme the live engine in place and repaint.
  useEffect(() => {
    const engine = engineRef.current
    if (!engine || !composedTheme) {
      return
    }
    if (skipInitialThemeRef.current) {
      skipInitialThemeRef.current = false
      return
    }
    engine.applyTheme(atermThemeColorsFromITheme(composedTheme))
  }, [composedTheme])

  const showToggle = showThemeToggle && modeOverride === undefined

  return (
    <Card className="gap-4 overflow-hidden py-0">
      <CardHeader className="gap-0 border-b border-border/50 px-4 py-3 !pb-3">
        <div className="flex min-h-7 items-center justify-between gap-3">
          <div className="min-w-0 space-y-1">
            <CardTitle className="text-sm">{title}</CardTitle>
            {description ? <CardDescription>{description}</CardDescription> : null}
          </div>
          <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
            <div className="flex items-center gap-2 rounded-md border border-border/50 bg-background/40 px-2 py-1">
              <span className="text-xs font-medium text-muted-foreground">
                {translate(
                  'auto.components.settings.TerminalSettingsPreview.50419052fe',
                  'Pane divider'
                )}
              </span>
              <SettingsSwitch
                checked={previewPaneDividerVisible}
                onChange={() => setPreviewPaneDividerVisible((visible) => !visible)}
                ariaLabel={translate(
                  'auto.components.settings.TerminalSettingsPreview.f8931d407d',
                  'Show pane divider in preview'
                )}
              />
            </div>
            {showToggle ? (
              <div
                className="flex gap-0.5 rounded-md border border-border/50 p-0.5"
                role="group"
                aria-label={translate(
                  'auto.components.settings.TerminalSettingsPreview.2c248fcc27',
                  'Preview theme'
                )}
              >
                {(['dark', 'light'] as const).map((mode) => (
                  <button
                    key={mode}
                    type="button"
                    onClick={() => setTogglePreviewMode(mode)}
                    aria-pressed={togglePreviewMode === mode}
                    aria-label={translate(
                      'auto.components.settings.TerminalSettingsPreview.a63953a48a',
                      'Preview {{value0}} theme',
                      { value0: mode }
                    )}
                    title={translate(
                      'auto.components.settings.TerminalSettingsPreview.a63953a48a',
                      'Preview {{value0}} theme',
                      { value0: mode }
                    )}
                    className={`rounded-sm p-1 transition-colors ${
                      togglePreviewMode === mode
                        ? 'bg-accent text-accent-foreground'
                        : 'text-muted-foreground hover:text-foreground'
                    }`}
                  >
                    {mode === 'dark' ? <Moon className="size-3.5" /> : <Sun className="size-3.5" />}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </div>
      </CardHeader>
      <CardContent className="px-4 pb-4">
        {/* Why: stub pane on the right keeps inactive-pane opacity visible; divider is opt-in to keep the default preview clean. */}
        <div className="flex h-[300px] flex-col overflow-hidden rounded-md border border-border/50">
          <div className="flex min-h-0 flex-1 overflow-hidden" aria-hidden="true">
            <div
              className="min-w-0 flex-1 overflow-hidden p-2"
              style={{ backgroundColor: paneBackground }}
              tabIndex={-1}
            >
              <canvas ref={canvasRef} />
            </div>
            {previewPaneDividerVisible ? (
              <div
                className="shrink-0"
                style={{
                  width: `${dividerThicknessPx}px`,
                  backgroundColor: appearance.dividerColor
                }}
              />
            ) : null}
            <div
              className="shrink-0"
              style={{
                width: `${STUB_PANE_PX}px`,
                backgroundColor: paneBackground,
                opacity: inactivePaneOpacity
              }}
            />
          </div>
        </div>
      </CardContent>
    </Card>
  )
}
