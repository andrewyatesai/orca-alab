import { describe, expect, it, vi } from 'vitest'
import {
  fetchReleases,
  latestStableDesktopReleaseTag,
  parseDesktopStableTag
} from './latest-stable-release.mjs'

function jsonResponse(body, init = {}) {
  return {
    ok: init.ok ?? true,
    status: init.status ?? 200,
    statusText: init.statusText ?? 'OK',
    json: vi.fn(async () => body),
    text: vi.fn(async () => (typeof body === 'string' ? body : JSON.stringify(body)))
  }
}

describe('parseDesktopStableTag', () => {
  it('accepts canonical ALab desktop release tags', () => {
    expect(parseDesktopStableTag('v1.4.147-fork.3')).toEqual({
      tag: 'v1.4.147-fork.3',
      major: 1,
      minor: 4,
      patch: 147,
      fork: 3
    })
    expect(parseDesktopStableTag('v1.4.147-fork.0')).toBeNull()
    expect(parseDesktopStableTag('v1.4.44-rc.0')).toBeNull()
    expect(parseDesktopStableTag('mobile-v0.0.12')).toBeNull()
    expect(parseDesktopStableTag('cli-v4.12.28')).toBeNull()
  })

  it('retains support for legacy plain stable desktop tags', () => {
    expect(parseDesktopStableTag('v1.4.44')).toEqual({
      tag: 'v1.4.44',
      major: 1,
      minor: 4,
      patch: 44,
      fork: null
    })
  })
})

describe('latestStableDesktopReleaseTag', () => {
  it('chooses the newest ALab fork version and cut instead of release list order', () => {
    const releases = [
      { tag_name: 'v9.0.0', draft: false },
      { tag_name: 'v1.4.148-fork.1', draft: false },
      { tag_name: 'v1.4.147-fork.3', draft: false },
      { tag_name: 'v1.4.148-fork.2', draft: false },
      { tag_name: 'v1.4.149-rc.0', draft: false },
      { tag_name: 'mobile-v0.0.12', draft: false }
    ]

    expect(latestStableDesktopReleaseTag(releases)).toBe('v1.4.148-fork.2')
  })

  it('ignores draft ALab releases', () => {
    const releases = [
      { tag_name: 'v1.4.148-fork.2', draft: true },
      { tag_name: 'v1.4.148-fork.1', draft: false }
    ]

    expect(latestStableDesktopReleaseTag(releases)).toBe('v1.4.148-fork.1')
  })

  it('retains semver ordering for legacy plain stable releases', () => {
    const releases = [
      { tag_name: 'v1.4.42', draft: false },
      { tag_name: 'v1.4.44', draft: false },
      { tag_name: 'v1.4.43', draft: false }
    ]

    expect(latestStableDesktopReleaseTag(releases)).toBe('v1.4.44')
  })

  it('returns empty when no published stable desktop release exists', () => {
    expect(
      latestStableDesktopReleaseTag([
        { tag_name: 'v1.4.44-rc.0', draft: false },
        { tag_name: 'mobile-v0.0.12', draft: false }
      ])
    ).toBe('')
  })
})

describe('fetchReleases', () => {
  it('fetches all release pages', async () => {
    const firstPage = Array.from({ length: 100 }, (_, index) => ({
      tag_name: `v1.0.${index}`,
      draft: false
    }))
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(firstPage))
      .mockResolvedValueOnce(jsonResponse([{ tag_name: 'v1.4.44', draft: false }]))

    const releases = await fetchReleases('alabsystems/orca-alab', 'token', fetchImpl)

    expect(releases).toHaveLength(101)
    expect(fetchImpl).toHaveBeenNthCalledWith(
      1,
      'https://api.github.com/repos/alabsystems/orca-alab/releases?per_page=100&page=1',
      expect.any(Object)
    )
    expect(fetchImpl).toHaveBeenNthCalledWith(
      2,
      'https://api.github.com/repos/alabsystems/orca-alab/releases?per_page=100&page=2',
      expect.any(Object)
    )
  })
})
