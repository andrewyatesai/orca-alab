import { useState } from 'react'
import type { GlobalSettings } from '../../../../shared/types'
import {
  WINDOWS_GIT_BASH_SHELL,
  WINDOWS_NUSHELL_SHELL
} from '../../../../shared/windows-terminal-shell'
import {
  SettingsRow,
  SettingsSegmentedControl,
  SettingsSubsectionHeader
} from './SettingsFormControls'
import { SearchableSetting } from './SearchableSetting'
import { TerminalCustomShellPathInput } from './TerminalCustomShellPathInput'
import { translate } from '@/i18n/i18n'
import { ShellIcon } from '../tab-bar/shell-icons'

const CUSTOM_SHELL_OPTION_VALUE = 'custom-shell-path'
// Why: a JS string, not a JSX attribute literal — backslash pairs must collapse.
const CUSTOM_SHELL_PATH_PLACEHOLDER = 'C:\\Program Files\\PowerShell\\7\\pwsh.exe'

// Why (#7467): terminalWindowsShell holds either a built-in choice or an explicit path; a separator marks the path form.
function isCustomWindowsShellPath(shell: string): boolean {
  return shell.includes('\\') || shell.includes('/')
}

type TerminalWindowsShellSectionProps = {
  updateSettings: (updates: Partial<GlobalSettings>) => void
  windowsShell: string
  gitBashAvailable: boolean
  nushellAvailable: boolean
}

function windowsShellLabel(shell: string, label: string): React.JSX.Element {
  return (
    <span className="inline-flex items-center justify-center gap-1.5">
      <ShellIcon shell={shell} size={12} />
      <span>{label}</span>
    </span>
  )
}

