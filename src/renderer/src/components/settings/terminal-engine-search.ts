import type { SettingsSearchEntry } from './settings-search'
import { translate } from '@/i18n/i18n'
import { translateSearchKeyword } from './settings-search-keywords'
import { createLocalizedCatalog } from '@/i18n/localized-catalog'

// Search metadata for the Terminal Engine section (the dedicated aterm panel).
// Only the entries UNIQUE to this section live here; the typography/clipboard
// settings it re-surfaces already match through their home sections' entries.

const getTerminalEngineSearchEntryCatalog = createLocalizedCatalog((): SettingsSearchEntry[] => [
  {
    title: translate('auto.components.settings.terminal.engine.search.sparkle', 'Sparkle Words'),
    description: translate(
      'auto.components.settings.terminal.engine.search.sparkleDescription',
      'Animate lexicon words (orca splashes, cats, supernovas) in terminal output.'
    ),
    keywords: [
      ...translateSearchKeyword('auto.components.settings.terminal.search.f66a7cf715', 'terminal'),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.engine.search.kwEffects',
        'effects'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.engine.search.kwSparkle',
        'sparkle'
      ),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.engine.search.kwAnimation',
        'animation'
      )
    ]
  },
  {
    title: translate('auto.components.settings.terminal.engine.search.glow', 'Cursor Glow'),
    description: translate(
      'auto.components.settings.terminal.engine.search.glowDescription',
      'An aurora of light in the terminal cursor’s wake.'
    ),
    keywords: [
      ...translateSearchKeyword('auto.components.settings.terminal.search.f66a7cf715', 'terminal'),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.engine.search.kwCursor',
        'cursor'
      ),
      ...translateSearchKeyword('auto.components.settings.terminal.engine.search.kwGlow', 'glow'),
      ...translateSearchKeyword('auto.components.settings.terminal.engine.search.kwTrail', 'trail')
    ]
  },
  {
    title: translate('auto.components.settings.terminal.engine.search.engine', 'Terminal Engine'),
    description: translate(
      'auto.components.settings.terminal.engine.search.engineDescription',
      'The aterm engine: effects, rendering, text shaping, scrollback, input, and clipboard security.'
    ),
    keywords: [
      ...translateSearchKeyword('auto.components.settings.terminal.search.f66a7cf715', 'terminal'),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.engine.search.kwEngine',
        'engine'
      ),
      ...translateSearchKeyword('auto.components.settings.terminal.engine.search.kwAterm', 'aterm'),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.engine.search.kwRenderer',
        'renderer'
      )
    ]
  }
])

export function getTerminalEngineSearchEntries(): SettingsSearchEntry[] {
  return getTerminalEngineSearchEntryCatalog()
}
