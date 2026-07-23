import { describe, expect, it, vi } from 'vitest'
import { isDesktopReleasePrerelease } from './desktop-release-tag.mjs'
import {
  createDraftRelease,
  latestPreviousPublishedDesktopReleaseTag,
  parseDesktopReleaseTag,
  truncateReleaseBody
} from './create-draft-release.mjs'

function release(tag, options = {}) {
  return {
    draft: false,
    tag_name: tag,
    ...options
  }
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

function createReleaseFetch({
  repo,
  notesRepo = repo,
  tag,
  releasePages = [[]],
  notes = { name: tag, body: 'notes' },
  missingRefRepos = []
}) {
  return vi.fn(async (url, options = {}) => {
    if (url.includes('/git/ref/tags/')) {
      const requestRepo = url.match(/repos\/(.+?)\/git\/ref/)?.[1]
      if (missingRefRepos.includes(requestRepo)) {
        return jsonResponse('missing', { ok: false, status: 404, statusText: 'Not Found' })
      }
      return jsonResponse({ ref: `refs/tags/${tag}` })
    }
    if (url.startsWith(`https://api.github.com/repos/${repo}/releases?`)) {
      const page = Number(new URL(url).searchParams.get('page'))
      return jsonResponse(releasePages[page - 1] ?? [])
    }
    if (url === `https://api.github.com/repos/${notesRepo}/releases/generate-notes`) {
      return jsonResponse(notes)
    }
    if (url === `https://api.github.com/repos/${repo}/releases` && options.method === 'POST') {
      return jsonResponse({ tag_name: tag, draft: true })
    }
    throw new Error(`Unexpected GitHub request: ${url}`)
  })
}

describe('truncateReleaseBody', () => {
  it('leaves short release notes unchanged', () => {
    expect(truncateReleaseBody('short notes', 120_000)).toBe('short notes')
  })

  it('caps long release notes and appends an explanation', () => {
    const body = truncateReleaseBody('a'.repeat(130_000), 1_000)

    expect(body).toHaveLength(1_000)
    expect(body).toContain('Release notes were truncated')
  })
})

describe('parseDesktopReleaseTag', () => {
  it('parses canonical ALab desktop release tags', () => {
    expect(parseDesktopReleaseTag('v1.4.147-fork.3')).toEqual({
      tag: 'v1.4.147-fork.3',
      major: 1,
      minor: 4,
      patch: 147,
      rc: null,
      fork: 3
    })
    expect(parseDesktopReleaseTag('v1.4.147-fork.0')).toBeNull()
    expect(parseDesktopReleaseTag('v0.4.147-fork.1')).toBeNull()
    expect(parseDesktopReleaseTag('v1.04.147-fork.1')).toBeNull()
    expect(parseDesktopReleaseTag('v1.4.147-fork.01')).toBeNull()
    expect(parseDesktopReleaseTag('v9007199254740992.4.147-fork.1')).toBeNull()
  })

  it('retains stable and rc desktop release tag support', () => {
    expect(parseDesktopReleaseTag('v1.4.36')).toMatchObject({
      tag: 'v1.4.36',
      major: 1,
      minor: 4,
      patch: 36,
      rc: null,
      fork: null
    })
    expect(parseDesktopReleaseTag('v1.4.36-rc.2')).toMatchObject({
      tag: 'v1.4.36-rc.2',
      major: 1,
      minor: 4,
      patch: 36,
      rc: 2,
      fork: null
    })
    expect(parseDesktopReleaseTag('mobile-v0.0.12')).toBeNull()
  })

  it('classifies only canonical rc tags as GitHub prereleases', () => {
    expect(isDesktopReleasePrerelease('v1.4.36-rc.2')).toBe(true)
    expect(isDesktopReleasePrerelease('v1.4.147-fork.3')).toBe(false)
    expect(isDesktopReleasePrerelease('not-a-release')).toBe(false)
  })
})

describe('latestPreviousPublishedDesktopReleaseTag', () => {
  it('bounds ALab notes to the previous published fork cut', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [
          release('v1.4.146-fork.4'),
          release('v1.4.147-fork.1'),
          release('v1.4.147-fork.2'),
          release('v1.4.147')
        ],
        'v1.4.147-fork.3'
      )
    ).toBe('v1.4.147-fork.2')
  })

  it('continues the ALab changelog across upstream-base changes', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.146-fork.4'), release('v1.4.147-fork.1')],
        'v1.4.148-fork.1'
      )
    ).toBe('v1.4.147-fork.1')
  })

  it('does not cross from the ALab release train into legacy stable or rc tags', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.146'), release('v1.4.147-rc.0')],
        'v1.4.147-fork.1'
      )
    ).toBe('')
  })

  it('bounds stable notes to the previous stable release when rcs exist', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.35'), release('v1.4.36-rc.0'), release('v1.4.36')],
        'v1.4.36'
      )
    ).toBe('v1.4.35')
  })

  it('does not collapse a stable changelog to its rc-to-stable version bump', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [
          release('v1.4.120'),
          release('v1.4.121-rc.0'),
          release('v1.4.121-rc.6'),
          release('v1.4.121')
        ],
        'v1.4.121'
      )
    ).toBe('v1.4.120')
  })

  it('bounds the first rc notes to the previous stable release', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.35'), release('v1.4.36-rc.0'), release('mobile-v0.0.12')],
        'v1.4.36-rc.0'
      )
    ).toBe('v1.4.35')
  })

  it('bounds later rc notes to the prior rc', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.36-rc.0'), release('v1.4.36-rc.1')],
        'v1.4.36-rc.1'
      )
    ).toBe('v1.4.36-rc.0')
  })

  it('ignores draft releases as public changelog boundaries', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.35'), release('v1.4.36-rc.0', { draft: true }), release('v1.4.36-rc.1')],
        'v1.4.36-rc.1'
      )
    ).toBe('v1.4.35')
  })

  it('returns empty string for the first desktop release when no earlier tag exists', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.36'), release('mobile-v0.0.12')],
        'v1.4.36'
      )
    ).toBe('')
    expect(latestPreviousPublishedDesktopReleaseTag([], 'v1.4.36')).toBe('')
  })

  it('returns empty string when the current tag is not a desktop release tag', () => {
    expect(
      latestPreviousPublishedDesktopReleaseTag(
        [release('v1.4.35'), release('v1.4.36')],
        'mobile-v0.0.12'
      )
    ).toBe('')
  })
})

