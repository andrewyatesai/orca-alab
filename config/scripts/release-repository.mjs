export const DEFAULT_RELEASE_REPOSITORY = 'alabsystems/orca-alab'
export const DEFAULT_RELEASE_NOTES_REPOSITORY = 'andrewyatesai/orca-alab'

export function resolveReleaseRepository(env = process.env) {
  // Why: source workflows run in the dev repo; its ambient GitHub slug must not redirect public releases.
  return env.ORCA_RELEASE_REPOSITORY?.trim() || DEFAULT_RELEASE_REPOSITORY
}

export function resolveReleaseNotesRepository(env = process.env) {
  // Why: the public snapshot has no development PR history, so GitHub must generate notes from the dev repo.
  return env.ORCA_RELEASE_NOTES_REPOSITORY?.trim() || DEFAULT_RELEASE_NOTES_REPOSITORY
}
