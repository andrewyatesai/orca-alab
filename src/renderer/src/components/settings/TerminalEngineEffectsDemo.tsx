import { useEffect, useMemo, useRef } from 'react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { composeActiveTerminalTheme } from '@/components/terminal-pane/terminal-appearance'
import { atermThemeColorsFromITheme } from '@/lib/pane-manager/aterm/aterm-theme-colors'
import { readAtermEffectsConfig } from '@/lib/pane-manager/aterm/aterm-effects-settings'
import { resolveEffectiveTerminalAppearance } from '@/lib/terminal-theme'
import {
  createTerminalEngineEffectsDemoEngine,
  type TerminalEngineEffectsDemoEngine
} from './terminal-engine-effects-demo-engine'
import type { GlobalSettings } from '../../../../shared/types'
import { translate } from '@/i18n/i18n'

// Live effects preview: a REAL aterm CPU engine instance (same pattern as
// TerminalSettingsPreview's embedded engine) typing a small script on a loop so
// the enabled effects — cursor glow, sparkle words — animate exactly as they
// would in a live pane, and settle to zero work when done.

// Fixed demo grid: wide enough for the longest script line + splash headroom,
// short enough to stay a compact card.
const DEMO_COLS = 44
const DEMO_ROWS = 8

type TerminalEngineEffectsDemoProps = {
  settings: GlobalSettings
  systemPrefersDark: boolean
}

export function TerminalEngineEffectsDemo({
  settings,
  systemPrefersDark
}: TerminalEngineEffectsDemoProps): React.JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const engineRef = useRef<TerminalEngineEffectsDemoEngine | null>(null)
  const skipInitialEffectsRef = useRef(false)
  const skipInitialThemeRef = useRef(false)

  // Reuse the live-pane theme resolvers so the demo matches real panes.
  const appearance = useMemo(
    () => resolveEffectiveTerminalAppearance(settings, systemPrefersDark),
    [settings, systemPrefersDark]
  )
  const composedTheme = useMemo(
    () => composeActiveTerminalTheme(appearance.theme, settings),
    [appearance, settings]
  )

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) {
      return
    }
    skipInitialEffectsRef.current = true
    skipInitialThemeRef.current = true
    let cancelled = false
    const themeColors = atermThemeColorsFromITheme(composedTheme ?? {})
    void createTerminalEngineEffectsDemoEngine(
      {
        canvas,
        cols: DEMO_COLS,
        rows: DEMO_ROWS,
        fontPx: settings.terminalFontSize,
        themeColors,
        effects: readAtermEffectsConfig()
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
    // Mount-once: live effects/theme changes flow through the effects below.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Live-apply the effect toggles to the running demo engine.
  useEffect(() => {
    if (skipInitialEffectsRef.current) {
      skipInitialEffectsRef.current = false
      return
    }
    engineRef.current?.applyEffects(readAtermEffectsConfig())
  }, [
    settings.terminalEffectsSparkleWords,
    settings.terminalEffectsSparkleProfanity,
    settings.terminalEffectsSparkleFeline,
    settings.terminalEffectsSparkleOrca,
    settings.terminalEffectsSparkleEmphasis,
    settings.terminalEffectsCursorGlow,
    settings.terminalEffectsCursorGlowStyle
  ])

  useEffect(() => {
    if (skipInitialThemeRef.current) {
      skipInitialThemeRef.current = false
      return
    }
    if (composedTheme) {
      engineRef.current?.applyTheme(atermThemeColorsFromITheme(composedTheme))
    }
  }, [composedTheme])

  return (
    <Card className="gap-0 overflow-hidden py-0">
      <CardHeader className="gap-0 border-b border-border/50 px-4 py-3 !pb-3">
        <CardTitle className="text-sm">
          {translate(
            'auto.components.settings.TerminalEnginePane.effectsDemo.title',
            'Live Effects Preview'
          )}
        </CardTitle>
        <CardDescription>
          {translate(
            'auto.components.settings.TerminalEnginePane.effectsDemo.description',
            'A real terminal engine typing on a loop — enabled effects animate here exactly as in your panes.'
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="p-4">
        <div
          className="overflow-hidden rounded-md border border-border/50 p-2"
          style={{ background: composedTheme?.background ?? '#000' }}
        >
          <canvas ref={canvasRef} aria-hidden="true" />
        </div>
      </CardContent>
    </Card>
  )
}
