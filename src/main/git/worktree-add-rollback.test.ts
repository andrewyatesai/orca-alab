import type * as FsPromises from 'node:fs/promises'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { gitExecFileAsyncMock, translateWslOutputPathsMock } = vi.hoisted(() => ({
  gitExecFileAsyncMock: vi.fn(),
  translateWslOutputPathsMock: vi.fn((output: string) => output)
}))

const { statMock } = vi.hoisted(() => ({
  statMock: vi.fn()
}))

vi.mock('./runner', () => ({
  gitExecFileAsync: gitExecFileAsyncMock,
  translateWslOutputPaths: translateWslOutputPathsMock
}))

vi.mock('node:fs/promises', async (importOriginal) => {
  const actual = await importOriginal<typeof FsPromises>()
  return { ...actual, stat: statMock }
})

vi.mock('./status', () => ({
  resolveGitDir: vi.fn(),
  runWithGitReadCacheInvalidation: (operation: () => unknown) => operation()
}))

import { addWorktree } from './worktree'

const REPO = '/repo'
const WORKTREE = '/repo-feature'
const BRANCH = 'feature/test'
const enoent = () => Object.assign(new Error('ENOENT'), { code: 'ENOENT' })

const registeredList = `worktree ${REPO}\nHEAD abc123\nbranch refs/heads/main\n\nworktree ${WORKTREE}\nHEAD def456\nbranch refs/heads/${BRANCH}\n`
const unregisteredList = `worktree ${REPO}\nHEAD abc123\nbranch refs/heads/main\n`

function isWorktreeAdd(args: string[]): boolean {
  return args.includes('worktree') && args.includes('add')
}

function isWorktreeList(args: string[]): boolean {
  return args[0] === 'worktree' && args[1] === 'list'
}

function mockGitWithFailingAdd(addError: Error, listStdout: string): void {
  gitExecFileAsyncMock.mockImplementation(async (args: string[]) => {
    if (isWorktreeAdd(args)) {
      throw addError
    }
    if (isWorktreeList(args)) {
      return { stdout: listStdout }
    }
    return { stdout: '' }
  })
}

describe('addWorktree rollback when the primary `git worktree add` fails', () => {
  beforeEach(() => {
    gitExecFileAsyncMock.mockReset()
    translateWslOutputPathsMock.mockClear()
    // Why: no git-crypt state — these tests cover the plain (non-deferred-checkout) add path.
    statMock.mockReset().mockRejectedValue(enoent())
  })

  it('removes the registered worktree and force-deletes the fresh branch after a killed add', async () => {
    const addError = new Error('failed to run git: timed out after 180000ms')
    mockGitWithFailingAdd(addError, registeredList)

    await expect(addWorktree(REPO, WORKTREE, BRANCH)).rejects.toBe(addError)

    const calls = gitExecFileAsyncMock.mock.calls.map((call) => call[0])
    expect(calls).toContainEqual(['worktree', 'remove', '--force', WORKTREE])
    expect(calls).toContainEqual(['branch', '-D', '--', BRANCH])
  })

  it('never deletes a pre-existing branch when a checkout-existing-branch add fails', async () => {
    const addError = new Error('failed to run git: timed out after 180000ms')
    mockGitWithFailingAdd(addError, registeredList)

    await expect(
      addWorktree(REPO, WORKTREE, BRANCH, undefined, false, false, {
        checkoutExistingBranch: true
      })
    ).rejects.toBe(addError)

    const calls = gitExecFileAsyncMock.mock.calls.map((call) => call[0])
    expect(calls).toContainEqual(['worktree', 'remove', '--force', WORKTREE])
    expect(calls.some((args) => args[0] === 'branch')).toBe(false)
  })

  it('rethrows unchanged without rollback when the add failed before registering the worktree', async () => {
    const addError = new Error(`fatal: a branch named '${BRANCH}' already exists`)
    mockGitWithFailingAdd(addError, unregisteredList)

    await expect(addWorktree(REPO, WORKTREE, BRANCH)).rejects.toBe(addError)

    expect(addError.message).not.toContain('cleanup also failed')
    const calls = gitExecFileAsyncMock.mock.calls.map((call) => call[0])
    expect(calls.some((args) => args[0] === 'worktree' && args[1] === 'remove')).toBe(false)
    expect(calls.some((args) => args[0] === 'branch')).toBe(false)
  })
})
