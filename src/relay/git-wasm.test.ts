import { describe, expect, it } from 'vitest'
import {
  branchIsSafeToDelete,
  detectPiAgentKindFromCommand,
  getUpstreamStatus,
  gitFetch,
  gitPullRebaseFromBase,
  gitPush,
  upstreamOnlyCommitsArePatchEquivalent
} from './git-wasm'

// The wrappers initSync the embedded wasm lazily — no setup needed. These pin
// the relay-side wasm path for the functions whose shared TS was deleted
// (ported from the deleted halves of the shared test files; the spy-based
// TS-internal assertions were dropped with the TS implementations).

describe('upstreamOnlyCommitsArePatchEquivalent (orca-git wasm)', () => {
  it('returns true when every upstream-only commit is patch-equivalent', () => {
    expect(upstreamOnlyCommitsArePatchEquivalent('= abc\n= def\n')).toBe(true)
  })

  it('returns false for empty output or non-equivalent commits', () => {
    expect(upstreamOnlyCommitsArePatchEquivalent('')).toBe(false)
    expect(upstreamOnlyCommitsArePatchEquivalent('= abc\n+ def\n')).toBe(false)
  })

  it('scans newline-heavy CRLF cherry output', () => {
    expect(
      upstreamOnlyCommitsArePatchEquivalent(`${'\r\n'.repeat(10_000)}= abc\r\n= def\r\n`)
    ).toBe(true)
  })
})

describe('detectPiAgentKindFromCommand (orca-git wasm)', () => {
  it('matches the napi-side detector for the boundary cases', () => {
    expect(detectPiAgentKindFromCommand(undefined)).toBe('pi')
    expect(detectPiAgentKindFromCommand('omp.sh --resume')).toBe('omp')
    expect(detectPiAgentKindFromCommand('pip install foo')).toBe('pi')
    expect(detectPiAgentKindFromCommand('pomp.exe')).toBe('pi')
  })
})

// The async "A bridge": orca-git's git_pull_rebase_from_base drives the (mock) git
// executor through wasm_bindgen_futures — resolve the source (list remotes →
// check-ref-format) AND run the mutating pull --rebase in one call, the SAME
// sequence the main process runs via napi, awaited instead of block_on'd.
describe('gitPullRebaseFromBase (orca-git wasm A-bridge)', () => {
  it('resolves the longest matching remote then pulls --rebase', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      return { stdout: args[0] === 'remote' ? 'origin\nupstream\n' : '', stderr: '' }
    }
    await gitPullRebaseFromBase(runGit, 'refs/remotes/upstream/main')
    // List remotes, validate branch, then the mutating rebase — one collapsed call.
    expect(calls).toEqual([
      ['remote'],
      ['check-ref-format', '--branch', 'main'],
      ['pull', '--rebase', 'upstream', 'main']
    ])
  })

  it('rejects with the resolver message (normalized as pull) when no remote matches', async () => {
    const runGit = async () => ({ stdout: 'origin\n', stderr: '' })
    await expect(gitPullRebaseFromBase(runGit, 'local-branch')).rejects.toThrow(
      'Choose a remote base branch to rebase from.'
    )
  })

  it('rejects empty/flag-like base refs without running the mutating pull', async () => {
    const runGit = async () => ({ stdout: 'origin\n', stderr: '' })
    await expect(gitPullRebaseFromBase(runGit, '   ')).rejects.toThrow(
      'Choose a remote base branch to rebase from.'
    )
    await expect(gitPullRebaseFromBase(runGit, '-rf')).rejects.toThrow(
      'Choose a remote base branch to rebase from.'
    )
  })
})

