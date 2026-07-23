export const ORCA_ALAB_PUBLIC_REPOSITORY_SLUG = 'alabsystems/orca-alab'
export const ORCA_ALAB_PUBLIC_REPOSITORY_URL = `https://github.com/${ORCA_ALAB_PUBLIC_REPOSITORY_SLUG}`
export const ORCA_ALAB_PUBLIC_RELEASES_URL = `${ORCA_ALAB_PUBLIC_REPOSITORY_URL}/releases`
export const ORCA_ALAB_PUBLIC_STARGAZERS_URL = `${ORCA_ALAB_PUBLIC_REPOSITORY_URL}/stargazers`
export const ORCA_ALAB_PUBLIC_CHANGELOG_URL = ORCA_ALAB_PUBLIC_RELEASES_URL

export const ORCA_ALAB_DEVELOPMENT_REPOSITORY_SLUG = 'andrewyatesai/orca-alab'
export const ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL = `https://github.com/${ORCA_ALAB_DEVELOPMENT_REPOSITORY_SLUG}`
export const ORCA_ALAB_DEVELOPMENT_ISSUES_URL = `${ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL}/issues`
export const ORCA_ALAB_DEVELOPMENT_NEW_ISSUE_URL = `${ORCA_ALAB_DEVELOPMENT_ISSUES_URL}/new`
export const ORCA_ALAB_DEVELOPMENT_DOCS_URL = `${ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL}/tree/main/docs`
export const ORCA_ALAB_FEATURE_WALKTHROUGH_URL = `${ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL}/blob/main/FEATURE_WALKTHROUGH.md`
export const ORCA_ALAB_FEATURE_WALKTHROUGH_SECTION_URLS = {
  readiness: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#launch-it-and-confirm-readiness`,
  terminalEngine: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#terminal-engine-pin-and-artifact-provenance`,
  addProject: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#1-add-a-project`,
  workspaces: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#2-create-isolated-workspaces-and-start-agents`,
  workbench: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#3-work-across-terminal-editor-and-browser`,
  terminal: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#terminal`,
  editor: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#editor-and-file-explorer`,
  floatingWorkspace: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#floating-workspace-and-optional-voice-input`,
  browser: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#built-in-browser`,
  review: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#4-review-and-ship-changes`,
  tasks: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#5-turn-tasks-into-workspaces`,
  orchestration: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#6-coordinate-multiple-agents`,
  automations: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#7-automate-recurring-work`,
  remoteMobile: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#8-work-remotely-and-from-mobile`,
  mobileEmulators: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#9-exercise-ios-and-android-apps`,
  computerUse: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#10-use-computer-use-for-desktop-apps`,
  cli: `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#cli-discovery`
} as const
export const ORCA_ALAB_PRIVACY_URL = `${ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL}/blob/main/docs/reference/privacy-staging.md`

export function getOrcaAlabPublicReleaseUrl(version: string | null): string {
  // Why: `/latest` can fail during GitHub API degradation; use the listing when no version is known.
  return version
    ? `${ORCA_ALAB_PUBLIC_RELEASES_URL}/tag/v${version}`
    : ORCA_ALAB_PUBLIC_RELEASES_URL
}
