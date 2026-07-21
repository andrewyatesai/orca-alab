import { translate } from '@/i18n/i18n'
import { translateSearchKeyword } from './settings-search-keywords'
import { createLocalizedCatalog } from '@/i18n/localized-catalog'

export const getTerminalPosixShellSearchEntry = createLocalizedCatalog(() => [
  {
    title: translate('auto.components.settings.terminal.posix.search.title', 'Default Shell'),
    description: translate(
      'auto.components.settings.terminal.posix.search.description',
      'Choose the default shell for new terminal panes on macOS and Linux.'
    ),
    keywords: [
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.terminal', 'terminal'),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.shell', 'shell'),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.default', 'default'),
      ...translateSearchKeyword(
        'auto.components.settings.terminal.posix.search.loginShell',
        'login shell'
      ),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.macos', 'macos'),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.linux', 'linux'),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.zsh', 'zsh', {
        englishOnly: true
      }),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.bash', 'bash', {
        englishOnly: true
      }),
      ...translateSearchKeyword('auto.components.settings.terminal.posix.search.fish', 'fish', {
        englishOnly: true
      })
    ]
  }
])
