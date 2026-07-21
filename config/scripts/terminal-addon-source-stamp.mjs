import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'

const STAMP_SCHEMA = 1
const FULL_GIT_OBJECT_ID = /^[0-9a-f]{40,64}$/

/**
 * Return the exact aterm commit only when its checkout is clean. A dirty
 * checkout is deliberately uncacheable: recording HEAD alone would let later
 * source edits reuse an addon built from different bytes.
 */
export function readCleanAtermSourceCommit(atermSource, { spawnSyncImpl = spawnSync } = {}) {
  const revision = spawnSyncImpl('git', ['-C', atermSource, 'rev-parse', '--verify', 'HEAD'], {
    encoding: 'utf8'
  })
  const sourceCommit = revision.status === 0 ? revision.stdout?.trim() : null
  if (!sourceCommit || !FULL_GIT_OBJECT_ID.test(sourceCommit)) {
    return null
  }

  const status = spawnSyncImpl(
    'git',
    [
      '-C',
      atermSource,
      'status',
      '--porcelain=v1',
      '--untracked-files=normal',
      '--ignore-submodules=none'
    ],
    { encoding: 'utf8' }
  )
  if (status.status !== 0 || status.stdout?.trim()) {
    return null
  }
  return sourceCommit
}

export function readInstalledAtermSourceCommit(stampPath) {
  if (!existsSync(stampPath)) {
    return null
  }
  try {
    const stamp = JSON.parse(readFileSync(stampPath, 'utf8'))
    return stamp.schema === STAMP_SCHEMA &&
      typeof stamp.sourceCommit === 'string' &&
      FULL_GIT_OBJECT_ID.test(stamp.sourceCommit)
      ? stamp.sourceCommit
      : null
  } catch {
    return null
  }
}

export function installedAtermSourceIsCurrent(atermSource, stampPath) {
  const sourceCommit = readCleanAtermSourceCommit(atermSource)
  return sourceCommit !== null && readInstalledAtermSourceCommit(stampPath) === sourceCommit
}

export function writeInstalledAtermSourceCommit(stampPath, sourceCommit) {
  if (!FULL_GIT_OBJECT_ID.test(sourceCommit)) {
    throw new Error(`invalid aterm source commit for addon stamp: ${sourceCommit}`)
  }
  mkdirSync(path.dirname(stampPath), { recursive: true })
  writeFileSync(
    stampPath,
    `${JSON.stringify({ schema: STAMP_SCHEMA, sourceCommit }, null, 2)}\n`,
    'utf8'
  )
}

export function clearInstalledAtermSourceCommit(stampPath) {
  rmSync(stampPath, { force: true })
}
