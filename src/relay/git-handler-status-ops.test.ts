import { mkdtempSync } from 'node:fs'
import * as fs from 'node:fs/promises'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GitExec } from './git-handler-ops'
import { collectNumstatPathspecs, getStatusOp } from './git-handler-status-ops'
import { clearNoEffectiveUpstreamStatusCache } from './git-status-upstream-negative-cache'
import type { GitStatusEntry } from '../shared/types'

const LARGE_STATUS_ENTRY_COUNT = 150_000

function buildLargeStatusOutput(count: number): string {
  const lines: string[] = []
  for (let index = 0; index < count; index += 1) {
    lines.push(`1 A. N... 100644 100644 100644 000000 111111 generated-${index}.txt`)
  }
  return lines.join('\n')
}

function buildBranchStatusOutput(head: string, branch: string): string {
  return [`# branch.oid ${head}`, `# branch.head ${branch}`].join('\n')
}

describe('getStatusOp', () => {
  let tmpDir: string

  beforeEach(() => {
    clearNoEffectiveUpstreamStatusCache()
    tmpDir = mkdtempSync(path.join(tmpdir(), 'relay-git-status-'))
  })

  afterEach(async () => {
    vi.useRealTimers()
    clearNoEffectiveUpstreamStatusCache()
    await fs.rm(tmpDir, { recursive: true, force: true })
  })

  it('truncates huge status lists at the limit and flags didHitLimit', async () => {
    const statusOutput = buildLargeStatusOutput(LARGE_STATUS_ENTRY_COUNT)
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: statusOutput, stderr: '' }
      }
      if (args.includes('diff')) {
        return { stdout: '', stderr: '' }
      }
      throw new Error(`Unexpected git command: ${args.join(' ')}`)
    })

    const result = await getStatusOp(git, { worktreePath: tmpDir, limit: 10_000 })

    expect(result.didHitLimit).toBe(true)
    expect(result.statusLength).toBe(LARGE_STATUS_ENTRY_COUNT)
    expect(result.entries).toHaveLength(10_000)
    expect(result.entries[0]).toEqual({
      path: 'generated-0.txt',
      status: 'added',
      area: 'staged'
    })
    // numstat (diff) must be skipped when the limit was hit.
    expect(git.mock.calls.some(([args]) => args.includes('diff'))).toBe(false)
  })

  it('returns the full list and no limit flag when under the limit', async () => {
    const statusOutput = buildLargeStatusOutput(5)
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: statusOutput, stderr: '' }
      }
      if (args.includes('diff')) {
        return { stdout: '', stderr: '' }
      }
      throw new Error(`Unexpected git command: ${args.join(' ')}`)
    })

    const result = await getStatusOp(git, { worktreePath: tmpDir, limit: 10_000 })

    expect(result.didHitLimit).toBeUndefined()
    expect(result.entries).toHaveLength(5)
  })

  it('scopes staged and unstaged numstat scans to each area’s changed paths', async () => {
    const numstatCalls: string[][] = []
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return {
          stdout:
            '1 M. N... 100644 100644 100644 aaaa aaaa src/staged.ts\n' +
            '1 .M N... 100644 100644 100644 bbbb bbbb src/unstaged.ts\n',
          stderr: ''
        }
      }
      if (args.includes('--numstat')) {
        numstatCalls.push(args)
        return {
          stdout: args.includes('--cached') ? '10\t0\tsrc/staged.ts\n' : '3\t4\tsrc/unstaged.ts\n',
          stderr: ''
        }
      }
      throw new Error(`Unexpected git command: ${args.join(' ')}`)
    })

    const result = await getStatusOp(git, { worktreePath: tmpDir })

    // Each area scans only its own path — no full-worktree rescan over SSH.
    expect(numstatCalls).toContainEqual([
      '-c',
      'core.quotePath=false',
      'diff',
      '--cached',
      '--numstat',
      '-M',
      '--',
      ':(literal)src/staged.ts'
    ])
    expect(numstatCalls).toContainEqual([
      '-c',
      'core.quotePath=false',
      'diff',
      '--numstat',
      '-M',
      '--',
      ':(literal)src/unstaged.ts'
    ])
    expect(result.entries).toEqual([
      { path: 'src/staged.ts', status: 'modified', area: 'staged', added: 10, removed: 0 },
      { path: 'src/unstaged.ts', status: 'modified', area: 'unstaged', added: 3, removed: 4 }
    ])
  })

  it('scopes the staged rename scan to both the old and new paths', async () => {
    const numstatCalls: string[][] = []
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return {
          stdout: '2 R. N... 100644 100644 100644 aaaa bbbb R100 src/new name.ts\tsrc/old name.ts\n',
          stderr: ''
        }
      }
      if (args.includes('--numstat')) {
        numstatCalls.push(args)
        return { stdout: '2\t1\tsrc/old name.ts => src/new name.ts\n', stderr: '' }
      }
      throw new Error(`Unexpected git command: ${args.join(' ')}`)
    })

    const result = await getStatusOp(git, { worktreePath: tmpDir })

    // Why: -M rename detection needs BOTH sides in the pathspec, or the new path
    // is mis-reported as a plain add — so the staged scan scopes to old + new.
    expect(numstatCalls).toContainEqual([
      '-c',
      'core.quotePath=false',
      'diff',
      '--cached',
      '--numstat',
      '-M',
      '--',
      ':(literal)src/new name.ts',
      ':(literal)src/old name.ts'
    ])
    expect(result.entries).toEqual([
      {
        path: 'src/new name.ts',
        oldPath: 'src/old name.ts',
        status: 'renamed',
        area: 'staged',
        added: 2,
        removed: 1
      }
    ])
  })

  it('runs no numstat diff for a clean working tree', async () => {
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: '', stderr: '' }
      }
      throw new Error(`Unexpected git command: ${args.join(' ')}`)
    })

    await getStatusOp(git, { worktreePath: tmpDir })

    // Only the single status read — attachLineStats short-circuits on no entries.
    expect(git).toHaveBeenCalledTimes(1)
    expect(git.mock.calls.some(([args]) => args.includes('diff'))).toBe(false)
  })

  it('caches no-effective-upstream probes across status polls for the same head', async () => {
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: buildBranchStatusOutput('abc123', 'feature'), stderr: '' }
      }
      if (args[0] === 'symbolic-ref') {
        return { stdout: 'feature\n', stderr: '' }
      }
      if (args[0] === 'rev-parse' && args.includes('HEAD@{u}')) {
        throw new Error('fatal: no upstream configured for branch feature')
      }
      throw new Error(`No upstream fixture for git ${args.join(' ')}`)
    })

    const first = await getStatusOp(git, { worktreePath: tmpDir })
    const firstCallCount = git.mock.calls.length
    const second = await getStatusOp(git, { worktreePath: tmpDir })

    expect(first.upstreamStatus).toEqual({ hasUpstream: false, ahead: 0, behind: 0 })
    expect(second.upstreamStatus).toEqual(first.upstreamStatus)
    expect(git.mock.calls).toHaveLength(firstCallCount + 1)
    expect(
      git.mock.calls.filter(([args]) => args[0] === 'rev-parse' && args.includes('HEAD@{u}'))
    ).toHaveLength(1)
    expect(
      git.mock.calls.filter(
        ([args]) => args[0] === 'rev-parse' && args.includes('refs/remotes/origin/feature')
      )
    ).toHaveLength(1)
  })

  it('keeps no-effective-upstream probes cached beyond thirty seconds', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(0)
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: buildBranchStatusOutput('abc123', 'feature'), stderr: '' }
      }
      if (args[0] === 'symbolic-ref') {
        return { stdout: 'feature\n', stderr: '' }
      }
      if (args[0] === 'rev-parse' && args.includes('HEAD@{u}')) {
        throw new Error('fatal: no upstream configured for branch feature')
      }
      if (args[0] === 'rev-parse' && args.includes('refs/remotes/origin/feature')) {
        throw new Error('missing remote branch')
      }
      throw new Error(`No upstream fixture for git ${args.join(' ')}`)
    })

    await getStatusOp(git, { worktreePath: tmpDir })
    vi.setSystemTime(31_000)
    await getStatusOp(git, { worktreePath: tmpDir })

    expect(
      git.mock.calls.filter(([args]) => args[0] === 'rev-parse' && args.includes('HEAD@{u}'))
    ).toHaveLength(1)
  })

  it('coalesces concurrent no-effective-upstream probes', async () => {
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: buildBranchStatusOutput('abc123', 'feature'), stderr: '' }
      }
      if (args[0] === 'symbolic-ref') {
        return { stdout: 'feature\n', stderr: '' }
      }
      if (args[0] === 'rev-parse' && args.includes('HEAD@{u}')) {
        await Promise.resolve()
        throw new Error('fatal: no upstream configured for branch feature')
      }
      if (args[0] === 'rev-parse' && args.includes('refs/remotes/origin/feature')) {
        await Promise.resolve()
        throw new Error('missing remote branch')
      }
      throw new Error(`No upstream fixture for git ${args.join(' ')}`)
    })

    await Promise.all([
      getStatusOp(git, { worktreePath: tmpDir }),
      getStatusOp(git, { worktreePath: tmpDir }),
      getStatusOp(git, { worktreePath: tmpDir })
    ])

    expect(
      git.mock.calls.filter(([args]) => args[0] === 'rev-parse' && args.includes('HEAD@{u}'))
    ).toHaveLength(1)
    expect(
      git.mock.calls.filter(
        ([args]) => args[0] === 'rev-parse' && args.includes('refs/remotes/origin/feature')
      )
    ).toHaveLength(1)
  })

  it('invalidates cached no-effective-upstream probes when the branch changes', async () => {
    let branch = 'feature'
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: buildBranchStatusOutput('abc123', branch), stderr: '' }
      }
      if (args[0] === 'symbolic-ref') {
        return { stdout: `${branch}\n`, stderr: '' }
      }
      if (args[0] === 'rev-parse' && args.includes('HEAD@{u}')) {
        throw new Error(`fatal: no upstream configured for branch ${branch}`)
      }
      if (args[0] === 'rev-parse' && args.some((arg) => arg.startsWith('refs/remotes/origin/'))) {
        throw new Error('missing remote branch')
      }
      throw new Error(`No upstream fixture for git ${args.join(' ')}`)
    })

    await getStatusOp(git, { worktreePath: tmpDir })
    branch = 'other-feature'
    await getStatusOp(git, { worktreePath: tmpDir })

    expect(
      git.mock.calls
        .filter(
          ([args]) =>
            args[0] === 'rev-parse' && args.some((arg) => arg.startsWith('refs/remotes/origin/'))
        )
        .map(([args]) => args.at(-1))
    ).toEqual(['refs/remotes/origin/feature', 'refs/remotes/origin/other-feature'])
  })

  it('does not cache a configured push target signal', async () => {
    const git = vi.fn<GitExec>(async (args) => {
      if (args.includes('status')) {
        return { stdout: buildBranchStatusOutput('abc123', 'feature/fix'), stderr: '' }
      }
      if (args[0] === 'symbolic-ref') {
        return { stdout: 'feature/fix\n', stderr: '' }
      }
      if (args[0] === 'rev-parse' && args.includes('HEAD@{u}')) {
        throw new Error('fatal: no upstream configured for branch feature/fix')
      }
      if (args[0] === 'config' && args.includes('branch.feature/fix.pushRemote')) {
        return { stdout: 'fork\n', stderr: '' }
      }
      if (args[0] === 'config' && args.includes('remote.pushDefault')) {
        throw new Error('missing push default')
      }
      if (args[0] === 'config' && args.includes('branch.feature/fix.remote')) {
        return { stdout: 'fork\n', stderr: '' }
      }
      if (args[0] === 'config' && args.includes('branch.feature/fix.merge')) {
        return { stdout: 'refs/heads/feature/fix\n', stderr: '' }
      }
      if (args[0] === 'config' && args.includes('branch.feature/fix.base')) {
        throw new Error('missing branch base')
      }
      if (args[0] === 'rev-parse' && args.some((arg) => arg.startsWith('refs/remotes/'))) {
        throw new Error('missing remote branch')
      }
      throw new Error(`No upstream fixture for git ${args.join(' ')}`)
    })

    await getStatusOp(git, { worktreePath: tmpDir })
    await getStatusOp(git, { worktreePath: tmpDir })

    expect(
      git.mock.calls.filter(([args]) => args[0] === 'rev-parse' && args.includes('HEAD@{u}'))
    ).toHaveLength(2)
  })
})

