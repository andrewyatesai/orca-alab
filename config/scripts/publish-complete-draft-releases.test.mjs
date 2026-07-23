import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import {
  isTagBuiltFromCurrentRef,
  isReleaseCutDraft,
  publishCompleteDraftReleases,
  resolveReleaseDraftAuthors,
  writeGithubOutputs
} from './publish-complete-draft-releases.mjs'

function git(cwd, args) {
  return execFileSync('git', args, {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe']
  }).trim()
}

function withGitRepo(run) {
  const dir = mkdtempSync(join(tmpdir(), 'orca-draft-release-'))
  try {
    git(dir, ['init', '--initial-branch=main'])
    git(dir, ['config', 'user.name', 'Test Bot'])
    git(dir, ['config', 'user.email', 'test@example.com'])
    run(dir)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

function commit(cwd, message) {
  git(cwd, ['commit', '--allow-empty', '-m', message])
}

function jsonResponse(body, init = {}) {
  return {
    ok: init.ok ?? true,
    status: init.status ?? 200,
    statusText: init.statusText ?? 'OK',
    json: vi.fn(async () => body),
    text: vi.fn(async () => (typeof body === 'string' ? body : JSON.stringify(body)))
  }
}

describe('isReleaseCutDraft', () => {
  it('accepts trusted ALab fork drafts and legacy bot-authored RC drafts', () => {
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'v1.4.147-fork.2',
        author: { login: 'alabsystems' }
      })
    ).toBe(true)
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'v1.4.2-rc.7',
        author: { login: 'github-actions[bot]' }
      })
    ).toBe(true)
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'mobile-v0.0.7',
        author: { login: 'github-actions[bot]' }
      })
    ).toBe(false)
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'v1.4.147-fork.2',
        author: { login: 'untrusted-user' }
      })
    ).toBe(false)
    expect(
      isReleaseCutDraft({
        draft: false,
        tag_name: 'v1.4.2',
        author: { login: 'github-actions[bot]' }
      })
    ).toBe(false)
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'v1.4.2',
        author: { login: 'github-actions[bot]' }
      })
    ).toBe(true)
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'v0.4.2-fork.1',
        author: { login: 'github-actions[bot]' }
      })
    ).toBe(false)
    expect(
      isReleaseCutDraft({
        draft: true,
        tag_name: 'v1.4.2-fork.0',
        author: { login: 'github-actions[bot]' }
      })
    ).toBe(false)
  })

  it('allows an explicit cross-repository service identity', () => {
    expect(
      isReleaseCutDraft(
        {
          draft: true,
          tag_name: 'v1.4.147-fork.2',
          author: { login: 'release-service' }
        },
        { allowedAuthors: ['release-service'] }
      )
    ).toBe(true)
    expect(
      resolveReleaseDraftAuthors({
        ORCA_RELEASE_DRAFT_AUTHORS: 'release-service, andrewyatesai'
      })
    ).toEqual(['release-service', 'andrewyatesai'])
  })
})

describe('isTagBuiltFromCurrentRef', () => {
  it('accepts a tag on the current release commit', () => {
    withGitRepo((repo) => {
      commit(repo, 'initial')
      commit(repo, 'release: v1.4.36-rc.6')
      git(repo, ['tag', 'v1.4.36-rc.6'])

      expect(isTagBuiltFromCurrentRef('v1.4.36-rc.6', { cwd: repo })).toBe(true)
    })
  })

  it('accepts a tag whose release commit is built from the current ref', () => {
    withGitRepo((repo) => {
      commit(repo, 'initial')
      const source = git(repo, ['rev-parse', 'HEAD'])
      commit(repo, 'release: v1.4.36-rc.6')
      git(repo, ['tag', 'v1.4.36-rc.6'])
      git(repo, ['checkout', source])

      expect(isTagBuiltFromCurrentRef('v1.4.36-rc.6', { cwd: repo })).toBe(true)
    })
  })

  it('rejects a stale tag when the current ref has moved on', () => {
    withGitRepo((repo) => {
      commit(repo, 'initial')
      commit(repo, 'release: v1.4.36-rc.6')
      git(repo, ['tag', 'v1.4.36-rc.6'])
      commit(repo, 'fix: release packaging')

      expect(isTagBuiltFromCurrentRef('v1.4.36-rc.6', { cwd: repo })).toBe(false)
    })
  })
})

