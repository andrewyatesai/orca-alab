import type { GlobalSettings } from '../../../../shared/types'
import {
  NumberField,
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { normalizeTerminalOpacity } from '@/lib/pane-manager/aterm/aterm-controller-option-readers'
import { translate } from '@/i18n/i18n'

type TerminalEngineRenderingSectionProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}

/** Engine-panel Rendering group. Re-surfaces the SAME store values as the
 *  Terminal section's rendering/appearance controls (both stay in sync). */
export function TerminalEngineRenderingSection({
  settings,
  updateSettings
}: TerminalEngineRenderingSectionProps): React.JSX.Element {
  return (
    <section key="engine-rendering" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate(
          'auto.components.settings.TerminalEnginePane.rendering.title',
          'Rendering'
        )}
        description={translate(
          'auto.components.settings.TerminalEnginePane.rendering.description',
          'How the engine rasterizes and presents terminal frames.'
        )}
      />
      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate('auto.components.settings.TerminalPane.c1fc9e9444', 'GPU Acceleration')}
          description={translate(
            'auto.components.settings.TerminalEnginePane.rendering.gpuDescription',
            'WebGL2 GPU presentation with automatic CPU-canvas fallback.'
          )}
          keywords={['terminal', 'engine', 'gpu', 'webgl', 'acceleration', 'renderer']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalPane.c1fc9e9444',
              'GPU Acceleration'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.rendering.gpuRowDescription',
              'Auto uses the GPU where supported and falls back to the CPU renderer. Also set in Terminal settings.'
            )}
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

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.rendering.backgroundOpacity.title',
            'Background Opacity'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.rendering.backgroundOpacity.description',
            'Opacity of the default terminal background; below 1 the pane shows through.'
          )}
          keywords={['terminal', 'background', 'opacity', 'transparent', 'translucent']}
        >
          <NumberField
            label={translate(
              'auto.components.settings.TerminalEnginePane.rendering.backgroundOpacity.title',
              'Background Opacity'
            )}
            description=""
            value={normalizeTerminalOpacity(settings.terminalBackgroundOpacity)}
            defaultValue={1}
            min={0.3}
            max={1}
            step={0.05}
            suffix="0.3-1"
            onChange={(value) =>
              updateSettings({ terminalBackgroundOpacity: normalizeTerminalOpacity(value) })
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.rendering.cursorOpacity.title',
            'Cursor Opacity'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.rendering.cursorOpacity.description',
            'Opacity of the cursor fill; below 1 the glyph under a block cursor shows through.'
          )}
          keywords={['terminal', 'cursor', 'opacity', 'transparent']}
        >
          <NumberField
            label={translate(
              'auto.components.settings.TerminalEnginePane.rendering.cursorOpacity.title',
              'Cursor Opacity'
            )}
            description=""
            value={normalizeTerminalOpacity(settings.terminalCursorOpacity)}
            defaultValue={1}
            min={0.2}
            max={1}
            step={0.05}
            suffix="0.2-1"
            onChange={(value) =>
              updateSettings({ terminalCursorOpacity: normalizeTerminalOpacity(value) })
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.rendering.minContrast.title',
            'Minimum Contrast'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.rendering.minContrast.description',
            'Per-cell WCAG contrast floor for text colors.'
          )}
          keywords={['contrast', 'wcag', 'accessibility', 'readability']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.rendering.minContrast.title',
              'Minimum Contrast'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.rendering.minContrast.note',
              'The engine floors every glyph’s color against its own cell background at a 4.5:1 WCAG ratio. Always on; not configurable.'
            )}
            control={null}
          />
        </SearchableSetting>
      </div>
    </section>
  )
}
