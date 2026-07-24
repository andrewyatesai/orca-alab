import { execFile } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { GitCapabilityCache } from '../shared/git-capability-cache'
import type { GitExec } from './git-handler-ops'
import { refreshLocalBaseRefForWorktreeCreateOp } from './git-handler-local-base-ref-refresh'

const execFileAsync = promisify(execFile)

// Why: exercise the real `git reset --keep` fail-closed behavior against a real
// binary — a mock cannot prove that a racing tracked edit survives instead of
// being destroyed the way `--hard` would.
function realGit(): GitExec {
  return async (args, cwd) => {
    const { stdout, stderr } = await execFileAsync('git', args, { cwd, encoding: 'utf8' })
    return { stdout, stderr }
  }
}

async function run(cwd: string, ...args: string[]): Promise<void> {
  await execFileAsync('git', args, { cwd, encoding: 'utf8' })
}

async function headOid(cwd: string): Promise<string> {
  const { stdout } = await execFileAsync('git', ['rev-parse', 'HEAD'], { cwd, encoding: 'utf8' })
  return stdout.trim()
}

describe('refreshLocalBaseRefForWorktreeCreateOp (real git)', () => {
  let repo: string

  beforeEach(async () => {
    repo = await mkdtemp(join(tmpdir(), 'orc-lbr-'))
    await run(repo, 'init')
    await run(repo, 'config', 'user.email', 'test@example.com')
    await run(repo, 'config', 'user.name', 'Test')
    await run(repo, 'config', 'commit.gpgsign', 'false')
    await writeFile(join(repo, 'shared.txt'), 'v1\n')
    await run(repo, 'add', 'shared.txt')
    await run(repo, 'commit', '-m', 'c1')
    await run(repo, 'branch', '-M', 'main')
    const c1 = await headOid(repo)
    // Why: build a fast-forward remote-tracking tip (C2) that changes shared.txt.
    await writeFile(join(repo, 'shared.txt'), 'v2\n')
    await run(repo, 'commit', '-am', 'c2')
    const c2 = await headOid(repo)
    await run(repo, 'update-ref', 'refs/remotes/origin/main', c2)
    // Why: move the owner worktree's main back to C1 so remote is one FF ahead.
    await run(repo, 'reset', '--hard', c1)
  })

  afterEach(async () => {
    await rm(repo, { recursive: true, force: true })
  })

  const params = () => ({
    repoPath: repo,
    fullRef: 'refs/heads/main',
    remoteTrackingRef: 'refs/remotes/origin/main'
  })

  it('fast-forwards the clean owner worktree to the remote tip', async () => {
    await refreshLocalBaseRefForWorktreeCreateOp(realGit(), params(), new GitCapabilityCache())

    expect(await readFile(join(repo, 'shared.txt'), 'utf8')).toBe('v2\n')
  })

  it('aborts (fail-closed) and preserves a tracked edit that races the clean-check', async () => {
    // Why: simulate the TOCTOU — the clean-check passes, then a tracked edit to
    // a file that differs between HEAD and target lands just before the reset.
    let injected = false
    const racingGit: GitExec = async (args, cwd, opts) => {
      if (args[0] === 'reset' && !injected) {
        injected = true
        await writeFile(join(repo, 'shared.txt'), 'RACING LOCAL EDIT\n')
      }
      return realGit()(args, cwd, opts)
    }

    await expect(
      refreshLocalBaseRefForWorktreeCreateOp(racingGit, params(), new GitCapabilityCache())
    ).rejects.toThrow('Local base ref worktree has tracked changes.')

    // The racing edit must survive — `--hard` would have destroyed it into 'v2\n'.
    expect(await readFile(join(repo, 'shared.txt'), 'utf8')).toBe('RACING LOCAL EDIT\n')
  })
})
