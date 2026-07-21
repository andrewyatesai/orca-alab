import type { GlobalSettings } from '../../../../shared/types'
import {
  AUTO_FETCH_MAX_INTERVAL_MINUTES,
  AUTO_FETCH_MIN_INTERVAL_MINUTES,
  resolveGitAutoFetchSettings
} from '../../../../shared/git-auto-fetch-settings'
import { translate } from '@/i18n/i18n'
import { SearchableSetting } from './SearchableSetting'
import { NumberField, SettingsSwitchRow } from './SettingsFormControls'
import { matchesSettingsSearch } from './settings-search'

export const GIT_AUTO_FETCH_KEYWORDS = [
  'auto fetch',
  'automatic fetch',
  'git fetch',
  'background fetch',
  'ahead',
  'behind',
  'stale',
  'remote',
  'refresh',
  'interval'
]

function getGitAutoFetchTitle(): string {
  return translate('auto.components.settings.GitAutoFetchSetting.title', 'Automatic Git Fetch')
}

function getGitAutoFetchDescription(): string {
  return translate(
    'auto.components.settings.GitAutoFetchSetting.description',
    'Periodically fetch remotes for local repositories so ahead/behind counts stay accurate for you and your agents. Failing repositories back off automatically.'
  )
}

export function gitAutoFetchMatchesSearch(searchQuery: string): boolean {
  return matchesSettingsSearch(searchQuery, {
    title: getGitAutoFetchTitle(),
    description: getGitAutoFetchDescription(),
    keywords: GIT_AUTO_FETCH_KEYWORDS
  })
}

export function GitAutoFetchSetting({
  settings,
  updateSettings
}: {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void | Promise<void>
}): React.JSX.Element {
  const resolved = resolveGitAutoFetchSettings(settings)

  return (
    <SearchableSetting
      title={getGitAutoFetchTitle()}
      description={getGitAutoFetchDescription()}
      keywords={GIT_AUTO_FETCH_KEYWORDS}
      className="space-y-2 py-2"
    >
      <SettingsSwitchRow
        label={getGitAutoFetchTitle()}
        description={getGitAutoFetchDescription()}
        checked={resolved.enabled}
        onChange={() => void updateSettings({ autoFetchEnabled: !resolved.enabled })}
      />
      {resolved.enabled ? (
        <NumberField
          label={translate(
            'auto.components.settings.GitAutoFetchSetting.intervalLabel',
            'Fetch interval'
          )}
          description={translate(
            'auto.components.settings.GitAutoFetchSetting.intervalDescription',
            'Minutes between background fetches per repository.'
          )}
          value={resolved.intervalMinutes}
          defaultValue={resolved.intervalMinutes}
          min={AUTO_FETCH_MIN_INTERVAL_MINUTES}
          max={AUTO_FETCH_MAX_INTERVAL_MINUTES}
          onChange={(next) => void updateSettings({ autoFetchIntervalMinutes: next })}
          suffix={translate('auto.components.settings.GitAutoFetchSetting.intervalSuffix', 'min')}
        />
      ) : null}
    </SearchableSetting>
  )
}