describe('publishCompleteDraftReleases', () => {
  it('publishes complete ALab fork drafts and skips incomplete ones', async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse([
          {
            id: 7,
            draft: true,
            tag_name: 'v1.4.147-fork.1',
            created_at: '2026-05-15T07:31:19Z',
            author: { login: 'github-actions[bot]' }
          },
          {
            id: 8,
            draft: true,
            tag_name: 'v1.4.147-fork.2',
            created_at: '2026-05-15T10:51:57Z',
            author: { login: 'github-actions[bot]' }
          }
        ])
      )
      .mockResolvedValueOnce(jsonResponse({ tag_name: 'v1.4.147-fork.1', draft: false }))
    const verifyReleaseAssets = vi.fn(async ({ tag }) => {
      if (tag === 'v1.4.147-fork.2') {
        throw new Error('Release v1.4.147-fork.2 is missing required assets.')
      }
    })
    const log = vi.fn()

    const result = await publishCompleteDraftReleases({
      repo: 'alabsystems/orca-alab',
      token: 'token',
      fetchImpl,
      verifyReleaseAssets,
      isDraftBuiltFromCurrentRef: vi.fn(async () => true),
      log
    })

    expect(result).toEqual({
      published: ['v1.4.147-fork.1'],
      skipped: [
        {
          tag: 'v1.4.147-fork.2',
          reason: 'Release v1.4.147-fork.2 is missing required assets.'
        }
      ]
    })
    expect(fetchImpl).toHaveBeenLastCalledWith(
      'https://api.github.com/repos/alabsystems/orca-alab/releases/7',
      expect.objectContaining({
        method: 'PATCH',
        body: JSON.stringify({ draft: false, prerelease: false })
      })
    )
    expect(verifyReleaseAssets).toHaveBeenCalledWith(
      expect.objectContaining({ profile: 'alab-macos' })
    )
  })

  it('skips stale complete drafts before publishing', async () => {
    const fetchImpl = vi.fn().mockResolvedValueOnce(
      jsonResponse([
        {
          id: 7,
          draft: true,
          tag_name: 'v1.4.2-rc.7',
          created_at: '2026-05-15T07:31:19Z',
          author: { login: 'github-actions[bot]' }
        }
      ])
    )
    const verifyReleaseAssets = vi.fn()
    const log = vi.fn()

    const result = await publishCompleteDraftReleases({
      repo: 'alabsystems/orca-alab',
      token: 'token',
      fetchImpl,
      verifyReleaseAssets,
      isDraftBuiltFromCurrentRef: vi.fn(async () => false),
      log
    })

    expect(result).toEqual({
      published: [],
      skipped: [{ tag: 'v1.4.2-rc.7', reason: 'tag is not built from the current release ref' }]
    })
    expect(verifyReleaseAssets).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
    expect(fetchImpl).toHaveBeenCalledWith(
      'https://api.github.com/repos/alabsystems/orca-alab/releases?per_page=100&page=1',
      expect.any(Object)
    )
  })

  it('paginates beyond the first 100 releases before publishing an exact candidate', async () => {
    const firstPage = Array.from({ length: 100 }, (_, index) => ({
      id: index,
      draft: false,
      tag_name: `mobile-v0.0.${index}`,
      author: { login: 'github-actions[bot]' }
    }))
    const candidate = {
      id: 201,
      draft: true,
      tag_name: 'v1.4.148-fork.1',
      created_at: '2026-05-16T10:51:57Z',
      author: { login: 'alabsystems' }
    }
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(firstPage))
      .mockResolvedValueOnce(jsonResponse([candidate]))
      .mockResolvedValueOnce(jsonResponse({ ...candidate, draft: false }))
    const verifyReleaseAssets = vi.fn()

    const result = await publishCompleteDraftReleases({
      repo: 'alabsystems/orca-alab',
      token: 'token',
      fetchImpl,
      verifyReleaseAssets,
      isDraftBuiltFromCurrentRef: vi.fn(async () => true),
      assetProfile: 'alab-full',
      log: vi.fn()
    })

    expect(result.published).toEqual(['v1.4.148-fork.1'])
    expect(fetchImpl).toHaveBeenNthCalledWith(
      2,
      'https://api.github.com/repos/alabsystems/orca-alab/releases?per_page=100&page=2',
      expect.any(Object)
    )
    expect(verifyReleaseAssets).toHaveBeenCalledWith(
      expect.objectContaining({ tag: 'v1.4.148-fork.1', profile: 'alab-full' })
    )
  })

  it('rejects an unknown asset profile before querying or publishing', async () => {
    const fetchImpl = vi.fn()
    await expect(
      publishCompleteDraftReleases({
        repo: 'alabsystems/orca-alab',
        token: 'token',
        fetchImpl,
        assetProfile: 'everything'
      })
    ).rejects.toThrow('Unknown release asset profile')
    expect(fetchImpl).not.toHaveBeenCalled()
  })
})

describe('writeGithubOutputs', () => {
  it('writes count outputs for workflow conditions', () => {
    const dir = mkdtempSync(join(tmpdir(), 'orca-release-outputs-'))
    const outputPath = join(dir, 'output')
    try {
      writeGithubOutputs(
        {
          published: ['v1.4.2-rc.7'],
          skipped: [{ tag: 'v1.4.2-rc.8', reason: 'missing assets' }]
        },
        outputPath
      )

      expect(readFileSync(outputPath, 'utf8')).toBe(
        `${[
          'published_count=1',
          'skipped_count=1',
          'latest_published_tag=v1.4.2-rc.7',
          'published_tags=v1.4.2-rc.7',
          'skipped_tags=v1.4.2-rc.8'
        ].join('\n')}\n`
      )
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