describe('createDraftRelease', () => {
  it('creates an ALab draft against the public release repository', async () => {
    const tag = 'v1.4.147-fork.3'
    const repo = 'alabsystems/orca-alab'
    const notesRepo = 'andrewyatesai/orca-alab'
    const fetchImpl = createReleaseFetch({
      repo,
      notesRepo,
      tag,
      releasePages: [[release('v1.4.147-fork.1'), release('v1.4.147-fork.2')]],
      notes: { name: tag, body: 'ALab notes' }
    })

    await createDraftRelease({
      repo,
      notesRepo,
      tag,
      token: 'token',
      fetchImpl,
      log: vi.fn()
    })

    const urls = fetchImpl.mock.calls.map(([url]) => url)
    expect(urls.slice(0, 2)).toEqual([
      `https://api.github.com/repos/${notesRepo}/git/ref/tags/${tag}`,
      `https://api.github.com/repos/${repo}/git/ref/tags/${tag}`
    ])
    const generateNotesCall = fetchImpl.mock.calls.find(([url]) =>
      url.endsWith('/releases/generate-notes')
    )
    const createCall = fetchImpl.mock.calls.find(
      ([url, options]) => url === `https://api.github.com/repos/${repo}/releases` && options.method
    )
    const generateNotesBody = JSON.parse(generateNotesCall[1].body)
    expect(generateNotesBody).toEqual({
      tag_name: tag,
      target_commitish: tag,
      previous_tag_name: 'v1.4.147-fork.2'
    })
    const createBody = JSON.parse(createCall[1].body)
    expect(createBody).toMatchObject({
      tag_name: tag,
      name: tag,
      draft: true,
      prerelease: false
    })
  })

  it('creates a draft release with bounded generated notes', async () => {
    const repo = 'stablyai/orca'
    const tag = 'v1.4.36'
    const fetchImpl = createReleaseFetch({
      repo,
      tag,
      releasePages: [[release('v1.4.35'), release(tag)]],
      notes: { name: tag, body: 'a'.repeat(130_000) }
    })

    await createDraftRelease({
      repo,
      notesRepo: repo,
      tag,
      token: 'token',
      fetchImpl,
      log: vi.fn()
    })

    const generateNotesCall = fetchImpl.mock.calls.find(([url]) =>
      url.endsWith('/releases/generate-notes')
    )
    expect(generateNotesCall[1]).toEqual(
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          tag_name: tag,
          target_commitish: tag,
          previous_tag_name: 'v1.4.35'
        })
      })
    )
    const createCall = fetchImpl.mock.calls.find(
      ([url, options]) => url === `https://api.github.com/repos/${repo}/releases` && options.method
    )
    const createBody = JSON.parse(createCall[1].body)
    expect(createBody).toMatchObject({
      tag_name: 'v1.4.36',
      name: 'v1.4.36',
      draft: true,
      prerelease: false
    })
    expect(createBody.body).toHaveLength(120_000)
    expect(createBody.body).toContain('Release notes were truncated')
  })

  it('marks rc tags as prereleases', async () => {
    const repo = 'stablyai/orca'
    const tag = 'v1.4.36-rc.1'
    const fetchImpl = createReleaseFetch({
      repo,
      tag,
      releasePages: [[release('v1.4.36'), release(tag)]]
    })

    await createDraftRelease({
      repo,
      notesRepo: repo,
      tag,
      token: 'token',
      fetchImpl,
      log: vi.fn()
    })

    const createCall = fetchImpl.mock.calls.find(
      ([url, options]) => url === `https://api.github.com/repos/${repo}/releases` && options.method
    )
    const createBody = JSON.parse(createCall[1].body)
    expect(createBody.prerelease).toBe(true)
  })

  it('omits previous_tag_name for the first desktop release so notes fall back to the GitHub default', async () => {
    const repo = 'stablyai/orca'
    const tag = 'v1.4.36'
    const fetchImpl = createReleaseFetch({
      repo,
      tag,
      releasePages: [[release(tag), release('mobile-v0.0.12')]]
    })

    await createDraftRelease({
      repo,
      notesRepo: repo,
      tag,
      token: 'token',
      fetchImpl,
      log: vi.fn()
    })

    const generateNotesCall = fetchImpl.mock.calls.find(([url]) =>
      url.endsWith('/releases/generate-notes')
    )
    const generateNotesBody = JSON.parse(generateNotesCall[1].body)
    expect(generateNotesBody).toEqual({ tag_name: tag, target_commitish: tag })
    expect(generateNotesBody).not.toHaveProperty('previous_tag_name')
  })

  it('paginates through every release page before choosing the previous release', async () => {
    const firstPage = Array.from({ length: 100 }, (_, index) => release(`mobile-v0.0.${index}`))
    const repo = 'stablyai/orca'
    const tag = 'v1.4.36'
    const fetchImpl = createReleaseFetch({
      repo,
      tag,
      releasePages: [firstPage, [release('v1.4.35')]]
    })

    await createDraftRelease({
      repo,
      notesRepo: repo,
      tag,
      token: 'token',
      fetchImpl,
      log: vi.fn()
    })

    expect(fetchImpl).toHaveBeenNthCalledWith(
      3,
      'https://api.github.com/repos/stablyai/orca/releases?per_page=100&page=1',
      expect.any(Object)
    )
    expect(fetchImpl).toHaveBeenNthCalledWith(
      4,
      'https://api.github.com/repos/stablyai/orca/releases?per_page=100&page=2',
      expect.any(Object)
    )
    const generateNotesCall = fetchImpl.mock.calls.find(([url]) =>
      url.endsWith('/releases/generate-notes')
    )
    const generateNotesBody = JSON.parse(generateNotesCall[1].body)
    expect(generateNotesBody.previous_tag_name).toBe('v1.4.35')
  })

  it('does not POST when either repository lacks the exact tag ref', async () => {
    const repo = 'alabsystems/orca-alab'
    const notesRepo = 'andrewyatesai/orca-alab'
    const tag = 'v1.4.147-fork.3'
    const fetchImpl = createReleaseFetch({
      repo,
      notesRepo,
      tag,
      missingRefRepos: [repo]
    })

    await expect(
      createDraftRelease({ repo, notesRepo, tag, token: 'token', fetchImpl })
    ).rejects.toThrow('GitHub request failed 404')
    expect(fetchImpl.mock.calls.some(([, options]) => options.method === 'POST')).toBe(false)
  })

  it('rejects a noncanonical tag before making a GitHub request', async () => {
    const fetchImpl = vi.fn()
    await expect(
      createDraftRelease({
        repo: 'alabsystems/orca-alab',
        notesRepo: 'andrewyatesai/orca-alab',
        tag: 'v1.4.147-fork.0',
        token: 'token',
        fetchImpl
      })
    ).rejects.toThrow('Invalid desktop release tag')
    expect(fetchImpl).not.toHaveBeenCalled()
  })
})
