import { describe, expect, it } from 'vitest'
import {
  detectPiAgentKindFromCommand,
  getUpstreamStatus,
  resolveGitRemoteRebaseSource,
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

// The async "A bridge": orca-git's rebase-source resolver drives the (mock) git
// executor through wasm_bindgen_futures — the SAME two-call sequence the main
// process runs via napi, awaited instead of block_on'd.
describe('resolveGitRemoteRebaseSource (orca-git wasm A-bridge)', () => {
  it('picks the longest matching remote and strips the refs/remotes prefix', async () => {
    const calls: string[][] = []
    const runGit = async (args: string[]) => {
      calls.push(args)
      return { stdout: args[0] === 'remote' ? 'origin\nupstream\n' : '', stderr: '' }
    }
    const source = await resolveGitRemoteRebaseSource(runGit, 'refs/remotes/upstream/main')
    expect(source).toEqual({
      remoteName: 'upstream',
      branchName: 'main',
      displayName: 'upstream/main'
    })
    // Exactly the two read-only calls, in order: list remotes, then validate branch.
    expect(calls).toEqual([['remote'], ['check-ref-format', '--branch', 'main']])
  })

  it('rejects with the RAW resolver message when no remote matches', async () => {
    const runGit = async () => ({ stdout: 'origin\n', stderr: '' })
    await expect(resolveGitRemoteRebaseSource(runGit, 'local-branch')).rejects.toThrow(
      'Choose a remote base branch to rebase from.'
    )
  })

  it('rejects empty/flag-like base refs without running git', async () => {
    const runGit = async () => ({ stdout: 'origin\n', stderr: '' })
    await expect(resolveGitRemoteRebaseSource(runGit, '   ')).rejects.toThrow(
      'Choose a remote base branch to rebase from.'
    )
    await expect(resolveGitRemoteRebaseSource(runGit, '-rf')).rejects.toThrow(
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
