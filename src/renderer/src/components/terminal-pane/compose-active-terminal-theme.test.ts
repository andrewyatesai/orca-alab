import { describe, expect, it } from 'vitest'
import { composeActiveTerminalTheme } from './terminal-appearance'
import type { GlobalSettings } from '../../../../shared/types'

describe('composeActiveTerminalTheme', () => {
  function settingsWith(partial: Partial<GlobalSettings>): GlobalSettings {
    return {
      terminalColorOverrides: undefined,
      terminalCursorOpacity: undefined,
      terminalBackgroundOpacity: undefined,
      ...partial
    } as GlobalSettings
  }

  it('layers terminal scrollbar defaults under the base theme', () => {
    const base = { background: '#101010', foreground: '#fafafa', cursor: '#fafafa' }
    const result = composeActiveTerminalTheme(base, settingsWith({}))
    expect(result).toEqual({
      overviewRulerBorder: 'transparent',
      scrollbarSliderBackground: 'rgba(180, 180, 185, 0.4)',
      scrollbarSliderHoverBackground: 'rgba(180, 180, 185, 0.6)',
      scrollbarSliderActiveBackground: 'rgba(180, 180, 185, 0.8)',
      ...base
    })
  })

  it('lets the base theme override terminal scrollbar defaults', () => {
    const result = composeActiveTerminalTheme(
      {
        background: '#101010',
        overviewRulerBorder: '#222222',
        scrollbarSliderBackground: 'rgba(1, 2, 3, 0.4)'
      },
      settingsWith({})
    )

    expect(result!.overviewRulerBorder).toBe('#222222')
    expect(result!.scrollbarSliderBackground).toBe('rgba(1, 2, 3, 0.4)')
  })

  it('layers terminalColorOverrides on top of the base theme', () => {
    const base = { background: '#101010', foreground: '#fafafa' }
    const result = composeActiveTerminalTheme(
      base,
      settingsWith({ terminalColorOverrides: { foreground: '#00ff00' } })
    )
    expect(result!.foreground).toBe('#00ff00')
    expect(result!.background).toBe('#101010')
  })

  it('ignores the opacity settings — the aterm engine composites opaque colors', () => {
    // Composing rgba() here would be a dead store: the aterm theme seed drops
    // alpha. The colors must stay hex so nothing downstream sees a fake alpha.
    const base = { background: '#112233', cursor: '#ffffff' }
    const result = composeActiveTerminalTheme(
      base,
      settingsWith({ terminalBackgroundOpacity: 0.5, terminalCursorOpacity: 0.3 })
    )
    expect(result!.background).toBe('#112233')
    expect(result!.cursor).toBe('#ffffff')
  })

  it('returns null when given a null base theme', () => {
    expect(composeActiveTerminalTheme(null, settingsWith({}))).toBeNull()
  })

  // Why: settings persisted before the `bold` override was removed (#8595) still
  // carry the key; composition must tolerate it instead of failing schema-strict.
  it('tolerates a stale persisted bold override key', () => {
    const legacyOverrides = { foreground: '#00ff00', bold: '#ff00ff' }
    const result = composeActiveTerminalTheme(
      { background: '#101010', foreground: '#fafafa' },
      settingsWith({ terminalColorOverrides: legacyOverrides })
    )
    expect(result!.foreground).toBe('#00ff00')
    expect(result!.background).toBe('#101010')
  })
})
