import { useState } from 'react'
import { ArrowDown, ArrowUp, Plus, X } from 'lucide-react'
import type { GlobalSettings } from '../../../../shared/types'
import { Button } from '../ui/button'
import { FontAutocomplete, SettingsRow } from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { getTerminalTypographySearchEntries } from './terminal-typography-search'
import { translate } from '@/i18n/i18n'

type TerminalFallbackFontsRowProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
  suggestions: string[]
  onRequestSuggestions?: () => void
  forceVisible?: boolean
}

/** Ordered user font-fallback stack (terminalFontFallbackFamilies): consulted
 *  after the primary font and before Orca's automatic OS fallbacks. Families are
 *  staged in the system-font autocomplete and committed with Add (the
 *  autocomplete's onChange fires per keystroke, so it cannot commit directly);
 *  chips reorder with the arrow buttons and remove per entry. */
export function TerminalFallbackFontsRow({
  settings,
  updateSettings,
  suggestions,
  onRequestSuggestions,
  forceVisible = false
}: TerminalFallbackFontsRowProps): React.JSX.Element {
  const families = settings.terminalFontFallbackFamilies ?? []
  const [draft, setDraft] = useState('')
  const searchEntry = getTerminalTypographySearchEntries()[2]

  const commit = (next: string[]): void => {
    updateSettings({ terminalFontFallbackFamilies: next })
  }

  const addDraft = (): void => {
    const name = draft.trim()
    setDraft('')
    if (!name) {
      return
    }
    // Why: a duplicate entry can't change glyph resolution (the earlier position wins).
    if (families.some((family) => family.toLowerCase() === name.toLowerCase())) {
      return
    }
    commit([...families, name])
  }

  const removeFamily = (index: number): void => {
    commit(families.filter((_, i) => i !== index))
  }

  const moveFamily = (index: number, delta: -1 | 1): void => {
    const target = index + delta
    if (target < 0 || target >= families.length) {
      return
    }
    const next = [...families]
    ;[next[index], next[target]] = [next[target], next[index]]
    commit(next)
  }

  const label = translate(
    'auto.components.settings.TerminalFallbackFontsRow.rowLabel',
    'Fallback Fonts'
  )

  return (
    <SearchableSetting
      title={label}
      description={searchEntry?.description}
      keywords={searchEntry?.keywords ?? ['terminal', 'typography', 'font', 'fallback']}
      forceVisible={forceVisible}
    >
      <SettingsRow
        alignTop
        label={label}
        description={translate(
          'auto.components.settings.TerminalFallbackFontsRow.rowDescription',
          'Consulted in order after the primary font, before Orca’s automatic OS fallbacks. Applies to new terminal panes.'
        )}
        control={
          <div className="flex w-64 flex-col gap-2">
            {families.length > 0 ? (
              <ul className="flex flex-col gap-1" aria-label={label}>
                {families.map((family, index) => (
                  <li
                    key={`${family}-${index}`}
                    className="flex items-center gap-0.5 rounded-md border border-border/60 bg-muted/40 py-0.5 pl-2 pr-1"
                  >
                    <span className="min-w-0 flex-1 truncate text-xs" title={family}>
                      {family}
                    </span>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      disabled={index === 0}
                      aria-label={translate(
                        'auto.components.settings.TerminalFallbackFontsRow.moveUp',
                        'Move {{value0}} earlier',
                        { value0: family }
                      )}
                      onClick={() => moveFamily(index, -1)}
                    >
                      <ArrowUp aria-hidden="true" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      disabled={index === families.length - 1}
                      aria-label={translate(
                        'auto.components.settings.TerminalFallbackFontsRow.moveDown',
                        'Move {{value0}} later',
                        { value0: family }
                      )}
                      onClick={() => moveFamily(index, 1)}
                    >
                      <ArrowDown aria-hidden="true" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      aria-label={translate(
                        'auto.components.settings.TerminalFallbackFontsRow.remove',
                        'Remove {{value0}}',
                        { value0: family }
                      )}
                      onClick={() => removeFamily(index)}
                    >
                      <X aria-hidden="true" />
                    </Button>
                  </li>
                ))}
              </ul>
            ) : null}
            <form
              className="flex items-start gap-1.5"
              // Why: Enter in the autocomplete (once a suggestion is staged) submits
              // the add without a dedicated key handler inside FontAutocomplete.
              onSubmit={(event) => {
                event.preventDefault()
                addDraft()
              }}
            >
              <div className="min-w-0 flex-1">
                <FontAutocomplete
                  value={draft}
                  suggestions={suggestions}
                  onRequestSuggestions={onRequestSuggestions}
                  onChange={setDraft}
                  placeholder={translate(
                    'auto.components.settings.TerminalFallbackFontsRow.addPlaceholder',
                    'Add fallback font'
                  )}
                />
              </div>
              <Button
                type="submit"
                variant="outline"
                size="sm"
                className="gap-1"
                disabled={draft.trim().length === 0}
                aria-label={translate(
                  'auto.components.settings.TerminalFallbackFontsRow.add',
                  'Add fallback font'
                )}
              >
                <Plus aria-hidden="true" className="size-3.5" />
                {translate('auto.components.settings.TerminalFallbackFontsRow.addButton', 'Add')}
              </Button>
            </form>
          </div>
        }
      />
    </SearchableSetting>
  )
}
