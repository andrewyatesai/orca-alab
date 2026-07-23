export type UpdateInstallMode = 'automatic' | 'manual'

export function getUpdateInstallMode(
  platform: NodeJS.Platform = process.platform
): UpdateInstallMode {
  // Why: ALab has no stable macOS or Windows publisher identity, so native
  // installers cannot authenticate a different release.
  return platform === 'linux' ? 'automatic' : 'manual'
}
