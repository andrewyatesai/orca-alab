import { beforeEach, describe, expect, it, vi } from 'vitest'

const gitExecFileAsyncMock = vi.hoisted(() => vi.fn())

vi.mock('./runner', () => ({
  gitExecFileAsync: gitExecFileAsyncMock
}))

import {
  hasCommitObjectViaGitExec,
  hasLocalCommitObject,
  isFullGitObjectId
} from './commit-object-ref'

describe('commit object refs', () => {
  beforeEach(() => {
    gitExecFileAsyncMock.mockReset()
  })

  it('recognizes only complete git object IDs', () => {
    expect(isFullGitObjectId('a'.repeat(40))).toBe(true)
    expect(isFullGitObjectId('A'.repeat(40))).toBe(true)
    expect(isFullGitObjectId('abc123')).toBe(false)
    expect(isFullGitObjectId('origin/main')).toBe(false)
    expect(isFullGitObjectId('g'.repeat(40))).toBe(false)
  })

  it('recognizes SHA-256 (64-hex) object IDs so a present commit is not read as absent', () => {
    // Why: SHA-256 object-format repos (git 2.29+) use 64-hex commit OIDs; the
    // old 40-only gate short-circuited hasCommitObjectViaGitExec to false and
    // fired a redundant remote fetch for a base commit that was already local.
    expect(isFullGitObjectId('a'.repeat(64))).toBe(true)
    expect(isFullGitObjectId('A'.repeat(64))).toBe(true)
    // Neither SHA-1 nor SHA-256 length: still rejected without shelling out.
    expect(isFullGitObjectId('a'.repeat(50))).toBe(false)
    expect(isFullGitObjectId('g'.repeat(64))).toBe(false)
  })

  it('runs rev-parse (rather than short-circuiting) for a 64-hex SHA-256 ref', async () => {
    const gitExec = vi.fn().mockResolvedValue({ stdout: 'a'.repeat(64), stderr: '' })

    await expect(hasCommitObjectViaGitExec(gitExec, 'a'.repeat(64))).resolves.toBe(true)

    expect(gitExec).toHaveBeenCalledWith([
      'rev-parse',
      '--verify',
      '--quiet',
      `${'a'.repeat(64)}^{commit}`
    ])
  })

  it('verifies full commit objects and rejects missing objects', async () => {
    const gitExec = vi.fn().mockResolvedValue({ stdout: 'a'.repeat(40), stderr: '' })

    await expect(hasCommitObjectViaGitExec(gitExec, 'a'.repeat(40))).resolves.toBe(true)

    expect(gitExec).toHaveBeenCalledWith([
      'rev-parse',
      '--verify',
      '--quiet',
      `${'a'.repeat(40)}^{commit}`
    ])

    gitExec.mockRejectedValueOnce(new Error('missing'))
    await expect(hasCommitObjectViaGitExec(gitExec, 'b'.repeat(40))).resolves.toBe(false)
  })

  it('does not shell out for branch names or short SHAs', async () => {
    const gitExec = vi.fn()

    await expect(hasCommitObjectViaGitExec(gitExec, 'abc123')).resolves.toBe(false)
    await expect(hasCommitObjectViaGitExec(gitExec, 'origin/main')).resolves.toBe(false)

    expect(gitExec).not.toHaveBeenCalled()
  })

  it('checks local commit objects in the target repo path', async () => {
    gitExecFileAsyncMock.mockResolvedValue({ stdout: 'a'.repeat(40), stderr: '' })

    await expect(hasLocalCommitObject('/repo', 'a'.repeat(40))).resolves.toBe(true)

    expect(gitExecFileAsyncMock).toHaveBeenCalledWith(
      ['rev-parse', '--verify', '--quiet', `${'a'.repeat(40)}^{commit}`],
      { cwd: '/repo' }
    )
  })
})
