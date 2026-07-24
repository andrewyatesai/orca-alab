/**
 * Regression: discarding an untracked folder that contains a nested/embedded git
 * repo must NOT wipe that inner repo (its unpushed commits + uncommitted work).
 * `git status --untracked-files=all` reports a nested clone as a single `?? dir/`
 * entry, so discard classified it as untracked and routed to `git clean`. With
 * `-ff` git recurses into and deletes the nested repo; with `-f` it fail-safe
 * skips it. Drives the real function against a real git repo (runner unmocked).
 */
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { bulkDiscardChanges, discardChanges } from './status'

function git(cwd: string, ...args: string[]): string {
  return execFileSync('git', args, { cwd, encoding: 'utf8' })
}

function initRepo(dir: string): void {
  git(dir, 'init', '-q')
  git(dir, 'config', 'user.email', 'test@example.com')
  git(dir, 'config', 'user.name', 'Test')
  git(dir, 'config', 'commit.gpgsign', 'false')
}

describe('discard preserves a nested git repo', () => {
  let repo: string

  beforeEach(() => {
    repo = mkdtempSync(path.join(tmpdir(), 'orca-discard-nested-'))
    initRepo(repo)
    writeFileSync(path.join(repo, 'seed.txt'), 'seed\n')
    git(repo, 'add', 'seed.txt')
    git(repo, 'commit', '-qm', 'init')
  })

  afterEach(() => {
    rmSync(repo, { recursive: true, force: true })
  })

  function makeNestedRepo(folder: string): { inner: string; innerGit: string } {
    const inner = path.join(repo, folder)
    mkdirSync(inner)
    initRepo(inner)
    // An unpushed commit + an uncommitted working change: both irrecoverable if wiped.
    writeFileSync(path.join(inner, 'committed.txt'), 'unpushed work\n')
    git(inner, 'add', 'committed.txt')
    git(inner, 'commit', '-qm', 'local-only commit')
    writeFileSync(path.join(inner, 'dirty.txt'), 'uncommitted\n')
    // Outer repo sees the nested clone as a single untracked entry.
    expect(git(repo, 'status', '--porcelain').trim()).toBe(`?? ${folder}/`)
    return { inner, innerGit: path.join(inner, '.git') }
  }

  it('does not delete the nested repo when discarding the folder', async () => {
    const { inner, innerGit } = makeNestedRepo('helper-repo')

    await discardChanges(repo, 'helper-repo')

    expect(existsSync(innerGit)).toBe(true)
    expect(existsSync(path.join(inner, 'committed.txt'))).toBe(true)
    expect(existsSync(path.join(inner, 'dirty.txt'))).toBe(true)
    // The inner repo's unpushed commit is still reachable.
    expect(git(inner, 'log', '--oneline').trim()).toContain('local-only commit')
  })

  it('does not delete the nested repo through the bulk discard path', async () => {
    const { inner, innerGit } = makeNestedRepo('vendor-repo')

    await bulkDiscardChanges(repo, ['vendor-repo'])

    expect(existsSync(innerGit)).toBe(true)
    expect(git(inner, 'log', '--oneline').trim()).toContain('local-only commit')
  })
})
