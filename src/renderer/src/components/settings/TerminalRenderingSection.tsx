import type { GlobalSettings } from '../../../../shared/types'
import {
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { translate } from '@/i18n/i18n'

type TerminalRenderingSectionProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}

export function TerminalRenderingSection({
  settings,
  updateSettings
}: TerminalRenderingSectionProps): React.JSX.Element {
  return (
    <section key="rendering" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate('auto.components.settings.TerminalPane.2fba319f21', 'Rendering')}
        description={translate(
          'auto.components.settings.TerminalPane.72bc9334a0',
          'Terminal renderer behavior for live panes and new panes.'
        )}
      />

      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate('auto.components.settings.TerminalPane.c1fc9e9444', 'GPU Acceleration')}
          description={translate(
            'auto.components.settings.TerminalPane.f07dfb4466',
            'Controls whether terminal panes draw with the WebGL2 GPU renderer. Auto uses the GPU when it is supported, with a conservative Linux fallback to the CPU canvas renderer for software or unknown GPUs.'
          )}
          keywords={[
            'terminal',
            'gpu',
            'acceleration',
            'webgl',
            'renderer',
            'rendering',
            'graphics',
            'linux'
          ]}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalPane.c1fc9e9444',
              'GPU Acceleration'
            )}
            description={
              settings.terminalGpuAcceleration === 'off'
                ? translate(
                    'auto.components.settings.TerminalPane.fe4acf36c6',
                    'GPU rendering disabled; CPU canvas renderer for max compatibility.'
                  )
                : settings.terminalGpuAcceleration === 'on'
                  ? translate(
                      'auto.components.settings.TerminalPane.7eaccc1424',
                      'WebGL2 GPU rendering is always attempted for terminal panes.'
                    )
                  : translate(
                      'auto.components.settings.TerminalPane.e0996d141a',
                      'Auto tries WebGL2, with CPU canvas fallback for unsupported or risky GPUs.'
                    )
            }
            control={
              <SettingsSegmentedControl
                ariaLabel={translate(
                  'auto.components.settings.TerminalPane.c1fc9e9444',
                  'GPU Acceleration'
                )}
                value={settings.terminalGpuAcceleration ?? 'auto'}
                onChange={(option) => updateSettings({ terminalGpuAcceleration: option })}
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
      </div>
    </section>
  )
}
