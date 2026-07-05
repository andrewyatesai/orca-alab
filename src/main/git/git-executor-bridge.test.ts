import { execFile } from 'node:child_process'
import { describe, expect, it } from 'vitest'
import { loadRustGitBinding, type RustGitExecutor } from '../daemon/rust-git-addon'

// The IO-tier "A bridge": Rust drives orca-git's sync GitRunner logic
// (validate_git_push_target = shape check + `git check-ref-format`) over an async
// JS executor. These tests prove Rust DRIVES while JS EXECUTES: the executor is
// JS-supplied (where runner.ts's SSH/WSL routing would live), Rust decides when to
// call it and classifies the result. Skips cleanly when the .node is absent.

const binding = loadRustGitBinding()
const suite = binding ? describe : describe.skip
const git = binding!

/** A deterministic mock git executor: records calls and maps a few known
 *  `check-ref-format --branch <name>` inputs to exit codes, without spawning git. */
function mockExecutor(): { executor: RustGitExecutor; calls: string[][] } {
  const calls: string[][] = []
  const executor: RustGitExecutor = (args) => {
    calls.push(args)
    const branch = args[2] // ['check-ref-format', '--branch', <name>]
    // git check-ref-format rejects '..' and other malformed refs with code 128.
    const ok = typeof branch === 'string' && !branch.includes('..') && branch.length > 0
    return Promise.resolve({ stdout: '', stderr: ok ? '' : 'fatal: bad ref', exitCode: ok ? 0 : 128 })
  }
  return { executor, calls }
}

/** A real git executor — resolves (never rejects) with the captured exit code. */
const realExecutor: RustGitExecutor = (args) =>
  new Promise((resolve) => {
    execFile('git', args, { cwd: process.cwd() }, (err, stdout, stderr) => {
      const exitCode = err && typeof err.code === 'number' ? err.code : err ? 1 : 0
      resolve({ stdout: stdout ?? '', stderr: stderr ?? '', exitCode })
    })
  })

suite('git executor bridge (A bridge)', () => {
  it('drives the executor with the check-ref-format args for a valid branch', async () => {
    const { executor, calls } = mockExecutor()
    const res = await git.validateGitPushTargetViaExecutor('origin', 'main', null, executor)
    expect(res).toBeNull()
    expect(calls).toEqual([['check-ref-format', '--branch', 'main']])
  })

  it('short-circuits shape-invalid targets in Rust WITHOUT calling the executor', async () => {
    const { executor, calls } = mockExecutor()
    const res = await git.validateGitPushTargetViaExecutor('origin', '-rf', null, executor)
    expect(res).toBe('Invalid git branch name: -rf')
    expect(calls).toEqual([]) // shape check ran first; git was never invoked
  })

  it('rejects a shape-valid branch that git check-ref-format fails (non-zero exit classified)', async () => {
    const { executor, calls } = mockExecutor()
    const res = await git.validateGitPushTargetViaExecutor('origin', 'foo..bar', null, executor)
    expect(res).not.toBeNull()
    expect(calls).toEqual([['check-ref-format', '--branch', 'foo..bar']])
  })

  it('rejects a shape-invalid remote name in Rust before the git call', async () => {
    const { executor, calls } = mockExecutor()
    const res = await git.validateGitPushTargetViaExecutor('foo//bar', 'main', null, executor)
    expect(res).toBe('Invalid git remote name: foo//bar')
    expect(calls).toEqual([])
  })

  it('is re-entrant: many concurrent drives resolve independently', async () => {
    const { executor } = mockExecutor()
    const results = await Promise.all([
      git.validateGitPushTargetViaExecutor('origin', 'main', null, executor),
      git.validateGitPushTargetViaExecutor('origin', '-x', null, executor),
      git.validateGitPushTargetViaExecutor('origin', 'feature/y', null, executor),
      git.validateGitPushTargetViaExecutor('origin', 'foo..bar', null, executor)
    ])
    expect(results[0]).toBeNull()
    expect(results[1]).toBe('Invalid git branch name: -x')
    expect(results[2]).toBeNull()
    expect(results[3]).not.toBeNull()
  })

  it('drives REAL git end-to-end (valid branch passes check-ref-format)', async () => {
    const res = await git.validateGitPushTargetViaExecutor('origin', 'main', null, realExecutor)
    expect(res).toBeNull()
  })
})
