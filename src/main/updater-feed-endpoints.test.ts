import { describe, expect, it } from 'vitest'
import {
  getReleasesAtomFeedUrl,
  getReleasesDownloadBaseUrl,
  getReleasesLatestDownloadUrl,
  getReleaseTagHrefPattern,
  getUpdateChangelogPageUrl,
  isUpdateFeedConfigured,
  isUpdateFeedSlugUsable,
  UPDATE_CHANGELOG_JSON_URL,
  UPDATE_FEED_REPO_SLUG,
  UPDATE_NUDGE_URL
} from './updater-feed-endpoints'

describe('updater-feed-endpoints', () => {
  it('derives every updater URL from the fork feed slug', () => {
    expect(getReleasesAtomFeedUrl()).toBe(
      `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases.atom`
    )
    expect(getReleasesDownloadBaseUrl()).toBe(
      `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases/download`
    )
    expect(getReleasesLatestDownloadUrl()).toBe(
      `https://github.com/${UPDATE_FEED_REPO_SLUG}/releases/latest/download`
    )
    expect(getUpdateChangelogPageUrl()).toBe(`https://github.com/${UPDATE_FEED_REPO_SLUG}/releases`)
  })

  // Why: audit F1 — one accepted public update replaces the fork build. No
  // updater endpoint may ever point at public Orca's repo or vendor domain.
  it('never references the public upstream feed or vendor endpoints', () => {
    const urls = [
      getReleasesAtomFeedUrl(),
      getReleasesDownloadBaseUrl(),
      getReleasesLatestDownloadUrl(),
      getUpdateChangelogPageUrl(),
      getReleaseTagHrefPattern().source
    ]
    for (const url of urls) {
      expect(url).not.toMatch(/stablyai/i)
      expect(url).not.toMatch(/onorca\.dev/i)
    }
    expect(UPDATE_NUDGE_URL).toBeNull()
    expect(UPDATE_CHANGELOG_JSON_URL).toBeNull()
  })

  it('treats any non-empty slug as configured', () => {
    expect(isUpdateFeedConfigured()).toBe(true)
    expect(isUpdateFeedSlugUsable('')).toBe(false)
    expect(isUpdateFeedSlugUsable('   ')).toBe(false)
    expect(isUpdateFeedSlugUsable('alabsystems/orca-alab')).toBe(true)
  })

  it('mines release tags for the fork repo only', () => {
    const body = [
      `<entry><link href="https://github.com/${UPDATE_FEED_REPO_SLUG}/releases/tag/v1.4.122-fork.2"/></entry>`,
      '<entry><link href="https://github.com/stablyai/orca/releases/tag/v9.9.9"/></entry>'
    ].join('\n')
    const tags = [...body.matchAll(getReleaseTagHrefPattern())].map((match) => match[1])
    expect(tags).toEqual(['v1.4.122-fork.2'])
  })
})
