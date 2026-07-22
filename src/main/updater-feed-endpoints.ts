/**
 * Single source of truth for every remote endpoint the auto-updater touches.
 *
 * Why: this fork must NEVER poll public Orca's update feed — one accepted
 * public update would silently replace the fork build with the public one
 * (staging-launch audit F1). Every updater URL derives from
 * UPDATE_FEED_REPO_SLUG, and when that slug is blank or points at the public
 * upstream repo the updater goes fully dormant instead of falling back to any
 * public URL.
 */

const PUBLIC_UPSTREAM_REPO_SLUG = 'stablyai/orca'

/** GitHub `owner/repo` that hosts this fork's releases and update manifests.
 *  Must match the publish block in config/electron-builder.config.cjs. */
export const UPDATE_FEED_REPO_SLUG = 'andrewyatesai/orca-alab'

// Why: public Orca points these at onorca.dev, the upstream vendor's service —
// which can remotely re-prompt users to update (nudge) and serves the public
// product's changelog. The fork has no equivalent service yet, so both stay
// null and the features stay dormant rather than pinging the public vendor.
export const UPDATE_NUDGE_URL: string | null = null
export const UPDATE_CHANGELOG_JSON_URL: string | null = null

export function isUpdateFeedSlugUsable(slug: string): boolean {
  const normalized = slug.trim().toLowerCase()
  return normalized !== '' && normalized !== PUBLIC_UPSTREAM_REPO_SLUG
}

/** False means "no fork-owned feed exists": the updater must stay dormant. */
export function isUpdateFeedConfigured(): boolean {
  return isUpdateFeedSlugUsable(UPDATE_FEED_REPO_SLUG)
}

export function getReleasesAtomFeedUrl(): string {
  return `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases.atom`
}

export function getReleasesDownloadBaseUrl(): string {
  return `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases/download`
}

export function getReleasesLatestDownloadUrl(): string {
  return `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases/latest/download`
}

/** Generic "release notes" page used when a changelog entry needs a
 *  non-version-specific link target. */
export function getUpdateChangelogPageUrl(): string {
  return `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases`
}

/** Mines `/releases/tag/<tag>` hrefs out of GitHub's releases atom feed.
 *  Rebuilt per call because `g`-flagged RegExps carry lastIndex state. */
export function getReleaseTagHrefPattern(): RegExp {
  const escapedSlug = UPDATE_FEED_REPO_SLUG.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return new RegExp(`href="https://github\\.com/${escapedSlug}/releases/tag/([^"]+)"`, 'g')
}
