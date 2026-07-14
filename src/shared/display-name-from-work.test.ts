import { describe, expect, it } from 'vitest'
import { deriveWorkspaceDisplayName } from './display-name-from-work'

// The canonical humanizeBranchSlug lives in the Rust orca-core branch-name core;
// shared code receives it injected. This stub mirrors its kebab→sentence-case
// behavior so the composition cases stay engine-independent.
const humanizeBranchSlug = (slug: string): string => {
  const words = slug.split('-').filter(Boolean)
  return words.length === 0
    ? ''
    : [words[0].charAt(0).toUpperCase() + words[0].slice(1), ...words.slice(1)].join(' ')
}
const derive = (input: { prompt: string; slug: string; resolvedLeaf?: string }): string =>
  deriveWorkspaceDisplayName({ ...input, humanizeBranchSlug })

// Identifier extraction itself is covered in work-item-reference.test.ts; these
// cases exercise the end-to-end display-name composition.
describe('deriveWorkspaceDisplayName', () => {
  it('leads with the identifier and a single action verb', () => {
    expect(
      derive({
        prompt: 'Carefully evaluate https://github.com/o/r/pull/1033. Fix the merge conflict.',
        slug: 'review-community-pr-conflict'
      })
    ).toBe('PR 1033 - Review')
  })

  it('drops identifier tokens the slug also carried', () => {
    expect(
      derive({
        prompt: 'look at this community PR https://github.com/o/r/pull/1094',
        slug: 'review-community-pr-1094'
      })
    ).toBe('PR 1094 - Review')
  })

  it('uses a namespaced ticket id bare, without a type prefix', () => {
    expect(derive({ prompt: 'fix ENG-456 crash', slug: 'fix-eng-456-crash' })).toBe('ENG-456 - Fix')
  })

  it('returns the identifier alone when no action word survives', () => {
    expect(derive({ prompt: 'PR 12', slug: 'pr-12' })).toBe('PR 12')
  })

  it('carries a collision suffix so same-target worktrees stay distinct', () => {
    expect(
      derive({
        prompt: 'review https://github.com/o/r/pull/1033',
        slug: 'review-conflict',
        resolvedLeaf: 'review-conflict-2'
      })
    ).toBe('PR 1033 - Review (2)')
  })

  it('falls back to the humanized leaf when no identifier is present', () => {
    expect(derive({ prompt: 'add a dark mode toggle', slug: 'add-dark-mode-toggle' })).toBe(
      'Add dark mode toggle'
    )
  })

  it('humanizes the resolved leaf (with suffix) on the fallback path', () => {
    expect(
      derive({
        prompt: 'add a logout button',
        slug: 'add-logout-button',
        resolvedLeaf: 'add-logout-button-2'
      })
    ).toBe('Add logout button 2')
  })
})
