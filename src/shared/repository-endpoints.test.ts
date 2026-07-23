import { describe, expect, it } from 'vitest'
import {
  getOrcaAlabPublicReleaseUrl,
  ORCA_ALAB_DEVELOPMENT_DOCS_URL,
  ORCA_ALAB_DEVELOPMENT_ISSUES_URL,
  ORCA_ALAB_DEVELOPMENT_NEW_ISSUE_URL,
  ORCA_ALAB_DEVELOPMENT_REPOSITORY_SLUG,
  ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL,
  ORCA_ALAB_FEATURE_WALKTHROUGH_SECTION_URLS,
  ORCA_ALAB_FEATURE_WALKTHROUGH_URL,
  ORCA_ALAB_PRIVACY_URL,
  ORCA_ALAB_PUBLIC_CHANGELOG_URL,
  ORCA_ALAB_PUBLIC_RELEASES_URL,
  ORCA_ALAB_PUBLIC_REPOSITORY_SLUG,
  ORCA_ALAB_PUBLIC_REPOSITORY_URL,
  ORCA_ALAB_PUBLIC_STARGAZERS_URL
} from './repository-endpoints'

describe('repository endpoints', () => {
  it('keeps public discovery and release traffic on the ALab public repository', () => {
    expect(ORCA_ALAB_PUBLIC_REPOSITORY_SLUG).toBe('alabsystems/orca-alab')
    expect(ORCA_ALAB_PUBLIC_REPOSITORY_URL).toBe('https://github.com/alabsystems/orca-alab')
    expect(ORCA_ALAB_PUBLIC_RELEASES_URL).toBe('https://github.com/alabsystems/orca-alab/releases')
    expect(ORCA_ALAB_PUBLIC_CHANGELOG_URL).toBe(ORCA_ALAB_PUBLIC_RELEASES_URL)
    expect(ORCA_ALAB_PUBLIC_STARGAZERS_URL).toBe(
      'https://github.com/alabsystems/orca-alab/stargazers'
    )
    expect(getOrcaAlabPublicReleaseUrl('1.4.2')).toBe(
      'https://github.com/alabsystems/orca-alab/releases/tag/v1.4.2'
    )
    expect(getOrcaAlabPublicReleaseUrl(null)).toBe(
      'https://github.com/alabsystems/orca-alab/releases'
    )
  })

  it('keeps source and issue traffic on the ALab development repository', () => {
    expect(ORCA_ALAB_DEVELOPMENT_REPOSITORY_SLUG).toBe('andrewyatesai/orca-alab')
    expect(ORCA_ALAB_DEVELOPMENT_REPOSITORY_URL).toBe('https://github.com/andrewyatesai/orca-alab')
    expect(ORCA_ALAB_DEVELOPMENT_ISSUES_URL).toBe(
      'https://github.com/andrewyatesai/orca-alab/issues'
    )
    expect(ORCA_ALAB_DEVELOPMENT_NEW_ISSUE_URL).toBe(
      'https://github.com/andrewyatesai/orca-alab/issues/new'
    )
    expect(ORCA_ALAB_DEVELOPMENT_DOCS_URL).toBe(
      'https://github.com/andrewyatesai/orca-alab/tree/main/docs'
    )
    expect(ORCA_ALAB_FEATURE_WALKTHROUGH_URL).toBe(
      'https://github.com/andrewyatesai/orca-alab/blob/main/FEATURE_WALKTHROUGH.md'
    )
    expect(ORCA_ALAB_FEATURE_WALKTHROUGH_SECTION_URLS.terminal).toBe(
      `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#terminal`
    )
    expect(ORCA_ALAB_FEATURE_WALKTHROUGH_SECTION_URLS.review).toBe(
      `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#4-review-and-ship-changes`
    )
    expect(ORCA_ALAB_FEATURE_WALKTHROUGH_SECTION_URLS.remoteMobile).toBe(
      `${ORCA_ALAB_FEATURE_WALKTHROUGH_URL}#8-work-remotely-and-from-mobile`
    )
    expect(ORCA_ALAB_PRIVACY_URL).toBe(
      'https://github.com/andrewyatesai/orca-alab/blob/main/docs/reference/privacy-staging.md'
    )
  })
})
