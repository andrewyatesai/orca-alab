import { readFile } from 'node:fs/promises'
import { describe, expect, it } from 'vitest'
import {
  DEFAULT_RELEASE_NOTES_REPOSITORY,
  DEFAULT_RELEASE_REPOSITORY,
  resolveReleaseNotesRepository,
  resolveReleaseRepository
} from './release-repository.mjs'

describe('resolveReleaseRepository', () => {
  it('defaults to the public release repository, ignoring the source workflow repository', () => {
    expect(resolveReleaseRepository({ GITHUB_REPOSITORY: 'andrewyatesai/orca-alab' })).toBe(
      DEFAULT_RELEASE_REPOSITORY
    )
    expect(DEFAULT_RELEASE_REPOSITORY).toBe('alabsystems/orca-alab')
  })

  it('accepts only the dedicated release repository override', () => {
    expect(
      resolveReleaseRepository({
        GITHUB_REPOSITORY: 'andrewyatesai/orca-alab',
        ORCA_RELEASE_REPOSITORY: ' release-owner/release-repo '
      })
    ).toBe('release-owner/release-repo')
  })

  it('routes release-note generation to development history', () => {
    expect(resolveReleaseNotesRepository({ GITHUB_REPOSITORY: 'other/source-repo' })).toBe(
      DEFAULT_RELEASE_NOTES_REPOSITORY
    )
    expect(DEFAULT_RELEASE_NOTES_REPOSITORY).toBe('andrewyatesai/orca-alab')
    expect(
      resolveReleaseNotesRepository({
        ORCA_RELEASE_NOTES_REPOSITORY: ' development-owner/development-repo '
      })
    ).toBe('development-owner/development-repo')
  })

  it('wires every release entrypoint through the dedicated resolver', async () => {
    const entrypoints = [
      'create-draft-release.mjs',
      'latest-stable-release.mjs',
      'publish-complete-draft-releases.mjs',
      'verify-release-required-assets.mjs',
      '../../.github/scripts/render-readme-downloads-badge.mjs'
    ]

    for (const entrypoint of entrypoints) {
      const source = await readFile(new URL(entrypoint, import.meta.url), 'utf8')
      expect(source).toContain('resolveReleaseRepository(process.env)')
      expect(source).not.toContain('GITHUB_REPOSITORY')
    }

    const draftReleaseSource = await readFile(
      new URL('create-draft-release.mjs', import.meta.url),
      'utf8'
    )
    expect(draftReleaseSource).toContain('resolveReleaseNotesRepository(process.env)')
  })
})
