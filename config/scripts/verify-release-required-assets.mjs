#!/usr/bin/env node

import { createHash, timingSafeEqual } from 'node:crypto'
import { pathToFileURL } from 'node:url'
import { parse as parseYaml } from 'yaml'
import { desktopReleaseVersion } from './desktop-release-tag.mjs'
import { resolveReleaseRepository } from './release-repository.mjs'

const API_VERSION = '2022-11-28'

export const RELEASE_ASSET_PROFILES = Object.freeze({
  ALAB_MACOS: 'alab-macos',
  ALAB_FULL: 'alab-full',
  LEGACY_FULL: 'legacy-full'
})
export const DEFAULT_RELEASE_ASSET_PROFILE = RELEASE_ASSET_PROFILES.ALAB_MACOS

export function assertReleaseAssetProfile(profile) {
  if (!Object.values(RELEASE_ASSET_PROFILES).includes(profile)) {
    throw new Error(`Unknown release asset profile: ${profile}`)
  }
  return profile
}

function macAssetNames(version, { legacy = false } = {}) {
  const x64Zip = legacy ? `Orca-${version}-mac.zip` : `orca-staging-${version}-x64-mac.zip`
  const arm64Zip = legacy
    ? `Orca-${version}-arm64-mac.zip`
    : `orca-staging-${version}-arm64-mac.zip`
  return [
    'latest-mac.yml',
    x64Zip,
    `${x64Zip}.blockmap`,
    arm64Zip,
    `${arm64Zip}.blockmap`,
    'orca-macos-x64.dmg',
    'orca-macos-x64.dmg.blockmap',
    'orca-macos-arm64.dmg',
    'orca-macos-arm64.dmg.blockmap'
  ]
}

function fullAssetNames(version, { legacy = false } = {}) {
  const assets = [
    ...macAssetNames(version, { legacy }),
    'latest-linux.yml',
    'latest-linux-arm64.yml',
    'latest.yml',
    'orca-linux.AppImage',
    'orca-linux-arm64.AppImage',
    `orca-ide_${version}_amd64.deb`,
    `orca-ide_${version}_arm64.deb`,
    'orca-windows-setup.exe',
    'orca-windows-setup.exe.blockmap'
  ]
  if (legacy) {
    assets.push(`orca-ide-${version}.x86_64.rpm`, `orca-ide-${version}.aarch64.rpm`)
  }
  return assets
}

function resolveAssetProfile(profile, version) {
  assertReleaseAssetProfile(profile)
  switch (profile) {
    case RELEASE_ASSET_PROFILES.ALAB_MACOS:
      return { required: macAssetNames(version), manifests: ['latest-mac.yml'] }
    case RELEASE_ASSET_PROFILES.ALAB_FULL:
      return {
        required: fullAssetNames(version),
        manifests: ['latest-mac.yml', 'latest-linux.yml', 'latest-linux-arm64.yml', 'latest.yml']
      }
    case RELEASE_ASSET_PROFILES.LEGACY_FULL:
      return {
        required: fullAssetNames(version, { legacy: true }),
        manifests: ['latest-mac.yml', 'latest-linux.yml', 'latest-linux-arm64.yml', 'latest.yml']
      }
  }
}

export function getRequiredReleaseAssetNames(
  tag,
  { profile = DEFAULT_RELEASE_ASSET_PROFILE } = {}
) {
  const version = desktopReleaseVersion(tag)
  if (!version) {
    throw new Error(`Invalid desktop release tag: ${tag}`)
  }
  return resolveAssetProfile(profile, version).required
}

function manifestAssetName(value) {
  // Why: the updater may follow absolute URLs, so only a same-release asset basename proves these bytes.
  return /^[A-Za-z0-9][A-Za-z0-9._+-]*$/.test(value) ? value : ''
}

function canonicalSha512(value, context) {
  if (typeof value !== 'string') {
    throw new Error(`${context} has no sha512`)
  }
  const decoded = Buffer.from(value, 'base64')
  if (decoded.length !== 64 || decoded.toString('base64') !== value) {
    throw new Error(`${context} has an invalid sha512`)
  }
  return value
}

