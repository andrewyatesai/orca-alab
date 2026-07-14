import * as path from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { lstatMock } = vi.hoisted(() => ({ lstatMock: vi.fn() }))
vi.mock('fs/promises', () => ({ lstat: lstatMock, readFile: vi.fn() }))

import type { GitExec } from './git-handler-ops'
import { collectUntrackedAdditionsViaGitNumstat } from './git-numstat-untracked-counter'
import { MAX_UNTRACKED_LINE_COUNT_FILES } from '../shared/git-uncommitted-line-stats'

function regularFileStat(size = 5, mtimeMs = 1) {
  return { size, mtimeMs, ctimeMs: mtimeMs, isFile: () => true, isSymbolicLink: () => false }
}

describe('collectUntrackedAdditionsViaGitNumstat', () => {
  beforeEach(() => {
    lstatMock.mockReset()
  })

  it('skips counting above the file-count cap without any git spawn or lstat', async () => {
    // Why: the middle-band (501-9,999 untracked files) generated/dependency dir
    // must not lstat + `git diff --no-index` every file each poll on the SSH host;
    // over the cap we skip entirely (rows render without a +N), matching local.
    const git = vi.fn<GitExec>()
    const paths = Array.from(
      { length: MAX_UNTRACKED_LINE_COUNT_FILES + 1 },
      (_, i) => `over-cap/file-${i}.ts`
    )

    const stats = await collectUntrackedAdditionsViaGitNumstat(git, '/repo', paths)

    expect(stats.size).toBe(0)
    expect(git).not.toHaveBeenCalled()
    expect(lstatMock).not.toHaveBeenCalled()
  })

  it('counts each untracked file via git at or below the cap', async () => {
    lstatMock.mockResolvedValue(regularFileStat())
    const git = vi.fn<GitExec>(async () => ({ stdout: '3\t0\tunder-cap/new.ts\n', stderr: '' }))

    const stats = await collectUntrackedAdditionsViaGitNumstat(git, '/repo', ['under-cap/new.ts'])

    expect(stats.get('under-cap/new.ts')).toEqual({ added: 3 })
    expect(git).toHaveBeenCalledTimes(1)
    expect(git.mock.calls[0][0]).toEqual([
      'diff',
      '--no-index',
      '--numstat',
      '--',
      '/dev/null',
      path.join('/repo', 'under-cap/new.ts')
    ])
  })
})
