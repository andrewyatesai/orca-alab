import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import {
  clearInstalledAtermSourceCommit,
  installedAtermSourceIsCurrent,
  readCleanAtermSourceCommit,
  readInstalledAtermSourceCommit,
  writeInstalledAtermSourceCommit
} from './terminal-addon-source-stamp.mjs'

const temporaryRoots = []

afterEach(() => {
  for (const root of temporaryRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true })
  }
})

function git(repo, ...args) {
  return execFileSync('git', ['-C', repo, ...args], { encoding: 'utf8' }).trim()
}

function makeAtermRepository() {
  const repo = mkdtempSync(path.join(tmpdir(), 'orca-terminal-addon-stamp-'))
  temporaryRoots.push(repo)
  git(repo, 'init', '--quiet')
  git(repo, 'config', 'user.email', 'tests@example.invalid')
  git(repo, 'config', 'user.name', 'Orca Tests')
  writeFileSync(path.join(repo, 'engine.rs'), 'pub const ENGINE: u8 = 1;\n')
  git(repo, 'add', 'engine.rs')
  git(repo, 'commit', '--quiet', '-m', 'initial engine')
  return repo
}

describe('terminal addon aterm source stamp', () => {
  it('reuses an addon only for the exact clean aterm commit', () => {
    const repo = makeAtermRepository()
    const stamp = path.join(repo, '.git', 'installed-aterm-source.json')
    const firstCommit = readCleanAtermSourceCommit(repo)
    expect(firstCommit).toMatch(/^[0-9a-f]{40}$/)

    writeInstalledAtermSourceCommit(stamp, firstCommit)
    expect(installedAtermSourceIsCurrent(repo, stamp)).toBe(true)

    writeFileSync(path.join(repo, 'engine.rs'), 'pub const ENGINE: u8 = 2;\n')
    git(repo, 'add', 'engine.rs')
    git(repo, 'commit', '--quiet', '-m', 'new engine')

    expect(readCleanAtermSourceCommit(repo)).not.toBe(firstCommit)
    expect(installedAtermSourceIsCurrent(repo, stamp)).toBe(false)
  })

  it('treats dirty source as uncacheable even when HEAD matches the stamp', () => {
    const repo = makeAtermRepository()
    const stamp = path.join(repo, '.git', 'installed-aterm-source.json')
    const sourceCommit = readCleanAtermSourceCommit(repo)
    writeInstalledAtermSourceCommit(stamp, sourceCommit)

    writeFileSync(path.join(repo, 'engine.rs'), 'pub const ENGINE: u8 = 9;\n')

    expect(readCleanAtermSourceCommit(repo)).toBeNull()
    expect(installedAtermSourceIsCurrent(repo, stamp)).toBe(false)
  })

  it('fails closed for missing or malformed stamps and can clear an old stamp', () => {
    const repo = makeAtermRepository()
    const stamp = path.join(repo, '.git', 'installed-aterm-source.json')
    const sourceCommit = readCleanAtermSourceCommit(repo)

    expect(installedAtermSourceIsCurrent(repo, stamp)).toBe(false)
    writeInstalledAtermSourceCommit(stamp, sourceCommit)
    writeFileSync(stamp, '{"schema":1,"sourceCommit":"not-a-commit"}\n')
    expect(readInstalledAtermSourceCommit(stamp)).toBeNull()

    writeInstalledAtermSourceCommit(stamp, sourceCommit)
    expect(JSON.parse(readFileSync(stamp, 'utf8'))).toEqual({
      schema: 1,
      sourceCommit
    })
    clearInstalledAtermSourceCommit(stamp)
    expect(readInstalledAtermSourceCommit(stamp)).toBeNull()
  })
})
