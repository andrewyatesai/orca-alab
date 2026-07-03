import type { GlobalSettings } from '../../../../shared/types'
import { Input } from '../ui/input'
import {
  NumberField,
  SettingsRow,
  SettingsSubsectionHeader,
  SettingsSwitchRow
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { isMacUserAgent } from '../terminal-pane/pane-helpers'
import { translate } from '@/i18n/i18n'

// The engine panel's Scrollback / Input / Clipboard & Security groups. Each
// re-surfaces the SAME store values as its home section (Terminal / Advanced /
// Interaction), so the two surfaces always stay in sync.

type SectionProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}

export function TerminalEngineScrollbackSection({
  settings,
  updateSettings
}: SectionProps): React.JSX.Element {
  return (
    <section key="engine-scrollback" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate(
          'auto.components.settings.TerminalEnginePane.scrollback.title',
          'Scrollback'
        )}
        description={translate(
          'auto.components.settings.TerminalEnginePane.scrollback.description',
          'History retention and text selection behavior.'
        )}
      />
      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.scrollback.rows.title',
            'Scrollback Rows'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.scrollback.rows.description',
            'History lines retained behind the live viewport, per pane.'
          )}
          keywords={['terminal', 'engine', 'scrollback', 'history', 'rows', 'buffer']}
        >
          <NumberField
            label={translate(
              'auto.components.settings.TerminalEnginePane.scrollback.rows.title',
              'Scrollback Rows'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.scrollback.rows.note',
              'Also selectable as presets in Terminal → Advanced.'
            )}
            value={settings.terminalScrollbackRows}
            defaultValue={10_000}
            min={1000}
            max={100_000}
            step={1000}
            suffix={translate(
              'auto.components.settings.TerminalEnginePane.scrollback.rows.suffix',
              'rows'
            )}
            onChange={(value) =>
              updateSettings({
                terminalScrollbackRows: Math.min(100_000, Math.max(1000, Math.round(value)))
              })
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.scrollback.wordSeparators.title',
            'Word Separators'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.scrollback.wordSeparators.description',
            'Characters that break a double-click word selection.'
          )}
          keywords={['word', 'separators', 'double click', 'selection', 'select']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.scrollback.wordSeparators.title',
              'Word Separators'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.scrollback.wordSeparators.note',
              'Empty uses the engine’s default word logic. URLs and file paths are still selected whole.'
            )}
            control={
              <Input
                className="w-48 font-mono"
                value={settings.terminalWordSeparator ?? ''}
                placeholder={`()[]{}'",;`}
                onChange={(event) => updateSettings({ terminalWordSeparator: event.target.value })}
              />
            }
          />
        </SearchableSetting>
      </div>
    </section>
  )
}

