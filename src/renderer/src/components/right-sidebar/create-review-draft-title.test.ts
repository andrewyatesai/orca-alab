// Init the orca-git wasm so humanizeBranchSlug returns real Rust results instead
// of the pre-ready pass-through fallback (this is a node-env test).
import '@/lib/git-wasm/init-git-wasm-for-test'
import { describe, expect, it } from 'vitest'
import { resolveCreateReviewDraftTitle } from './create-review-draft-title'

describe('resolveCreateReviewDraftTitle', () => {
  it('uses the hosted eligibility title when one is available', () => {
    expect(
      resolveCreateReviewDraftTitle({
        branch: 'feature/improve-diff-view',
        eligibilityTitle: '  Improve diff view  '
      })
    ).toBe('Improve diff view')
  })

  it('falls back when the hosted eligibility title is blank', () => {
    expect(
      resolveCreateReviewDraftTitle({
        branch: 'feature/restore-create-review',
        eligibilityTitle: '   '
      })
    ).toBe('Restore create review')
  })

  it('falls back to a readable title from the branch leaf', () => {
    expect(
      resolveCreateReviewDraftTitle({
        branch: 'refs/heads/feature/improve-diff-view-per-file',
        eligibilityTitle: null
      })
    ).toBe('Improve diff view per file')
  })

  it('normalizes remote refs before deriving the branch leaf', () => {
    expect(
      resolveCreateReviewDraftTitle({
        branch: 'refs/remotes/origin/feature/improve-diff-view-per-file'
      })
    ).toBe('Improve diff view per file')
  })
})