// Explicit publish-target upstream status through the async A-bridge: Rust does
// check-ref-format → rev-parse → rev-list → cherry-mark, exactly as main's napi.
describe('getUpstreamStatus (orca-git wasm A-bridge, explicit target)', () => {
  const target = { remoteName: 'fork', branchName: 'feature/fix' }
  const remoteRef = 'refs/remotes/fork/feature/fix'

  it('runs the full sequence and reports ahead/behind + patch-equivalence', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      if (args[0] === 'rev-list') return { stdout: '1\t2\n', stderr: '' }
      // Diverged but not rebased → cherry-mark shows a non-'=' commit.
      if (args[0] === 'log') return { stdout: '+ def456 remote work\n', stderr: '' }
      return { stdout: '', stderr: '' } // check-ref-format, rev-parse
    }
    const status = await getUpstreamStatus(runGit, target)
    expect(status).toEqual({
      hasUpstream: true,
      upstreamName: 'fork/feature/fix',
      ahead: 1,
      behind: 2,
      behindCommitsArePatchEquivalent: false
    })
    expect(calls).toEqual([
      ['check-ref-format', '--branch', 'feature/fix'],
      ['rev-parse', '--verify', '--quiet', remoteRef],
      ['rev-list', '--left-right', '--count', `HEAD...${remoteRef}`],
      ['log', '--oneline', '--cherry-mark', '--right-only', `HEAD...${remoteRef}`, '--']
    ])
  })

  it('treats a bare "exited 1" rev-parse as an unfetched publishable target', async () => {
    const runGit = async (args: string[]) => {
      if (args[0] === 'rev-parse') {
        // execFileAsync's rejection shape for a non-zero exit with no stderr.
        throw Object.assign(new Error('Command failed'), { code: 1, stdout: '', stderr: '' })
      }
      return { stdout: '', stderr: '' }
    }
    const status = await getUpstreamStatus(runGit, target)
    expect(status).toEqual({
      hasUpstream: false,
      upstreamName: 'fork/feature/fix',
      ahead: 0,
      behind: 0,
      hasConfiguredPushTarget: true
    })
  })
})

// The one destructive op through the async A-bridge: Rust resolves the push target
// (config-driven or explicit) and runs the mutating push; git stays in the relay.
describe('gitPush (orca-git wasm A-bridge)', () => {
  // A config-value runner: resolves listed keys, rejects the rest like git's exit 1.
  const configRunner = (config: Record<string, string>) => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      if (args[0] === 'symbolic-ref') return { stdout: 'feature\n', stderr: '' }
      if (args[0] === 'config' && args[1] === '--get') {
        const value = config[args[2]]
        if (value === undefined) throw Object.assign(new Error('miss'), { code: 1, stderr: '' })
        return { stdout: `${value}\n`, stderr: '' }
      }
      if (args[0] === 'push') return { stdout: '', stderr: '' }
      throw Object.assign(new Error('unexpected'), { code: 1, stderr: '' })
    }
    return { runGit, calls }
  }

  it('pushes to the configured pushRemote over branch.remote', async () => {
    const { runGit, calls } = configRunner({
      'branch.feature.remote': 'origin',
      'branch.feature.pushRemote': 'myfork', // wins
      'branch.feature.merge': 'refs/heads/feature'
    })
    await gitPush(runGit, undefined, false)
    expect(calls.at(-1)).toEqual(['push', '--set-upstream', 'myfork', 'HEAD:feature'])
  })

  it('pushes an explicit target with --force-with-lease', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      return { stdout: '', stderr: '' }
    }
    await gitPush(runGit, { remoteName: 'origin', branchName: 'contributor/fix' }, true)
    expect(calls).toEqual([
      ['check-ref-format', '--branch', 'contributor/fix'],
      ['push', '--force-with-lease', '--set-upstream', 'origin', 'HEAD:contributor/fix']
    ])
  })

  it('falls back to origin HEAD and normalizes a non-fast-forward rejection', async () => {
    const runGit = async (args: string[]) => {
      if (args[0] === 'push') {
        throw Object.assign(new Error('remote rejected: non-fast-forward'), {
          code: 1,
          stderr: 'remote rejected: non-fast-forward'
        })
      }
      // No branch / no config → configured-target resolution yields null.
      throw Object.assign(new Error('miss'), { code: 1, stderr: '' })
    }
    await expect(gitPush(runGit, undefined, false)).rejects.toThrow(
      'Push rejected: remote has newer commits (non-fast-forward). Please pull or sync first.'
    )
  })
})