export function TerminalEngineInputSection({
  settings,
  updateSettings
}: SectionProps): React.JSX.Element {
  const isMac = isMacUserAgent()
  return (
    <section key="engine-input" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate('auto.components.settings.TerminalEnginePane.input.title', 'Input')}
        description={translate(
          'auto.components.settings.TerminalEnginePane.input.description',
          'Keyboard protocols and platform key handling.'
        )}
      />
      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.input.kitty.title',
            'Kitty Keyboard Protocol'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.input.kitty.description',
            'Progressive keyboard enhancement for modern TUIs.'
          )}
          keywords={['kitty', 'keyboard', 'protocol', 'csi u', 'keys']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.input.kitty.title',
              'Kitty Keyboard Protocol'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.input.kitty.note',
              'The engine speaks the full Kitty protocol and negotiates it per pane automatically (withheld on local Windows ConPTY shells that mis-handle it). No configuration needed.'
            )}
            control={null}
          />
        </SearchableSetting>

        {isMac ? (
          <SearchableSetting
            title={translate(
              'auto.components.settings.TerminalEnginePane.input.macOption.title',
              'Option Key Sends Alt'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.input.macOption.description',
              'Whether macOS Option acts as the terminal Alt/Meta modifier.'
            )}
            keywords={['mac', 'option', 'alt', 'meta', 'modifier', 'keyboard']}
          >
            <SettingsSwitchRow
              label={translate(
                'auto.components.settings.TerminalEnginePane.input.macOption.title',
                'Option Key Sends Alt'
              )}
              description={translate(
                'auto.components.settings.TerminalEnginePane.input.macOption.note',
                'On: Option chords send Alt sequences. Off: Option composes special characters. Auto and per-side variants are in Terminal → macOS Keyboard.'
              )}
              checked={settings.terminalMacOptionAsAlt === 'true'}
              onChange={() =>
                updateSettings({
                  terminalMacOptionAsAlt:
                    settings.terminalMacOptionAsAlt === 'true' ? 'auto' : 'true'
                })
              }
            />
          </SearchableSetting>
        ) : null}

        {isMac ? (
          <SearchableSetting
            title={translate(
              'auto.components.settings.TerminalEnginePane.input.jisYen.title',
              'JIS Yen Sends Backslash'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.input.jisYen.description',
              'Translate the JIS keyboard’s ¥ key to backslash for shells.'
            )}
            keywords={['jis', 'yen', 'backslash', 'japanese', 'keyboard']}
          >
            <SettingsSwitchRow
              label={translate(
                'auto.components.settings.TerminalEnginePane.input.jisYen.title',
                'JIS Yen Sends Backslash'
              )}
              description={translate(
                'auto.components.settings.TerminalEnginePane.input.jisYen.note',
                'Shells and programming languages expect backslash; Option+¥ still types ¥.'
              )}
              checked={settings.terminalJISYenToBackslash}
              onChange={() =>
                updateSettings({ terminalJISYenToBackslash: !settings.terminalJISYenToBackslash })
              }
            />
          </SearchableSetting>
        ) : null}
      </div>
    </section>
  )
}

export function TerminalEngineClipboardSection({
  settings,
  updateSettings
}: SectionProps): React.JSX.Element {
  return (
    <section key="engine-clipboard" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate(
          'auto.components.settings.TerminalEnginePane.clipboard.title',
          'Clipboard & Security'
        )}
        description={translate(
          'auto.components.settings.TerminalEnginePane.clipboard.description',
          'What terminal programs may do with your clipboard.'
        )}
      />
      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalPane.3338dcf8c1',
            'Allow TUI Clipboard Writes (OSC 52)'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.clipboard.osc52Description',
            'The engine is fail-closed: OSC 52 writes are dropped until you opt in here.'
          )}
          keywords={['osc 52', 'osc52', 'clipboard', 'tmux', 'neovim', 'ssh', 'security']}
        >
          <SettingsSwitchRow
            label={translate(
              'auto.components.settings.TerminalPane.3338dcf8c1',
              'Allow TUI Clipboard Writes (OSC 52)'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.clipboard.osc52Note',
              'Lets tmux, Neovim, and fzf copy to your clipboard over the PTY (including SSH). Off by default because any program printing to the terminal could rewrite your clipboard.'
            )}
            checked={settings.terminalAllowOsc52Clipboard}
            onChange={() =>
              updateSettings({
                terminalAllowOsc52Clipboard: !settings.terminalAllowOsc52Clipboard
              })
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate('auto.components.settings.TerminalPane.902f5dee1f', 'Copy on Select')}
          description={translate(
            'auto.components.settings.TerminalPane.4729c645fc',
            'Automatically copy terminal selections to the clipboard.'
          )}
          keywords={['clipboard', 'copy', 'select', 'selection', 'automatic']}
        >
          <SettingsSwitchRow
            label={translate('auto.components.settings.TerminalPane.902f5dee1f', 'Copy on Select')}
            description={translate(
              'auto.components.settings.TerminalPane.4729c645fc',
              'Automatically copy terminal selections to the clipboard.'
            )}
            checked={settings.terminalClipboardOnSelect}
            onChange={() =>
              updateSettings({
                terminalClipboardOnSelect: !settings.terminalClipboardOnSelect
              })
            }
          />
        </SearchableSetting>
      </div>
    </section>
  )
}
