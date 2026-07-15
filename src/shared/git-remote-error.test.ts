import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  isDivergentPullReconciliationError,
  MERGE_RECONCILIATION_PULL_ARGS,
  pullArgsSpecifyReconciliation,
  runPullWithDivergenceFallback,
  stripCredentialsFromMessage
} from './git-remote-error'

// The text-normalization helpers (normalizeGitErrorMessage,
// formatSubmodulePushFailureDetail, isNoUpstreamError) moved to the Rust
// orca-text core; their assertions now live in
// rust/crates/orca-text/src/git_remote_error.rs. Only the divergent-pull retry
// control flow stays in TS, so only it is exercised here.

afterEach(() => {
  vi.restoreAllMocks()
})

describe('stripCredentialsFromMessage', () => {
  it('scrubs userpass and token credentials on https', () => {
    expect(stripCredentialsFromMessage('remote: https://user:ghp_secret@github.com/o/r.git')).toBe(
      'remote: https://github.com/o/r.git'
    )
    expect(stripCredentialsFromMessage('https://ghp_token@github.com/o/r.git')).toBe(
      'https://github.com/o/r.git'
    )
  })

  it('keeps a raw BOM/NEL inside the credential span so it still redacts', () => {
    // The credential class bounds on ASCII whitespace, not `\s` — so a U+FEFF
    // (BOM) or U+0085 (NEL) byte stays in the credential and is scrubbed,
    // byte-identically to the Rust core (guards the JS-vs-Rust `\s` drift).
    expect(stripCredentialsFromMessage('https://user:ghp_\uFEFFsecret@github.com/o/r.git')).toBe(
      'https://github.com/o/r.git'
    )
    expect(stripCredentialsFromMessage('https://ghp_\u0085token@github.com/o/r.git')).toBe(
      'https://github.com/o/r.git'
    )
  })

  it('leaves a non-contiguous pseudo-credential (real whitespace bound) untouched', () => {
    expect(stripCredentialsFromMessage('https://user:pass @github.com/o/r.git')).toBe(
      'https://user:pass @github.com/o/r.git'
    )
  })
})

describe('isDivergentPullReconciliationError', () => {
  it('detects git 2.27+ divergent-branch reconciliation failures', () => {
    const error = new Error(
      'Command failed: git pull\n' +
        'hint: You have divergent branches and need to specify how to reconcile them.\n' +
        'fatal: Need to specify how to reconcile divergent branches.'
    )

    expect(isDivergentPullReconciliationError(error)).toBe(true)
  })

  it('does not match a fast-forward-only abort on divergent branches', () => {
    const error = new Error(
      'Command failed: git pull\nfatal: Not possible to fast-forward, aborting.'
    )

    expect(isDivergentPullReconciliationError(error)).toBe(false)
  })

  it('returns false for non-Error values', () => {
    expect(isDivergentPullReconciliationError('divergent branches')).toBe(false)
  })
})

describe('pullArgsSpecifyReconciliation', () => {
  it('is false when no strategy flag is present', () => {
    expect(pullArgsSpecifyReconciliation([])).toBe(false)
    expect(pullArgsSpecifyReconciliation(['origin', 'main'])).toBe(false)
  })

  it('is true for any explicit reconciliation flag', () => {
    expect(pullArgsSpecifyReconciliation(['--ff-only'])).toBe(true)
    expect(pullArgsSpecifyReconciliation(['--rebase'])).toBe(true)
    expect(pullArgsSpecifyReconciliation(['--no-rebase'])).toBe(true)
    expect(pullArgsSpecifyReconciliation(['--rebase=interactive'])).toBe(true)
    expect(pullArgsSpecifyReconciliation(['-r'])).toBe(true)
  })
})

describe('runPullWithDivergenceFallback', () => {
  const divergentError = new Error(
    'Command failed: git pull\n' +
      'hint: You have divergent branches and need to specify how to reconcile them.\n' +
      'fatal: Need to specify how to reconcile divergent branches.'
  )

  it('retries with merge reconciliation args on a policy error', async () => {
    const calls: string[][] = []
    const runPull = vi.fn(async (effectiveArgs: string[]) => {
      calls.push(effectiveArgs)
      if (effectiveArgs.length === 0) {
        throw divergentError
      }
    })

    await runPullWithDivergenceFallback([], runPull)

    expect(calls).toEqual([[], [...MERGE_RECONCILIATION_PULL_ARGS]])
    expect(runPull).toHaveBeenCalledTimes(2)
  })

  it('does not retry when pull args already specify reconciliation', async () => {
    const runPull = vi.fn(async () => {
      throw divergentError
    })

    await expect(runPullWithDivergenceFallback(['--ff-only'], runPull)).rejects.toBe(divergentError)
    expect(runPull).toHaveBeenCalledTimes(1)
    expect(runPull).toHaveBeenCalledWith(['--ff-only'])
  })

  it('rethrows non-divergence errors without retrying', async () => {
    const otherError = new Error('fatal: Not possible to fast-forward, aborting.')
    const runPull = vi.fn(async () => {
      throw otherError
    })

    await expect(runPullWithDivergenceFallback([], runPull)).rejects.toBe(otherError)
    expect(runPull).toHaveBeenCalledTimes(1)
  })
})
