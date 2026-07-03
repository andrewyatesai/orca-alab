import type { GlobalSettings, TerminalCursorGlowStyle } from '../../../../shared/types'
import {
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader,
  SettingsSwitch,
  SettingsSwitchRow
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { TerminalEngineEffectsDemo } from './TerminalEngineEffectsDemo'
import { prefersReducedMotion } from '@/lib/pane-manager/aterm/aterm-effects-settings'
import { translate } from '@/i18n/i18n'

type TerminalEngineEffectsSectionProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
  systemPrefersDark: boolean
}

const GLOW_STYLES: TerminalCursorGlowStyle[] = [
  'lumen',
  'rainbow',
  'sparkle',
  'fire',
  'laser',
  'water'
]

type SparkleClassKey =
  | 'terminalEffectsSparkleProfanity'
  | 'terminalEffectsSparkleFeline'
  | 'terminalEffectsSparkleOrca'
  | 'terminalEffectsSparkleEmphasis'

export function TerminalEngineEffectsSection({
  settings,
  updateSettings,
  systemPrefersDark
}: TerminalEngineEffectsSectionProps): React.JSX.Element {
  const sparkleOn = settings.terminalEffectsSparkleWords ?? false
  const reducedMotion = prefersReducedMotion()

  // Class gates only take effect while the master is on — mirror that with a
  // disabled switch instead of hiding the rows (discoverability).
  const classRow = (
    key: SparkleClassKey,
    label: string,
    description: string
  ): React.JSX.Element => (
    <SettingsRow
      label={label}
      description={description}
      control={
        <SettingsSwitch
          checked={settings[key] ?? true}
          disabled={!sparkleOn}
          ariaLabel={label}
          onChange={() => updateSettings({ [key]: !(settings[key] ?? true) })}
        />
      }
    />
  )

  return (
    <section key="engine-effects" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate('auto.components.settings.TerminalEnginePane.effects.title', 'Effects')}
        description={translate(
          'auto.components.settings.TerminalEnginePane.effects.description',
          'Animated engine effects. All default off; when off, terminal rendering is byte-identical.'
        )}
      />

      <TerminalEngineEffectsDemo settings={settings} systemPrefersDark={systemPrefersDark} />

      {reducedMotion ? (
        <p className="text-xs text-muted-foreground">
          {translate(
            'auto.components.settings.TerminalEnginePane.effects.reducedMotionNote',
            'Your system prefers reduced motion: sparkle decorations render statically and cursor glow stays off. The engine’s flash-safety limiter always applies.'
          )}
        </p>
      ) : null}

      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.effects.sparkle.title',
            'Sparkle Words'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.effects.sparkle.description',
            'Decorate words from the engine lexicon as they appear in terminal output.'
          )}
          keywords={['terminal', 'effects', 'sparkle', 'words', 'animation', 'orca', 'cat', 'nova']}
        >
          <SettingsSwitchRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.effects.sparkle.title',
              'Sparkle Words'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.effects.sparkle.switchDescription',
              'Master switch. Scans visible output for lexicon words and animates them; turning it off restores byte-identical rendering.'
            )}
            checked={sparkleOn}
            onChange={() => updateSettings({ terminalEffectsSparkleWords: !sparkleOn })}
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.effects.sparkleClasses.title',
            'Sparkle Word Classes'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.effects.sparkleClasses.description',
            'Per-class toggles for the sparkle-words lexicon families.'
          )}
          keywords={['sparkle', 'class', 'profanity', 'feline', 'cat', 'orca', 'emphasis']}
        >
          <div className="space-y-1 py-1">
            {classRow(
              'terminalEffectsSparkleProfanity',
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classProfanity.title',
                'Profanity supernovas'
              ),
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classProfanity.description',
                'Expletives ignite a brief supernova. Flash-limited to at most two per second.'
              )
            )}
            {classRow(
              'terminalEffectsSparkleFeline',
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classFeline.title',
                'Feline cats'
              ),
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classFeline.description',
                'Cat words get a peeking cat that blinks while the pane is focused.'
              )
            )}
            {classRow(
              'terminalEffectsSparkleOrca',
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classOrca.title',
                'Orca splashes'
              ),
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classOrca.description',
                'Orca words splash water droplets.'
              )
            )}
            {classRow(
              'terminalEffectsSparkleEmphasis',
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classEmphasis.title',
                'Emphasis ink'
              ),
              translate(
                'auto.components.settings.TerminalEnginePane.effects.classEmphasis.description',
                'Animated ink gradient on emphasis words. The built-in lexicon ships no emphasis words yet, so this only affects custom lexicon overrides.'
              )
            )}
          </div>
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.effects.cursorGlow.title',
            'Cursor Glow'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.effects.cursorGlow.description',
            'An aurora of light in the cursor’s wake as it moves.'
          )}
          keywords={['cursor', 'glow', 'trail', 'aurora', 'comet', 'effects', 'animation']}
        >
          <SettingsSwitchRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.effects.cursorGlow.title',
              'Cursor Glow'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.effects.cursorGlow.switchDescription',
              'Additive light follows the cursor; it fades and settles when the cursor rests.'
            )}
            checked={settings.terminalEffectsCursorGlow ?? false}
            onChange={() =>
              updateSettings({
                terminalEffectsCursorGlow: !(settings.terminalEffectsCursorGlow ?? false)
              })
            }
          />
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.effects.glowStyle.title',
              'Glow Style'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.effects.glowStyle.description',
              'Colors derive from your terminal theme’s cursor color.'
            )}
            control={
              <SettingsSegmentedControl
                ariaLabel={translate(
                  'auto.components.settings.TerminalEnginePane.effects.glowStyle.title',
                  'Glow Style'
                )}
                value={settings.terminalEffectsCursorGlowStyle ?? 'lumen'}
                onChange={(style) => updateSettings({ terminalEffectsCursorGlowStyle: style })}
                options={GLOW_STYLES.map((style) => ({ value: style, label: style }))}
              />
            }
          />
        </SearchableSetting>

        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalEnginePane.effects.scenes.title',
            'Scenes'
          )}
          description={translate(
            'auto.components.settings.TerminalEnginePane.effects.scenes.description',
            'Ambient animated pane backgrounds.'
          )}
          keywords={['scene', 'scenes', 'background', 'meadow', 'ambient']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalEnginePane.effects.scenes.title',
              'Scenes'
            )}
            description={translate(
              'auto.components.settings.TerminalEnginePane.effects.scenes.unavailable',
              'The engine ships the scene framework, but its built-in scene art is being rewritten and no scenes are available yet. A picker will appear here when they land.'
            )}
            control={null}
          />
        </SearchableSetting>
      </div>
    </section>
  )
}