export function parseUpdateManifest(manifestText, manifestName = 'update manifest') {
  let document
  try {
    document = parseYaml(manifestText)
  } catch (error) {
    throw new Error(`${manifestName} is not valid YAML: ${error.message}`)
  }
  if (!document || typeof document !== 'object' || Array.isArray(document)) {
    throw new Error(`${manifestName} must contain a YAML mapping`)
  }
  if (typeof document.version !== 'string') {
    throw new Error(`${manifestName} has no version`)
  }

  if (document.files !== undefined && !Array.isArray(document.files)) {
    throw new Error(`${manifestName} files must be a list`)
  }
  const rawFiles = document.files ?? []
  if (document.path !== undefined || document.sha512 !== undefined) {
    rawFiles.push({ url: document.path, sha512: document.sha512 })
  }
  if (rawFiles.length === 0) {
    throw new Error(`${manifestName} has no files`)
  }

  const filesByName = new Map()
  rawFiles.forEach((file, index) => {
    const context = `${manifestName} file ${index + 1}`
    if (!file || typeof file !== 'object' || typeof file.url !== 'string') {
      throw new Error(`${context} has no URL`)
    }
    const name = manifestAssetName(file.url)
    if (!name) {
      throw new Error(`${context} has an invalid URL`)
    }
    const sha512 = canonicalSha512(file.sha512, context)
    const size = file.size
    if (size !== undefined && (!Number.isSafeInteger(size) || size <= 0)) {
      throw new Error(`${context} has an invalid size`)
    }
    const previous = filesByName.get(name)
    if (
      previous &&
      (previous.sha512 !== sha512 || (previous.size && size && previous.size !== size))
    ) {
      throw new Error(`${manifestName} has conflicting metadata for ${name}`)
    }
    filesByName.set(name, { name, sha512, size: previous?.size ?? size })
  })

  return { version: document.version, files: [...filesByName.values()] }
}

export function extractManifestAssetNames(manifestText) {
  return parseUpdateManifest(manifestText).files.map((file) => file.name)
}

async function githubFetch(fetchImpl, url, token, accept = 'application/vnd.github+json') {
  const res = await fetchImpl(url, {
    headers: {
      Accept: accept,
      Authorization: `Bearer ${token}`,
      'X-GitHub-Api-Version': API_VERSION
    }
  })
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(`GitHub request failed ${res.status} ${res.statusText}: ${body.slice(0, 300)}`)
  }
  return res
}

async function fetchRelease(repo, tag, token, fetchImpl) {
  // Why: a tag-specific lookup cannot silently miss a draft beyond the first releases page.
  const res = await githubFetch(
    fetchImpl,
    `https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`,
    token
  )
  const release = await res.json()
  if (release?.tag_name !== tag) {
    throw new Error(`GitHub did not return the exact release ${repo}@${tag}`)
  }
  if (!Array.isArray(release.assets)) {
    throw new Error(`Release ${repo}@${tag} has no asset list`)
  }
  return release
}

async function fetchAsset(repo, asset, token, fetchImpl) {
  return githubFetch(
    fetchImpl,
    `https://api.github.com/repos/${repo}/releases/assets/${asset.id}`,
    token,
    'application/octet-stream'
  )
}

async function sha512Response(response) {
  const hash = createHash('sha512')
  let byteLength = 0
  if (response.body?.getReader) {
    const reader = response.body.getReader()
    for (;;) {
      const { done, value } = await reader.read()
      if (done) {
        break
      }
      hash.update(value)
      byteLength += value.byteLength
    }
  } else if (response.arrayBuffer) {
    const bytes = Buffer.from(await response.arrayBuffer())
    hash.update(bytes)
    byteLength = bytes.byteLength
  } else {
    throw new Error('GitHub asset response did not expose binary bytes')
  }
  return { digest: hash.digest(), byteLength }
}

function requiredAssetFailures(requiredNames, assetsByName) {
  const missing = [...requiredNames].filter((name) => !assetsByName.has(name)).sort()
  const notUploaded = [...requiredNames]
    .map((name) => assetsByName.get(name))
    .filter((asset) => asset && asset.state && asset.state !== 'uploaded')
    .map((asset) => `${asset.name}:${asset.state}`)
    .sort()
  const empty = [...requiredNames]
    .map((name) => assetsByName.get(name))
    .filter((asset) => asset && asset.size === 0)
    .map((asset) => asset.name)
    .sort()
  return { missing, notUploaded, empty }
}

