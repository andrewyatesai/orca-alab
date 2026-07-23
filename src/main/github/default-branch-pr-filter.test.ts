import { describe, expect, it } from 'vitest'
import {
  defaultBaseRefToBranchName,
  isStaleClosedDefaultBranchPR
} from './default-branch-pr-filter'

describe('defaultBaseRefToBranchName', () => {
  it('strips the remote segment from a default base ref', () => {
    expect(defaultBaseRefToBranchName('origin/main')).toBe('main')
    expect(defaultBaseRefToBranchName('upstream/master')).toBe('master')
  })

  it('preserves branch names that contain slashes', () => {
    expect(defaultBaseRefToBranchName('origin/release/v2')).toBe('release/v2')
  })

  it('returns the ref unchanged when there is no remote prefix', () => {
    expect(defaultBaseRefToBranchName('main')).toBe('main')
  })

  it('returns null for empty or missing refs', () => {
    expect(defaultBaseRefToBranchName(null)).toBeNull()
    expect(defaultBaseRefToBranchName('')).toBeNull()
    expect(defaultBaseRefToBranchName('origin/')).toBeNull()
  })
})

describe('isStaleClosedDefaultBranchPR', () => {
  it('drops a closed implicit PR discovered on the default branch (#9171)', () => {
    expect(
      isStaleClosedDefaultBranchPR({
        branchName: 'master',
        defaultBranchName: 'master',
        prState: 'closed'
      })
    ).toBe(true)
  })

  it('keeps an OPEN PR from the default branch (fork master->upstream, master->release)', () => {
    expect(
      isStaleClosedDefaultBranchPR({
        branchName: 'master',
        defaultBranchName: 'master',
        prState: 'open'
      })
    ).toBe(false)
  })

  it('does not touch merged matches (their head-oid preservation logic still applies)', () => {
    expect(
      isStaleClosedDefaultBranchPR({
        branchName: 'master',
        defaultBranchName: 'master',
        prState: 'merged'
      })
    ).toBe(false)
  })

  it('keeps a closed PR on a non-default branch', () => {
    expect(
      isStaleClosedDefaultBranchPR({
        branchName: 'feature/x',
        defaultBranchName: 'master',
        prState: 'closed'
      })
    ).toBe(false)
  })

  it('never fires for an explicitly linked PR number', () => {
    expect(
      isStaleClosedDefaultBranchPR({
        branchName: 'master',
        defaultBranchName: 'master',
        prState: 'closed',
        linkedPRNumber: 123
      })
    ).toBe(false)
  })

  it('is inert when the default branch cannot be resolved', () => {
    expect(
      isStaleClosedDefaultBranchPR({
        branchName: 'master',
        defaultBranchName: null,
        prState: 'closed'
      })
    ).toBe(false)
  })
})
