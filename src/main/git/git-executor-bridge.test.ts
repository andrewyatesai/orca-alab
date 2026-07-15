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

suite('pull-rebase from base via bridge (resolve + mutating pull, collapsed)', () => {
  const drive = async (
    baseRef: string,
    responder: (args: string[]) => { stdout?: string; stderr?: string; exitCode: number }
  ): Promise<{ ok?: boolean; error?: string; calls: string[][] }> => {
    const { executor, calls } = respondingExecutor(responder)
    try {
      await git.gitPullRebaseFromBaseViaExecutor(baseRef, executor)
      return { ok: true, calls }
    } catch (e) {
      return { error: e instanceof Error ? e.message : String(e), calls }
    }
  }
  const listRemotes = (stdout: string) => (a: string[]) => {
    if (a[0] === 'remote') {
      return { stdout, exitCode: 0 }
    }
    return { exitCode: 0 } // check-ref-format + pull ok
  }

  it('resolves a refs/remotes/ base ref then runs the mutating pull --rebase', async () => {
    const { ok, calls } = await drive('refs/remotes/origin/main', listRemotes('origin\n'))
    expect(ok).toBe(true)
    // One collapsed call: list remotes, validate branch, then pull --rebase.
    expect(calls).toEqual([
      ['remote'],
      ['check-ref-format', '--branch', 'main'],
      ['pull', '--rebase', 'origin', 'main']
    ])
  })

  it('picks the LONGEST matching remote name for the pull', async () => {
    const { ok, calls } = await drive(
      'refs/remotes/origin-fork/feature/x',
      listRemotes('origin\norigin-fork\n')
    )
    expect(ok).toBe(true)
    expect(calls.at(-1)).toEqual(['pull', '--rebase', 'origin-fork', 'feature/x'])
  })

  it('rejects empty/flag-like base refs (normalized) with ZERO git calls', async () => {
    for (const bad of ['', '   ', '-rf']) {
      const { error, calls } = await drive(bad, listRemotes('origin\n'))
      expect(error).toBe('Choose a remote base branch to rebase from.')
      expect(calls).toEqual([])
    }
  })

  it('rejects when no configured remote matches (after listing remotes, before pull)', async () => {
    const { error, calls } = await drive('main', listRemotes('origin\n'))
    expect(error).toBe('Choose a remote base branch to rebase from.')
    expect(calls.map((c) => c[0])).toEqual(['remote'])
  })

  it('surfaces the check-ref-format failure for a malformed branch and skips the pull', async () => {
    const { error, calls } = await drive('refs/remotes/origin/bad..name', (a) => {
      if (a[0] === 'remote') {
        return { stdout: 'origin\n', exitCode: 0 }
      }
      return { stderr: "fatal: 'bad..name' is not a valid branch name", exitCode: 128 }
    })
    // Single-line git diagnostic survives normalize(Pull) via the tail-line rule.
    expect(error).toBe("fatal: 'bad..name' is not a valid branch name")
    // The mutating pull never ran — branch validation failed first.
    expect(calls.map((c) => c[0])).toEqual(['remote', 'check-ref-format'])
  })
})

suite('branch cleanup decision via bridge (stdin: git patch-id)', () => {
  // Deterministic responder for a branch that is squash-merged (its net patch
  // matches a squash commit on the target) yet carries a merge commit.
  const squashScenario = (
    branchPatchId: string,
    squashPatchId: string,
    stdinSeen: (string | null)[]
  ): RustGitExecutor => {
    const map: Record<string, string> = {
      'config --get branch.feature.base': 'origin/main',
      'rev-parse --verify --quiet origin/main^{commit}': 'TOID',
      'merge-tree --write-tree TOID refs/heads/feature': 'OTHERTREE\n',
      'rev-parse --verify --quiet TOID^{tree}': 'TTREE',
      'rev-list --right-only --merges --count TOID...refs/heads/feature': '1',
      'merge-base TOID refs/heads/feature': 'MBASE',
      'diff MBASE refs/heads/feature': 'BRANCH_PATCH_TEXT',
      'show --format= SQUASH': 'SQUASH_PATCH_TEXT',
      'merge-tree --write-tree SQUASH refs/heads/feature': 'STREE\n',
      'rev-parse --verify --quiet SQUASH^{tree}': 'STREE'
    }
    return (args, stdin) => {
      let stdout: string | undefined = map[args.join(' ')]
      if (args[0] === 'rev-list' && args[1] === '--ancestry-path') {
        stdout = 'SQUASH\n'
      }
      if (args[0] === 'patch-id') {
        stdinSeen.push(stdin)
        stdout = stdin === 'BRANCH_PATCH_TEXT' ? branchPatchId : stdin === 'SQUASH_PATCH_TEXT' ? squashPatchId : ''
      }
      const found = stdout !== undefined
      return Promise.resolve({ stdout: found ? stdout : '', stderr: '', exitCode: found ? 0 : 1 })
    }
  }

  it('deletes a squash-merged branch — patch text piped to git patch-id via stdin', async () => {
    const stdinSeen: (string | null)[] = []
    // same patch-id (first token) -> squash match -> safe. Output is
    // "<patch-id> <commit-id>", so the shared leading token is the patch-id.
    const safe = await git.branchIsSafeToDeleteViaExecutor(
      'feature',
      squashScenario('SAMEID aaa\n', 'SAMEID bbb\n', stdinSeen)
    )
    expect(safe).toBe(true)
    expect(stdinSeen).toEqual(['BRANCH_PATCH_TEXT', 'SQUASH_PATCH_TEXT'])
  })

  it('preserves a merge-commit branch whose patch-id matches no squash commit', async () => {
    const stdinSeen: (string | null)[] = []
    // different patch-ids (leading token) -> no squash match -> preserve
    const safe = await git.branchIsSafeToDeleteViaExecutor(
      'feature',
      squashScenario('BRANCHID aaa\n', 'SQUASHID bbb\n', stdinSeen)
    )
    expect(safe).toBe(false)
  })
})
