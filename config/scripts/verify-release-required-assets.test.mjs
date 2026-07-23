import { describe, expect, it, vi } from 'vitest'
import {
  DEFAULT_RELEASE_ASSET_PROFILE,
  RELEASE_ASSET_PROFILES,
  extractManifestAssetNames,
  getRequiredReleaseAssetNames,
  parseUpdateManifest,
  verifyRequiredReleaseAssets
} from './verify-release-required-assets.mjs'

const TAG = 'v1.4.147-fork.3'
const VERSION = '1.4.147-fork.3'
// Fixed digests make the verifier tests independent from the values under test.
const PAYLOADS = {
  [`orca-staging-${VERSION}-x64-mac.zip`]: {
    bytes: 'x64 archive bytes\n',
    sha512:
      '6lKRGUwoldGlbUT5l+TK4Z8F50kSVCZR/Uf11dv8s8u2xeP2WlYneyoK5uVTRGl2jVCV8OyioAhtuKxfjePL8g=='
  },
  [`orca-staging-${VERSION}-arm64-mac.zip`]: {
    bytes: 'arm64 archive bytes\n',
    sha512:
      '/mH+csYgKDTYHOATGB90j/qhav0idFfIYT7d/XZPdlMfwMSi2LjfC6YXvifAP3OEze2g6XCEDeXpsOoZMgSXuQ=='
  },
  'orca-macos-x64.dmg': {
    bytes: 'x64 installer bytes\n',
    sha512:
      '/rYV/gI8v4Iy/VBne0hkTvHceX8QbEh4/jtMcBm0QiEiVyjisHVxz6iLw6pam5m6iF+gF0Z8b/gutyRtb2bF7g=='
  },
  'orca-macos-arm64.dmg': {
    bytes: 'arm64 installer bytes\n',
    sha512:
      'FBqJvNX7yxAY1UfigRSpok/YjwYw0oDwn0G611WbfDnpmY3jzqwEzr9VRE7kVyNsGPTC0dWHqpYZF1JbeG190A=='
  }
}

function response(body, init = {}) {
  const bytes = Buffer.isBuffer(body) ? body : Buffer.from(body)
  return {
    ok: init.ok ?? true,
    status: init.status ?? 200,
    statusText: init.statusText ?? 'OK',
    json: vi.fn(async () => (typeof body === 'string' ? JSON.parse(body) : body)),
    text: vi.fn(async () => bytes.toString('utf8')),
    arrayBuffer: vi.fn(async () => bytes)
  }
}

function manifestText({ version = VERSION, payloads = PAYLOADS } = {}) {
  const entries = Object.entries(payloads)
    .map(([name, payload]) =>
      [
        `  - url: ${name}`,
        `    sha512: ${payload.sha512}`,
        `    size: ${Buffer.byteLength(payload.bytes)}`
      ].join('\n')
    )
    .join('\n')
  const [firstName, firstPayload] = Object.entries(payloads)[0]
  return [
    `version: ${version}`,
    'files:',
    entries,
    `path: ${firstName}`,
    `sha512: ${firstPayload.sha512}`,
    "releaseDate: '2026-07-22T06:08:28.166Z'"
  ].join('\n')
}

function releaseFixture({ omitted = [], manifestVersion = VERSION, corruptAsset = '' } = {}) {
  const manifest = manifestText({ version: manifestVersion })
  const names = getRequiredReleaseAssetNames(TAG)
  const assets = names
    .filter((name) => !omitted.includes(name))
    .map((name, index) => ({
      id: index + 1,
      name,
      state: 'uploaded',
      size:
        name === 'latest-mac.yml'
          ? Buffer.byteLength(manifest)
          : Buffer.byteLength(PAYLOADS[name]?.bytes ?? 'blockmap')
    }))
  const release = { tag_name: TAG, draft: true, prerelease: false, assets }
  const assetsById = new Map(assets.map((asset) => [String(asset.id), asset]))
  const fetchImpl = vi.fn(async (url) => {
    if (url.endsWith(`/releases/tags/${TAG}`)) {
      return response(JSON.stringify(release))
    }
    const assetId = url.match(/\/releases\/assets\/(\d+)$/)?.[1]
    const asset = assetsById.get(assetId)
    if (!asset) {
      throw new Error(`Unexpected GitHub request: ${url}`)
    }
    if (asset.name === 'latest-mac.yml') {
      return response(manifest)
    }
    const payload = PAYLOADS[asset.name]
    if (!payload) {
      throw new Error(`Unexpected asset download: ${asset.name}`)
    }
    const bytes = asset.name === corruptAsset ? payload.bytes.replace(/^./, 'X') : payload.bytes
    return response(Buffer.from(bytes))
  })
  return { fetchImpl, release }
}