describe('collectNumstatPathspecs', () => {
  it('scopes to a single area and excludes other areas', () => {
    const entries: GitStatusEntry[] = [
      { path: 'src/staged.ts', status: 'modified', area: 'staged' },
      { path: 'src/unstaged.ts', status: 'modified', area: 'unstaged' },
      { path: 'src/new.ts', status: 'added', area: 'untracked' }
    ]

    expect(collectNumstatPathspecs(entries, 'staged')).toEqual(['src/staged.ts'])
    expect(collectNumstatPathspecs(entries, 'unstaged')).toEqual(['src/unstaged.ts'])
  })

  it('includes both the old and new sides of a rename', () => {
    const entries: GitStatusEntry[] = [
      { path: 'src/new.ts', oldPath: 'src/old.ts', status: 'renamed', area: 'staged' }
    ]

    expect(collectNumstatPathspecs(entries, 'staged')).toEqual(['src/new.ts', 'src/old.ts'])
  })

  it('includes the copy source so a scoped scan matches the full scan', () => {
    const entries: GitStatusEntry[] = [
      { path: 'src/copy.ts', oldPath: 'src/source.ts', status: 'copied', area: 'staged' }
    ]

    expect(collectNumstatPathspecs(entries, 'staged')).toEqual(['src/copy.ts', 'src/source.ts'])
  })

  it('falls back to a full scan (null) when a rename is missing its old side', () => {
    // Why: without the old path, -M would mis-report the new path as a plain add,
    // so returning null tells runNumstat to run the unscoped scan for that area.
    const entries: GitStatusEntry[] = [
      { path: 'src/new.ts', status: 'renamed', area: 'staged' },
      { path: 'src/other.ts', status: 'modified', area: 'staged' }
    ]

    expect(collectNumstatPathspecs(entries, 'staged')).toBeNull()
  })

  it('returns an empty list for an area with no entries', () => {
    const entries: GitStatusEntry[] = [
      { path: 'src/unstaged.ts', status: 'modified', area: 'unstaged' }
    ]

    expect(collectNumstatPathspecs(entries, 'staged')).toEqual([])
  })
})
