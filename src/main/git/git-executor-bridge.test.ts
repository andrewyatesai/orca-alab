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

/** A mock git executor driven by a per-argv responder, recording calls. */
function respondingExecutor(
  responder: (args: string[]) => { stdout?: string; stderr?: string; exitCode: number }
): { executor: RustGitExecutor; calls: string[][] } {
  const calls: string[][] = []
  const executor: RustGitExecutor = (args) => {
    calls.push(args)
    const r = responder(args)
    return Promise.resolve({ stdout: r.stdout ?? '', stderr: r.stderr ?? '', exitCode: r.exitCode })
  }
  return { executor, calls }
}

suite('get upstream status via bridge (multi-round A bridge)', () => {
  const drive = async (
    branch: string,
    responder: (args: string[]) => { stdout?: string; stderr?: string; exitCode: number }
  ): Promise<{ status?: unknown; error?: string; calls: string[][] }> => {
    const { executor, calls } = respondingExecutor(responder)
    try {
      const json = await git.getUpstreamStatusViaExecutor('fork', branch, null, executor)
      return { status: JSON.parse(json), calls }
    } catch (e) {
      return { error: e instanceof Error ? e.message : String(e), calls }
    }
  }

  it('runs all four rounds and reports ahead/behind + patch-equivalence (non-equivalent)', async () => {
    const { status, calls } = await drive('feature', (a) => {
      if (a[0] === 'rev-list') {
        return { stdout: '1\t2\n', exitCode: 0 }
      }
      if (a[0] === 'log') {
        return { stdout: '+ abc work\n', exitCode: 0 }
      }
      return { exitCode: 0 }
    })
    expect(status).toEqual({
      hasUpstream: true,
      upstreamName: 'fork/feature',
      ahead: 1,
      behind: 2,
      behindCommitsArePatchEquivalent: false
    })
    expect(calls.map((c) => c[0])).toEqual(['check-ref-format', 'rev-parse', 'rev-list', 'log'])
  })

  it('marks behind commits patch-equivalent when the cherry-mark log is all "="', async () => {
    const { status } = await drive('feature', (a) => {
      if (a[0] === 'rev-list') {
        return { stdout: '5\t3\n', exitCode: 0 }
      }
      if (a[0] === 'log') {
        return { stdout: '= abc\n= def\n', exitCode: 0 }
      }
      return { exitCode: 0 }
    })
    expect((status as { behindCommitsArePatchEquivalent?: boolean }).behindCommitsArePatchEquivalent).toBe(
      true
    )
  })

  it('skips the cherry-mark log when not both ahead AND behind', async () => {
    const { status, calls } = await drive('feature', (a) => {
      if (a[0] === 'rev-list') {
        return { stdout: '3\t0\n', exitCode: 0 }
      }
      return { exitCode: 0 }
    })
    expect(status).toEqual({ hasUpstream: true, upstreamName: 'fork/feature', ahead: 3, behind: 0 })
    expect(calls.map((c) => c[0])).toEqual(['check-ref-format', 'rev-parse', 'rev-list']) // no log
  })

  it('reports no-upstream + hasConfiguredPushTarget when the tracking ref is unfetched (exit 1, empty stderr)', async () => {
    const { status, calls } = await drive('feature', (a) => {
      if (a[0] === 'rev-parse') {
        return { stdout: '', stderr: '', exitCode: 1 }
      }
      return { exitCode: 0 }
    })
    expect(status).toEqual({
      hasUpstream: false,
      upstreamName: 'fork/feature',
      ahead: 0,
      behind: 0,
      hasConfiguredPushTarget: true
    })
    expect(calls.map((c) => c[0])).toEqual(['check-ref-format', 'rev-parse']) // no rev-list/log
  })

  it('rejects (normalized) when rev-parse fails with a real stderr diagnostic', async () => {
    const { error, calls } = await drive('feature', (a) => {
      if (a[0] === 'rev-parse') {
        return { stderr: 'fatal: not a git repository', exitCode: 128 }
      }
      return { exitCode: 0 }
    })
    // Normalized to git's stderr tail line — NOT "git exited with Some(128)".
    expect(error).toBe('fatal: not a git repository')
    expect(calls.map((c) => c[0])).toEqual(['check-ref-format', 'rev-parse'])
  })

  it('rejects shape-invalid targets in Rust with ZERO git calls', async () => {
    const { error, calls } = await drive('-rf', () => ({ exitCode: 0 }))
    expect(error).toBe('Invalid git branch name: -rf')
    expect(calls).toEqual([])
  })

  it('is re-entrant across concurrent multi-round drives', async () => {
    const responder = (a: string[]): { stdout?: string; stderr?: string; exitCode: number } => {
      if (a[0] === 'rev-list') {
        return { stdout: '1\t1\n', exitCode: 0 }
      }
      if (a[0] === 'log') {
        return { stdout: '+ x\n', exitCode: 0 }
      }
      return { exitCode: 0 }
    }
    const results = await Promise.all([
      drive('a', responder),
      drive('b', responder),
      drive('c', responder)
    ])
    for (const r of results) {
      expect((r.status as { ahead: number }).ahead).toBe(1)
    }
  })

  it('drives REAL git end-to-end and returns a well-formed status', async () => {
    const json = await git.getUpstreamStatusViaExecutor('origin', 'main', null, realExecutor)
    const status = JSON.parse(json) as { hasUpstream: boolean; ahead: number; behind: number }
    expect(typeof status.hasUpstream).toBe('boolean')
    expect(typeof status.ahead).toBe('number')
    expect(typeof status.behind).toBe('number')
  })
})
