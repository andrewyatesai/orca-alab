import type { GlobalSettings } from '../../../../shared/types'
import { Label } from '../ui/label'
import { SearchableSetting } from './SearchableSetting'
import { matchesSettingsSearch } from './settings-search'
import { translate } from '@/i18n/i18n'

export const PUBLISH_REMOTE_BRANCH_KEYWORDS = [
  'publish',
  'remote branch',
  'origin',
  'upstream',
  'push',
  'worktree'
]

function publishRemoteBranchTitle(): string {
  return translate(
    'auto.components.settings.GitPane.publishRemoteBranchTitle',
    'Publish New Workspace Branches'
  )
}

function publishRemoteBranchDescription(): string {
  return translate(
    'auto.components.settings.GitPane.publishRemoteBranchDescription',
    'Push newly-created worktree branches to origin and set upstream tracking.'
  )
}

export function publishRemoteBranchMatchesSearch(searchQuery: string): boolean {
  return matchesSettingsSearch(searchQuery, {
    title: publishRemoteBranchTitle(),
    description: publishRemoteBranchDescription(),
    keywords: PUBLISH_REMOTE_BRANCH_KEYWORDS
  })
}

export function PublishRemoteBranchSetting({
  settings,
  updateSettings
}: {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
}): React.JSX.Element {
  return (
    <SearchableSetting
      key="publish-remote-branch"
      title={publishRemoteBranchTitle()}
      description={publishRemoteBranchDescription()}
      keywords={PUBLISH_REMOTE_BRANCH_KEYWORDS}
      className="flex items-center justify-between gap-4 py-2"
    >
      <div className="space-y-0.5">
        <Label>{publishRemoteBranchTitle()}</Label>
        <p className="text-xs text-muted-foreground">
          {translate(
            'auto.components.settings.GitPane.publishRemoteBranchBodyLead',
            'When enabled, Orca runs'
          )}{' '}
          <code>{translate("auto.components.settings.PublishRemoteBranchSetting.cd5221266b", "git push -u origin HEAD")}</code>{' '}
          {translate(
            'auto.components.settings.GitPane.publishRemoteBranchBodyTail',
            'after creating a workspace so terminal pushes target the workspace branch.'
          )}
        </p>
      </div>
      <button
        role="switch"
        aria-checked={settings.publishRemoteBranchOnWorktreeCreate}
        onClick={() =>
          updateSettings({
            publishRemoteBranchOnWorktreeCreate: !settings.publishRemoteBranchOnWorktreeCreate
          })
        }
        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors ${
          settings.publishRemoteBranchOnWorktreeCreate ? 'bg-foreground' : 'bg-muted-foreground/30'
        }`}
      >
        <span
          className={`pointer-events-none block size-3.5 rounded-full bg-background shadow-sm transition-transform ${
            settings.publishRemoteBranchOnWorktreeCreate ? 'translate-x-4' : 'translate-x-0.5'
          }`}
        />
      </button>
    </SearchableSetting>
  )
}
