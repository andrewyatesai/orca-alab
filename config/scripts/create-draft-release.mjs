#!/usr/bin/env node

import { pathToFileURL } from 'node:url'
import { parseDesktopReleaseTag } from './desktop-release-tag.mjs'
import { resolveReleaseNotesRepository, resolveReleaseRepository } from './release-repository.mjs'

const API_VERSION = '2022-11-28'
const MAX_RELEASE_BODY_LENGTH = 120_000
const TRUNCATION_NOTICE =
  '\n\n---\nRelease notes were truncated because GitHub release bodies are limited to 125,000 characters.'
export { parseDesktopReleaseTag } from './desktop-release-tag.mjs'

function compareDesktopReleaseTags(a, b) {
  const versionDiff = a.major - b.major || a.minor - b.minor || a.patch - b.patch
  if (versionDiff !== 0) {
    return versionDiff
  }
  const aPrerelease = a.rc ?? a.fork
  const bPrerelease = b.rc ?? b.fork
  if (aPrerelease === bPrerelease && a.rc === b.rc && a.fork === b.fork) {
    return 0
  }
  if (aPrerelease === null) {
    return 1
  }
  if (bPrerelease === null) {
    return -1
  }
  if ((a.fork === null) !== (b.fork === null)) {
    return a.fork === null ? 1 : -1
  }
  return aPrerelease - bPrerelease
}

export function latestPreviousPublishedDesktopReleaseTag(releases, tag) {
  const current = parseDesktopReleaseTag(tag)
  if (!current) {
    return ''
  }
  const previousReleases = releases
    .filter((release) => release?.draft === false && typeof release.tag_name === 'string')
    .map((release) => parseDesktopReleaseTag(release.tag_name))
    .filter((candidate) => candidate && candidate.tag !== current.tag)
    .filter((candidate) => compareDesktopReleaseTags(candidate, current) < 0)
    // Why: ALab fork notes follow the fork train; legacy stable/RC notes retain their existing channel.
    .filter((candidate) =>
      current.fork !== null
        ? candidate.fork !== null
        : current.rc !== null
          ? candidate.fork === null
          : candidate.rc === null && candidate.fork === null
    )
    .sort(compareDesktopReleaseTags)
  return previousReleases.at(-1)?.tag ?? ''
}

function githubHeaders(token) {
  return {
    Accept: 'application/vnd.github+json',
    Authorization: `Bearer ${token}`,
    'X-GitHub-Api-Version': API_VERSION
  }
}

async function githubJson(fetchImpl, url, token, options = {}) {
  const res = await fetchImpl(url, {
    ...options,
    headers: {
      ...githubHeaders(token),
      ...options.headers
    }
  })
  if (!res.ok) {
    const body = await res.text().catch(() => '')
    throw new Error(`GitHub request failed ${res.status} ${res.statusText}: ${body.slice(0, 300)}`)
  }
  return res.json()
}

async function fetchRepoReleases(repo, token, fetchImpl) {
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

async function assertExactTagRef(repo, tag, token, fetchImpl) {
  const ref = await githubJson(
    fetchImpl,
    `https://api.github.com/repos/${repo}/git/ref/tags/${encodeURIComponent(tag)}`,
    token
  )
  if (ref?.ref !== `refs/tags/${tag}`) {
    throw new Error(`Repository ${repo} did not return the exact tag ref refs/tags/${tag}`)
  }
}

export function truncateReleaseBody(body, maxLength = MAX_RELEASE_BODY_LENGTH) {
  if (body.length <= maxLength) {
    return body
  }

  const availableLength = maxLength - TRUNCATION_NOTICE.length
  if (availableLength <= 0) {
    throw new Error('Release truncation notice is longer than the maximum release body length')
  }

  return `${body.slice(0, availableLength).trimEnd()}${TRUNCATION_NOTICE}`
}

export async function createDraftRelease({
  repo,
  notesRepo,
  tag,
  token,
  fetchImpl = fetch,
  log = console.log
}) {
  if (!repo) {
    throw new Error('repo is required')
  }
  if (!notesRepo) {
    throw new Error('notesRepo is required')
  }
  if (!tag) {
    throw new Error('tag is required')
  }
  if (!token) {
    throw new Error('token is required')
  }
  if (!parseDesktopReleaseTag(tag)) {
    throw new Error(`Invalid desktop release tag: ${tag}`)
  }

  // Why: creating a release for a missing tag makes GitHub synthesize it at the default branch.
  await Promise.all([
    assertExactTagRef(notesRepo, tag, token, fetchImpl),
    assertExactTagRef(repo, tag, token, fetchImpl)
  ])

  const previousTag = latestPreviousPublishedDesktopReleaseTag(
    await fetchRepoReleases(repo, token, fetchImpl),
    tag
  )
  const generateNotesBody = {
    tag_name: tag,
    target_commitish: tag,
    ...(previousTag ? { previous_tag_name: previousTag } : {})
  }

  // Why: GitHub's generate-notes baseline ignores draft releases, so pass the
  // previous public changelog boundary explicitly.
  const releaseNotes = await githubJson(
    fetchImpl,
    `https://api.github.com/repos/${notesRepo}/releases/generate-notes`,
    token,
    {
      method: 'POST',
      body: JSON.stringify(generateNotesBody)
    }
  )

  const generatedBody = typeof releaseNotes.body === 'string' ? releaseNotes.body : ''
  const body = truncateReleaseBody(generatedBody)
  const name =
    typeof releaseNotes.name === 'string' && releaseNotes.name.length > 0 ? releaseNotes.name : tag
  const parsedTag = parseDesktopReleaseTag(tag)
  const prerelease = parsedTag.rc !== null

  // Why: GitHub's generated release notes can exceed the release body API
  // limit, so create with a bounded body. Omit target_commitish because the
  // release-cut tag already exists and GitHub rejects the tag name there.
  await githubJson(fetchImpl, `https://api.github.com/repos/${repo}/releases`, token, {
    method: 'POST',
    body: JSON.stringify({
      tag_name: tag,
      name,
      body,
      draft: true,
      prerelease
    })
  })

  if (generatedBody.length !== body.length) {
    log(`Created draft release ${tag} with truncated generated notes (${body.length} chars).`)
  } else {
    log(`Created draft release ${tag} with generated notes (${body.length} chars).`)
  }
}

async function main() {
  const tag = process.argv[2]
  const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN
  const repo = resolveReleaseRepository(process.env)
  const notesRepo = resolveReleaseNotesRepository(process.env)
  await createDraftRelease({ repo, notesRepo, tag, token })
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message)
    process.exit(1)
  })
}
