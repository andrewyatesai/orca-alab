import type { GlobalSettings } from '../../../../shared/types'
import {
  NumberField,
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { normalizeTerminalOpacity } from '@/lib/pane-manager/aterm/aterm-controller-option-readers'
import {
  readTerminalEngineRendererStatus,
  type TerminalEngineRendererStatus
} from './terminal-engine-renderer-status'
import { translate } from '@/i18n/i18n'

type TerminalEngineRenderingSectionProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}

/** Localized "why this path" line for the live renderer-status row. */
function rendererStatusReasonText(status: TerminalEngineRendererStatus): string {
  switch (status.reason) {
    case 'forced-on':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonForcedOn',
        'A test override forces the GPU path on.'
      )
    case 'forced-off':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonForcedOff',
        'A test override forces the CPU path.'
      )
    case 'setting-on':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonSettingOn',
        'GPU acceleration is set to On.'
      )
    case 'setting-off':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonSettingOff',
        'GPU acceleration is set to Off.'
      )
    case 'auto-allowed':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonAutoAllowed',
        'Auto: this GPU passed the renderer safety checks.'
      )
    case 'auto-no-webgl2':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonNoWebgl2',
        'No WebGL2 context could be created, so panes use the CPU rasterizer.'
      )
    case 'auto-unsafe-renderer':
      return translate(
        'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.reasonUnsafeRenderer',
        'Auto: a software or unidentified GPU was detected, so panes use the CPU rasterizer.'
      )
  }
}

/** Engine-panel Rendering group. Re-surfaces the SAME store values as the
 *  Terminal section's rendering/appearance controls (both stay in sync). */
export function TerminalEngineRenderingSection({
  settings,
  updateSettings
}: TerminalEngineRenderingSectionProps): React.JSX.Element {
  // Computed per render (cheap: cached probe + a store read) so the status row
  // always agrees with the GPU control above it the moment the setting flips.
  const rendererStatus = readTerminalEngineRendererStatus()
  const rendererStatusValue = [
    rendererStatus.path === 'gpu'
      ? translate(
          'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.valueGpu',
          'GPU (WebGL2)'
        )
      : translate(
          'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.valueCpu',
          'CPU'
        ),
    rendererStatus.workerPresentation
      ? translate(
          'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.presentationWorker',
          'render worker'
        )
      : translate(
          'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.presentationInProcess',
          'in-process'
        )
  ].join(' · ')
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
            'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.title',
            'Renderer'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.description',
            'The live draw path new terminal panes take.'
          )}
          keywords={['renderer', 'gpu', 'cpu', 'webgl', 'status', 'adapter', 'worker', 'angle']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.title',
              'Renderer'
            )}
            description={
              <>
                {rendererStatusReasonText(rendererStatus)}{' '}
                {translate(
                  'auto.components.settings.TerminalEnginePane.rendering.rendererStatus.newPanesNote',
                  'Applies to new panes; a pane whose GPU init fails or loses its context falls back to the CPU rasterizer.'
                )}
                {rendererStatus.adapter ? (
                  <span className="block truncate font-mono">{rendererStatus.adapter}</span>
                ) : null}
              </>
            }
            control={
              <span className="font-mono text-xs text-muted-foreground">{rendererStatusValue}</span>
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