// Fetch through the async A-bridge: Rust validates an explicit target, then runs
// the mutating `fetch --prune [<remote>]`; git stays in the relay. No
// effective-upstream resolution, unlike fast-forward/pull.
describe('gitFetch (orca-git wasm A-bridge)', () => {
  it('runs a plain prune-fetch when no target is given', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      return { stdout: '', stderr: '' }
    }
    await gitFetch(runGit, undefined)
    expect(calls).toEqual([['fetch', '--prune']])
  })

  it('validates then fetches an explicit publish target', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      return { stdout: '', stderr: '' }
    }
    await gitFetch(runGit, { remoteName: 'fork', branchName: 'feature/fix' })
    expect(calls).toEqual([
      ['check-ref-format', '--branch', 'feature/fix'],
      ['fetch', '--prune', 'fork']
    ])
  })

  it('normalizes a fetch authentication failure', async () => {
    const runGit = async () => {
      throw Object.assign(new Error('Authentication failed'), {
        code: 1,
        stderr: 'Authentication failed'
      })
    }
    await expect(gitFetch(runGit, undefined)).rejects.toThrow(
      'Authentication failed. Check your remote credentials.'
    )
  })
})

// Branch-cleanup safety decision through the async A-bridge: Rust gathers base
// refs, fetch --prunes, and runs the no-op-merge proof (with git patch-id stdin).
describe('branchIsSafeToDelete (orca-git wasm A-bridge)', () => {
  it('is safe to delete when the branch merges tree-equal into a base', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      if (args[0] === 'config') return { stdout: 'refs/remotes/origin/main\n', stderr: '' }
      if (args[0] === 'symbolic-ref') throw Object.assign(new Error('miss'), { code: 1, stderr: '' })
      if (args[0] === 'remote') return { stdout: 'origin\n', stderr: '' }
      if (args[0] === 'fetch') return { stdout: '', stderr: '' }
      if (args[0] === 'rev-parse' && args.at(-1)?.endsWith('^{commit}'))
        return { stdout: 'TOID\n', stderr: '' }
      if (args[0] === 'merge-tree') return { stdout: 'SAME\n', stderr: '' }
      if (args[0] === 'rev-parse' && args.at(-1) === 'TOID^{tree}')
        return { stdout: 'SAME\n', stderr: '' }
      return { stdout: '', stderr: '' }
    }
    expect(await branchIsSafeToDelete(runGit, 'feature')).toBe(true)
    // The one mutation — fetch --prune of the base's remote — ran.
    expect(calls).toContainEqual(['fetch', '--prune', 'origin'])
  })

  it('preserves a branch with distinct, non-equivalent commits', async () => {
    const runGit = async (args: string[]) => {
      if (args[0] === 'config') return { stdout: 'refs/remotes/origin/main\n', stderr: '' }
      if (args[0] === 'symbolic-ref') throw Object.assign(new Error('miss'), { code: 1, stderr: '' })
      if (args[0] === 'remote') return { stdout: 'origin\n', stderr: '' }
      if (args[0] === 'rev-parse' && args.at(-1)?.endsWith('^{commit}'))
        return { stdout: 'TOID\n', stderr: '' }
      if (args[0] === 'merge-tree') return { stdout: 'DIFFERENT\n', stderr: '' }
      if (args[0] === 'rev-parse' && args.at(-1) === 'TOID^{tree}')
        return { stdout: 'TTREE\n', stderr: '' }
      if (args[0] === 'rev-list') return { stdout: '0\n', stderr: '' } // no merge-only commits
      if (args[0] === 'cherry') return { stdout: '+ abc new work\n', stderr: '' } // non-equivalent
      return { stdout: '', stderr: '' }
    }
    expect(await branchIsSafeToDelete(runGit, 'feature')).toBe(false)
  })
})
