#!/usr/bin/env node

import { pathToFileURL } from 'node:url'
import { parseDesktopReleaseTag } from './desktop-release-tag.mjs'
import { resolveReleaseRepository } from './release-repository.mjs'

const API_VERSION = '2022-11-28'

export function parseDesktopStableTag(tag) {
  const parsed = parseDesktopReleaseTag(tag)
  if (!parsed || parsed.rc !== null) {
    return null
  }

  return {
    tag: parsed.tag,
    major: parsed.major,
    minor: parsed.minor,
    patch: parsed.patch,
    fork: parsed.fork
  }
}

function compareDesktopStableTags(a, b) {
  const versionDiff = a.major - b.major || a.minor - b.minor || a.patch - b.patch
  if (versionDiff !== 0 || a.fork === b.fork) {
    return versionDiff
  }
  if (a.fork === null) {
    return 1
  }
  if (b.fork === null) {
    return -1
  }
  return a.fork - b.fork
}

export function latestStableDesktopReleaseTag(releases) {
  const stableTags = releases
    .filter((release) => release?.draft !== true)
    .map((release) => parseDesktopStableTag(release?.tag_name ?? release?.tagName ?? ''))
    .filter(Boolean)

  // Why: imported upstream-style tags must never advance ALab's independent fork release train.
  const forkTags = stableTags.filter((release) => release.fork !== null)
  return (
    (forkTags.length > 0 ? forkTags : stableTags).sort(compareDesktopStableTags).at(-1)?.tag ?? ''
  )
}

async function githubJson(fetchImpl, url, token) {
  const res = await fetchImpl(url, {
    headers: {
      Accept: 'application/vnd.github+json',
      Authorization: `Bearer ${token}`,
      'X-GitHub-Api-Version': API_VERSION
    }
  })
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(`GitHub request failed ${res.status} ${res.statusText}: ${body.slice(0, 300)}`)
  }
  return res.json()
}

export async function fetchReleases(repo, token, fetchImpl = fetch) {
  if (!repo) {
    throw new Error('repo is required')
  }
  if (!token) {
    throw new Error('token is required')
  }

  const releases = []
  for (let page = 1; ; page += 1) {
    const pageReleases = await githubJson(
      fetchImpl,
      `https://api.github.com/repos/${repo}/releases?per_page=100&page=${page}`,
      token
    )
    if (!Array.isArray(pageReleases)) {
      throw new Error(`GitHub releases response page ${page} for ${repo} was not an array`)
    }

    releases.push(...pageReleases)
    if (pageReleases.length < 100) {
      break
    }
  }

  return releases
}

async function main() {
  const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN
  const repo = resolveReleaseRepository(process.env)
  const releases = await fetchReleases(repo, token)
  process.stdout.write(latestStableDesktopReleaseTag(releases))
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  })
}