describe('release asset profiles', () => {
  it('defaults to the documented Mac-only ALab builder output', () => {
    const names = getRequiredReleaseAssetNames(TAG)
    expect(DEFAULT_RELEASE_ASSET_PROFILE).toBe('alab-macos')
    expect(names).toEqual([
      'latest-mac.yml',
      `orca-staging-${VERSION}-x64-mac.zip`,
      `orca-staging-${VERSION}-x64-mac.zip.blockmap`,
      `orca-staging-${VERSION}-arm64-mac.zip`,
      `orca-staging-${VERSION}-arm64-mac.zip.blockmap`,
      'orca-macos-x64.dmg',
      'orca-macos-x64.dmg.blockmap',
      'orca-macos-arm64.dmg',
      'orca-macos-arm64.dmg.blockmap'
    ])
    expect(names.some((name) => name.endsWith('.rpm'))).toBe(false)
  })

  it('requires cross-platform assets only with the explicit full profile', () => {
    const names = getRequiredReleaseAssetNames(TAG, {
      profile: RELEASE_ASSET_PROFILES.ALAB_FULL
    })
    expect(names).toEqual(
      expect.arrayContaining([
        'latest-linux-arm64.yml',
        'orca-linux-arm64.AppImage',
        `orca-ide_${VERSION}_arm64.deb`,
        'orca-windows-setup.exe'
      ])
    )
    expect(names.some((name) => name.endsWith('.rpm'))).toBe(false)
  })

  it('keeps legacy names and RPMs behind the explicit legacy profile', () => {
    const names = getRequiredReleaseAssetNames('v1.4.27', {
      profile: RELEASE_ASSET_PROFILES.LEGACY_FULL
    })
    expect(names).toEqual(
      expect.arrayContaining([
        'Orca-1.4.27-mac.zip',
        'Orca-1.4.27-arm64-mac.zip',
        'orca-ide-1.4.27.x86_64.rpm',
        'orca-ide-1.4.27.aarch64.rpm'
      ])
    )
  })
})

describe('parseUpdateManifest', () => {
  it('parses real electron-builder file records and de-duplicates path', () => {
    const manifest = manifestText()
    expect(parseUpdateManifest(manifest, 'latest-mac.yml')).toMatchObject({
      version: VERSION,
      files: expect.arrayContaining([
        {
          name: `orca-staging-${VERSION}-x64-mac.zip`,
          sha512: PAYLOADS[`orca-staging-${VERSION}-x64-mac.zip`].sha512,
          size: 18
        },
        {
          name: 'orca-macos-arm64.dmg',
          sha512: PAYLOADS['orca-macos-arm64.dmg'].sha512,
          size: 22
        }
      ])
    })
    expect(extractManifestAssetNames(manifest)).toHaveLength(4)
  })

  it('rejects malformed digest metadata', () => {
    expect(() =>
      parseUpdateManifest(
        ['version: 1.4.147-fork.3', 'path: archive.zip', 'sha512: not-a-digest'].join('\n')
      )
    ).toThrow('invalid sha512')
  })

  it('rejects external and path-qualified manifest URLs', () => {
    const sha512 = PAYLOADS[`orca-staging-${VERSION}-x64-mac.zip`].sha512
    for (const url of [
      'https://downloads.example/archive.zip',
      '//downloads.example/archive.zip',
      '../archive.zip',
      'folder/archive.zip',
      'archive.zip?mirror=1'
    ]) {
      expect(() =>
        parseUpdateManifest(
          ['version: 1.4.147-fork.3', `path: ${url}`, `sha512: ${sha512}`].join('\n')
        )
      ).toThrow('invalid URL')
    }
  })
})

describe('verifyRequiredReleaseAssets', () => {
  it('accepts the complete ALab Mac release and hashes every manifest payload', async () => {
    const { fetchImpl } = releaseFixture()
    const result = await verifyRequiredReleaseAssets({
      repo: 'alabsystems/orca-alab',
      tag: TAG,
      token: 'token',
      fetchImpl
    })

    expect(result).toMatchObject({
      tag: TAG,
      profile: RELEASE_ASSET_PROFILES.ALAB_MACOS,
      draft: true,
      prerelease: false
    })
    expect(fetchImpl).toHaveBeenNthCalledWith(
      1,
      `https://api.github.com/repos/alabsystems/orca-alab/releases/tags/${TAG}`,
      expect.any(Object)
    )
    expect(fetchImpl).toHaveBeenCalledTimes(6)
  })

  it('rejects a manifest version that does not exactly match the tag', async () => {
    const { fetchImpl } = releaseFixture({ manifestVersion: '1.4.147-fork.2' })
    await expect(
      verifyRequiredReleaseAssets({
        repo: 'alabsystems/orca-alab',
        tag: TAG,
        token: 'token',
        fetchImpl
      })
    ).rejects.toThrow('latest-mac.yml version 1.4.147-fork.2 does not match tag')
  })

  it('rejects uploaded bytes that do not match the manifest sha512', async () => {
    const corruptAsset = `orca-staging-${VERSION}-x64-mac.zip`
    const { fetchImpl } = releaseFixture({ corruptAsset })
    await expect(
      verifyRequiredReleaseAssets({
        repo: 'alabsystems/orca-alab',
        tag: TAG,
        token: 'token',
        fetchImpl
      })
    ).rejects.toThrow(`${corruptAsset} sha512 does not match the uploaded bytes`)
  })

  it('fails when a manifest-referenced archive was not uploaded', async () => {
    const missingAsset = `orca-staging-${VERSION}-arm64-mac.zip`
    const { fetchImpl } = releaseFixture({ omitted: [missingAsset] })
    await expect(
      verifyRequiredReleaseAssets({
        repo: 'alabsystems/orca-alab',
        tag: TAG,
        token: 'token',
        fetchImpl
      })
    ).rejects.toThrow(`Missing: ${missingAsset}`)
  })

  it('rejects noncanonical release tags before querying GitHub', async () => {
    const fetchImpl = vi.fn()
    await expect(
      verifyRequiredReleaseAssets({
        repo: 'alabsystems/orca-alab',
        tag: 'v1.4.147-fork.0',
        token: 'token',
        fetchImpl
      })
    ).rejects.toThrow('Invalid desktop release tag')
    expect(fetchImpl).not.toHaveBeenCalled()
  })
})