function assertRequiredAssets(tag, failures) {
  const { missing, notUploaded, empty } = failures
  if (missing.length === 0 && notUploaded.length === 0 && empty.length === 0) {
    return
  }
  throw new Error(
    [
      `Release ${tag} is missing required assets.`,
      missing.length > 0 ? `Missing: ${missing.join(', ')}` : null,
      notUploaded.length > 0 ? `Not uploaded: ${notUploaded.join(', ')}` : null,
      empty.length > 0 ? `Empty: ${empty.join(', ')}` : null
    ]
      .filter(Boolean)
      .join('\n')
  )
}

export async function verifyRequiredReleaseAssets({
  repo,
  tag,
  token,
  profile = DEFAULT_RELEASE_ASSET_PROFILE,
  fetchImpl = fetch
}) {
  if (!repo) {
    throw new Error('repo is required')
  }
  if (!token) {
    throw new Error('token is required')
  }
  const version = desktopReleaseVersion(tag)
  if (!version) {
    throw new Error(`Invalid desktop release tag: ${tag}`)
  }
  const assetProfile = resolveAssetProfile(profile, version)
  const release = await fetchRelease(repo, tag, token, fetchImpl)
  const assetsByName = new Map(release.assets.map((asset) => [asset.name, asset]))
  const requiredNames = new Set(assetProfile.required)
  const manifestFiles = new Map()

  for (const manifestName of assetProfile.manifests) {
    const manifestAsset = assetsByName.get(manifestName)
    if (!manifestAsset) {
      continue
    }
    const response = await fetchAsset(repo, manifestAsset, token, fetchImpl)
    const manifest = parseUpdateManifest(await response.text(), manifestName)
    if (manifest.version !== version) {
      throw new Error(`${manifestName} version ${manifest.version} does not match tag ${tag}`)
    }
    for (const file of manifest.files) {
      requiredNames.add(file.name)
      const previous = manifestFiles.get(file.name)
      if (previous && previous.sha512 !== file.sha512) {
        throw new Error(`Update manifests disagree on sha512 for ${file.name}`)
      }
      if (previous?.size && file.size && previous.size !== file.size) {
        throw new Error(`Update manifests disagree on size for ${file.name}`)
      }
      manifestFiles.set(file.name, { ...file, size: previous?.size ?? file.size })
    }
  }

  assertRequiredAssets(tag, requiredAssetFailures(requiredNames, assetsByName))

  for (const file of manifestFiles.values()) {
    const asset = assetsByName.get(file.name)
    if (file.size && asset.size !== file.size) {
      throw new Error(`${file.name} size ${asset.size} does not match manifest size ${file.size}`)
    }
    const response = await fetchAsset(repo, asset, token, fetchImpl)
    const actual = await sha512Response(response)
    if (actual.byteLength !== asset.size) {
      throw new Error(
        `${file.name} downloaded size ${actual.byteLength} does not match ${asset.size}`
      )
    }
    const expectedDigest = Buffer.from(file.sha512, 'base64')
    if (!timingSafeEqual(actual.digest, expectedDigest)) {
      throw new Error(`${file.name} sha512 does not match the uploaded bytes`)
    }
  }

  return {
    tag,
    profile,
    checked: [...requiredNames].sort(),
    draft: release.draft,
    prerelease: release.prerelease
  }
}

async function main() {
  const tag = process.argv[2]
  if (!tag) {
    throw new Error('Usage: node config/scripts/verify-release-required-assets.mjs <tag>')
  }
  const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN
  if (!token) {
    throw new Error('GH_TOKEN or GITHUB_TOKEN must be set')
  }
  const repo = resolveReleaseRepository(process.env)
  const profile = process.env.ORCA_RELEASE_ASSET_PROFILE || DEFAULT_RELEASE_ASSET_PROFILE
  const result = await verifyRequiredReleaseAssets({ repo, tag, token, profile })
  console.log(`Verified ${result.checked.length} ${profile} release assets for ${repo}@${tag}`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message)
    process.exit(1)
  })
}