export function TerminalWindowsShellSection({
  updateSettings,
  windowsShell,
  gitBashAvailable,
  nushellAvailable
}: TerminalWindowsShellSectionProps): React.JSX.Element {
  const [customSelected, setCustomSelected] = useState(false)
  const hasCustomShellPath = isCustomWindowsShellPath(windowsShell)
  const showCustomInput = customSelected || hasCustomShellPath
  const showGitBashOption = gitBashAvailable || windowsShell === WINDOWS_GIT_BASH_SHELL
  // Why: keep a selected-but-missing shell visible (disabled) so the setting is never silently hidden — same rule as Git Bash.
  const showNushellOption = nushellAvailable || windowsShell === WINDOWS_NUSHELL_SHELL
  // Why (#9779): selecting WSL here would omit its required distro, but an existing WSL default must stay visible.
  const showWslOption = windowsShell === 'wsl.exe'

  return (
    <section key="windows-shell" className="space-y-3">
      <SettingsSubsectionHeader
        title={translate('auto.components.settings.TerminalPane.87e678a8af', 'Windows Shell')}
        description={translate(
          'auto.components.settings.TerminalPane.a55eee649f',
          'Default shell for new terminal panes on Windows.'
        )}
      />

      <div className="divide-y divide-border/40">
        <SearchableSetting
          title={translate('auto.components.settings.TerminalPane.27e301f22c', 'Default Shell')}
          description={translate(
            'auto.components.settings.TerminalPane.bd68f3170d',
            'Choose the default shell for new terminal panes on Windows.'
          )}
          keywords={[
            'terminal',
            'windows',
            'shell',
            'powershell',
            'cmd',
            'command prompt',
            'git bash',
            'bash.exe',
            'nushell',
            'nu.exe',
            'default'
          ]}
        >
          <SettingsRow
            label={translate('auto.components.settings.TerminalPane.27e301f22c', 'Default Shell')}
            description={translate(
              'auto.components.settings.TerminalPane.09bf02de9a',
              'Shell used when opening a new terminal pane. Takes effect for new terminals.'
            )}
            control={
              <SettingsSegmentedControl
                ariaLabel={translate(
                  'auto.components.settings.TerminalPane.27e301f22c',
                  'Default Shell'
                )}
                value={showCustomInput ? CUSTOM_SHELL_OPTION_VALUE : windowsShell}
                onChange={(value) => {
                  if (value === CUSTOM_SHELL_OPTION_VALUE) {
                    // Why: selecting Custom… reveals the input but keeps the setting until a path is committed.
                    setCustomSelected(true)
                    return
                  }
                  setCustomSelected(false)
                  updateSettings({ terminalWindowsShell: value })
                }}
                options={[
                  {
                    value: 'powershell.exe',
                    label: windowsShellLabel(
                      'powershell.exe',
                      translate('auto.components.settings.TerminalPane.eb7fc4d98a', 'PowerShell')
                    ),
                    ariaLabel: translate(
                      'auto.components.settings.TerminalPane.eb7fc4d98a',
                      'PowerShell'
                    )
                  },
                  {
                    value: 'cmd.exe',
                    label: windowsShellLabel(
                      'cmd.exe',
                      translate(
                        'auto.components.settings.TerminalPane.0f1b8669e6',
                        'Command Prompt'
                      )
                    ),
                    ariaLabel: translate(
                      'auto.components.settings.TerminalPane.0f1b8669e6',
                      'Command Prompt'
                    )
                  },
                  ...(showGitBashOption
                    ? [
                        {
                          value: WINDOWS_GIT_BASH_SHELL,
                          label: windowsShellLabel(
                            WINDOWS_GIT_BASH_SHELL,
                            translate(
                              'auto.components.settings.TerminalPane.f61ac77f16',
                              'Git Bash'
                            )
                          ),
                          ariaLabel: translate(
                            'auto.components.settings.TerminalPane.f61ac77f16',
                            'Git Bash'
                          ),
                          disabled: !gitBashAvailable
                        }
                      ]
                    : []),
                  ...(showNushellOption
                    ? [
                        {
                          value: WINDOWS_NUSHELL_SHELL,
                          label: windowsShellLabel(
                            WINDOWS_NUSHELL_SHELL,
                            translate('auto.components.settings.TerminalPane.nushell', 'Nushell')
                          ),
                          ariaLabel: translate(
                            'auto.components.settings.TerminalPane.nushell',
                            'Nushell'
                          ),
                          disabled: !nushellAvailable
                        }
                      ]
                    : []),
                  ...(showWslOption
                    ? [
                        {
                          value: 'wsl.exe',
                          label: windowsShellLabel(
                            'wsl.exe',
                            translate('auto.components.settings.TerminalPane.b637dd57a7', 'WSL')
                          ),
                          ariaLabel: translate(
                            'auto.components.settings.TerminalPane.b637dd57a7',
                            'WSL'
                          ),
                          disabled: true
                        }
                      ]
                    : []),
                  {
                    value: CUSTOM_SHELL_OPTION_VALUE,
                    label: translate(
                      'auto.components.settings.TerminalWindowsShellSection.customOption',
                      'Custom…'
                    ),
                    ariaLabel: translate(
                      'auto.components.settings.TerminalWindowsShellSection.customOptionAria',
                      'Custom shell path'
                    )
                  }
                ]}
              />
            }
          />
          {showCustomInput ? (
            <SettingsRow
              label={translate(
                'auto.components.settings.TerminalWindowsShellSection.customPathLabel',
                'Shell Path'
              )}
              description={translate(
                'auto.components.settings.TerminalWindowsShellSection.customPathDescription',
                'Absolute path to the shell executable. Takes effect for new terminals.'
              )}
              alignTop
              control={
                <div className="w-64">
                  <TerminalCustomShellPathInput
                    inputId="settings-terminal-windows-shell-path"
                    value={hasCustomShellPath ? windowsShell : ''}
                    placeholder={CUSTOM_SHELL_PATH_PLACEHOLDER}
                    onCommit={(path) => updateSettings({ terminalWindowsShell: path })}
                  />
                </div>
              }
            />
          ) : null}
        </SearchableSetting>
      </div>
    </section>
  )
}
