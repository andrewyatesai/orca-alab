import { useEffect, useState } from 'react'
import type { GlobalSettings } from '../../../../shared/types'
import {
  normalizePosixShellSetting,
  POSIX_TERMINAL_SHELL_CHOICES,
  posixShellDisplayName,
  type PosixTerminalShellDetection
} from '../../../../shared/posix-terminal-shell'
import {
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { translate } from '@/i18n/i18n'

const SYSTEM_SHELL_OPTION_VALUE = 'system'

type TerminalPosixShellSectionProps = {
  updateSettings: (updates: Partial<GlobalSettings>) => void
  posixShell: string | null
}

export function TerminalPosixShellSection({
  updateSettings,
  posixShell
}: TerminalPosixShellSectionProps): React.JSX.Element {
  const [detection, setDetection] = useState<PosixTerminalShellDetection | null>(null)
  useEffect(() => {
    let cancelled = false
    window.api.posixShells
      .detect()
      .then((result) => {
        if (!cancelled) {
          setDetection(result)
        }
      })
      // Why: null detection means "availability unknown" — keep the selected choice usable instead of disabling everything.
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  const selectedShell = normalizePosixShellSetting(posixShell)
  const availableShells = detection ? new Set(detection.shells.map((option) => option.shell)) : null
  const knownChoices = POSIX_TERMINAL_SHELL_CHOICES.filter(
    (shell) => shell === selectedShell || (availableShells?.has(shell) ?? false)
  )
  // Why: a hand-edited settings value (e.g. an explicit path) must stay visible and deselectable.
  const customChoice =
    selectedShell && !POSIX_TERMINAL_SHELL_CHOICES.some((shell) => shell === selectedShell)
      ? selectedShell
      : null

  const systemDescription = detection?.systemShellName
    ? translate(
        'auto.components.settings.TerminalPosixShellSection.rowDescriptionWithSystemShell',
        'Shell used when opening a new terminal pane. System uses your login shell ({{shell}}). Takes effect for new terminals.',
        { shell: detection.systemShellName }
      )
    : translate(
        'auto.components.settings.TerminalPosixShellSection.rowDescription',
        'Shell used when opening a new terminal pane. System uses your login shell. Takes effect for new terminals.'
      )

  return (
    <section key="posix-shell" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate('auto.components.settings.TerminalPosixShellSection.title', 'Shell')}
        description={translate(
          'auto.components.settings.TerminalPosixShellSection.description',
          'Default shell for new local terminal panes. SSH terminals keep the remote login shell.'
        )}
      />

      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate(
            'auto.components.settings.TerminalPosixShellSection.rowTitle',
            'Default Shell'
          )}
          description={translate(
            'auto.components.settings.TerminalPosixShellSection.searchDescription',
            'Choose the default shell for new terminal panes on macOS and Linux.'
          )}
          keywords={['terminal', 'shell', 'default', 'zsh', 'bash', 'fish', 'login shell']}
        >
          <SettingsRow
            label={translate(
              'auto.components.settings.TerminalPosixShellSection.rowTitle',
              'Default Shell'
            )}
            description={systemDescription}
            control={
              <SettingsSegmentedControl
                ariaLabel={translate(
                  'auto.components.settings.TerminalPosixShellSection.rowTitle',
                  'Default Shell'
                )}
                value={selectedShell ?? SYSTEM_SHELL_OPTION_VALUE}
                onChange={(value) =>
                  updateSettings({
                    terminalPosixShell: value === SYSTEM_SHELL_OPTION_VALUE ? null : value
                  })
                }
                options={[
                  {
                    value: SYSTEM_SHELL_OPTION_VALUE,
                    label: translate(
                      'auto.components.settings.TerminalPosixShellSection.systemOption',
                      'System'
                    ),
                    ariaLabel: translate(
                      'auto.components.settings.TerminalPosixShellSection.systemOptionAria',
                      'System login shell'
                    )
                  },
                  ...knownChoices.map((shell) => ({
                    value: shell as string,
                    label: shell,
                    ariaLabel: shell,
                    disabled: availableShells ? !availableShells.has(shell) : false
                  })),
                  ...(customChoice
                    ? [
                        {
                          value: customChoice,
                          label: posixShellDisplayName(customChoice),
                          ariaLabel: posixShellDisplayName(customChoice)
                        }
                      ]
                    : [])
                ]}
              />
            }
          />
        </SearchableSetting>
      </div>
    </section>
  )
}
