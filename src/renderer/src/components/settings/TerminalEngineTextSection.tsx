import type { GlobalSettings } from '../../../../shared/types'
import {
  DEFAULT_TERMINAL_FONT_WEIGHT,
  TERMINAL_FONT_WEIGHT_MAX,
  TERMINAL_FONT_WEIGHT_MIN,
  TERMINAL_FONT_WEIGHT_STEP,
  normalizeTerminalFontWeight
} from '../../../../shared/terminal-fonts'
import { resolveTerminalLigaturesEnabled } from '../../../../shared/terminal-ligatures'
import { Input } from '../ui/input'
import {
  NumberField,
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { clampNumber } from '@/lib/terminal-theme'
import { translate } from '@/i18n/i18n'

type TerminalEngineTextSectionProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}

/** Engine-panel Text group: typography the engine rasterizes with. Re-surfaces
 *  the SAME store values as Appearance → Terminal typography (both stay in sync). */
export function TerminalEngineTextSection({
  settings,
  updateSettings
}: TerminalEngineTextSectionProps): React.JSX.Element {
  const ligaturesResolved = resolveTerminalLigaturesEnabled(
    settings.terminalLigatures,
    settings.terminalFontFamily
  )
  return (
    <section key="engine-text" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate('auto.components.settings.TerminalEnginePane.text.title', 'Text')}
        description={translate(
          'auto.components.settings.TerminalEnginePane.text.description',
          'Fonts and shaping. The engine rasterizes real font faces with HarfBuzz-class shaping.'
        )}
      />
      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate('auto.components.settings.terminal.search.e989914ad6', 'Font Family')}
          description={translate(
            'auto.components.settings.TerminalEnginePane.text.familyDescription',
            'Primary terminal font family; applies to new panes.'
          )}
          keywords={['terminal', 'engine', 'font', 'family', 'typeface']}
        >
          <SettingsRow
            label={translate('auto.components.settings.terminal.search.e989914ad6', 'Font Family')}
            description={translate(
              'auto.components.settings.TerminalEnginePane.text.familyRowDescription',
              'The engine loads the family’s real font file. Unknown names keep the bundled JetBrains Mono.'
            )}
            control={
              <Input
                className="w-48"
                value={settings.terminalFontFamily}
                placeholder="JetBrains Mono"
                onChange={(event) => updateSettings({ terminalFontFamily: event.target.value })}
              />
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate('auto.components.settings.terminal.search.5930244899', 'Font Size')}
          description={translate(
            'auto.components.settings.terminal.search.0fe0073f0c',
            'Default terminal font size for new panes and live updates.'
          )}
          keywords={['terminal', 'engine', 'font', 'size']}
        >
          <NumberField
            label={translate('auto.components.settings.terminal.search.5930244899', 'Font Size')}
            description=""
            value={settings.terminalFontSize}
            defaultValue={14}
            min={10}
            max={32}
            step={1}
            suffix="px"
            onChange={(value) => updateSettings({ terminalFontSize: clampNumber(value, 10, 32) })}
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate('auto.components.settings.terminal.search.28ea41bd2d', 'Font Weight')}
          description={translate(
            'auto.components.settings.terminal.search.98c18f2c77',
            'Controls the terminal text font weight.'
          )}
          keywords={['terminal', 'engine', 'font', 'weight', 'bold']}
        >
          <NumberField
            label={translate('auto.components.settings.terminal.search.28ea41bd2d', 'Font Weight')}
            description={translate(
              'auto.components.settings.TerminalEnginePane.text.weightRowDescription',
              'Selects the closest installed style of the family; SGR bold uses the real Bold face when installed.'
            )}
            value={normalizeTerminalFontWeight(settings.terminalFontWeight)}
            defaultValue={DEFAULT_TERMINAL_FONT_WEIGHT}
            min={TERMINAL_FONT_WEIGHT_MIN}
            max={TERMINAL_FONT_WEIGHT_MAX}
            step={TERMINAL_FONT_WEIGHT_STEP}
            suffix="100-900"
            onChange={(value) =>
              updateSettings({ terminalFontWeight: normalizeTerminalFontWeight(value) })
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate('auto.components.settings.terminal.search.0f2fb0cb74', 'Line Height')}
          description={translate(
            'auto.components.settings.terminal.search.36a1b38bc8',
            'Controls the terminal line height multiplier.'
          )}
          keywords={['terminal', 'engine', 'line height', 'spacing']}
        >
          <NumberField
            label={translate('auto.components.settings.terminal.search.0f2fb0cb74', 'Line Height')}
            description=""
            value={settings.terminalLineHeight}
            defaultValue={1}
            min={1}
            max={3}
            step={0.1}
            suffix="1-3"
            onChange={(value) => updateSettings({ terminalLineHeight: clampNumber(value, 1, 3) })}
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalAppearanceSection.be8da35e7f',
            'Font Ligatures'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.text.ligaturesDescription',
            'Programming ligatures (=>, !=, ===) shaped natively by the engine.'
          )}
          keywords={['terminal', 'engine', 'ligatures', 'calt', 'shaping']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalAppearanceSection.be8da35e7f',
              'Font Ligatures'
            )}
            description={
              ligaturesResolved
                ? translate(
                    'auto.components.settings.TerminalEnginePane.text.ligaturesOnNote',
                    'Currently resolved: on for your font.'
                  )
                : translate(
                    'auto.components.settings.TerminalEnginePane.text.ligaturesOffNote',
                    'Currently resolved: off for your font.'
                  )
            }
            control={
              <SettingsSegmentedControl
                ariaLabel={translate(
                  'auto.components.settings.TerminalAppearanceSection.be8da35e7f',
                  'Font Ligatures'
                )}
                value={settings.terminalLigatures ?? 'auto'}
                onChange={(option) => updateSettings({ terminalLigatures: option })}
                options={[
                  {
                    value: 'auto',
                    label: translate('auto.components.settings.TerminalPane.43c2ff7b0e', 'Auto')
                  },
                  {
                    value: 'on',
                    label: translate('auto.components.settings.TerminalPane.9c0b1c1792', 'On')
                  },
                  {
                    value: 'off',
                    label: translate('auto.components.settings.TerminalPane.3fe1c5bfe0', 'Off')
                  }
                ]}
              />
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.text.fallbacks.title',
            'Fallback Fonts'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.text.fallbacks.description',
            'CJK, symbol, and color-emoji fallback faces.'
          )}
          keywords={['fallback', 'cjk', 'emoji', 'unicode', 'tofu']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.text.fallbacks.title',
              'Fallback Fonts'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.text.fallbacks.note',
              'The engine automatically loads your OS’s CJK and color-emoji faces as fallbacks, so non-Latin text and emoji render without configuration.'
            )}
            control={null}
          />
        </SearchableSetting>
      </div>
    </section>
  )
}
