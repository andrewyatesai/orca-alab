import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { GENERATED_TAB_TITLE_MAX_LENGTH } from '../../../../shared/agent-tab-title'
import { deriveGeneratedTabTitle } from './agent-tab-title'
import { initGitWasmForTestFromBytes } from './git-line-stats'

// Ported from the deleted src/shared/agent-tab-title.test.ts: the same golden
// expectations now run THROUGH the Rust orca-text core via wasm (the spy-based
// normalization-bound test died with the TS body — the 512-unit preview cap is
// pinned in the Rust crate's tests).

const preInitTitle = deriveGeneratedTabTitle('Fix the flaky status tests')

beforeAll(() => {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('deriveGeneratedTabTitle (orca-text wasm)', () => {
  it('returns null before the wasm is ready (tab keeps its default title)', () => {
    expect(preInitTitle).toBeNull()
  })

  it('derives a short title from the first useful prompt clause', () => {
    expect(
      deriveGeneratedTabTitle('Can you please refactor the auth middleware to use JWT tokens?')
    ).toBe('Refactor the auth middleware to use JWT')
  })

  it('strips markup, links, emoji, and punctuation from generated titles', () => {
    expect(
      deriveGeneratedTabTitle('Please fix `src/auth.ts`!!! https://example.com 🔥 then add tests')
    ).toBe('Fix src auth')
  })

  it('preserves non-ASCII title text while folding Unicode whitespace', () => {
    expect(deriveGeneratedTabTitle('Please 修正 résumé\t検索　１２３!!!')).toBe(
      '修正 résumé 検索 １２３'
    )
  })

  it('leads with the issue identifier and keeps the description after it', () => {
    expect(deriveGeneratedTabTitle('Issue #2056: Opt-in generated tab titles for agents')).toBe(
      'Issue 2056 - Opt in generated tab'
    )
  })

  it('recovers a PR number from a URL the pipeline strips, leading with it', () => {
    expect(
      deriveGeneratedTabTitle(
        'Review this community PR https://github.com/EveryInc/plugin/pull/1094'
      )
    ).toBe('PR 1094 - Review this community')
  })

  it('leads with a GitLab MR identifier', () => {
    expect(
      deriveGeneratedTabTitle('fix https://gitlab.com/group/app/-/merge_requests/42 quickly')
    ).toBe('MR 42 - Fix quickly')
  })

  it('strips a URL containing underscores intact', () => {
    expect(
      deriveGeneratedTabTitle('inspect https://gitlab.com/g/p/-/work_items/9 then report')
    ).toBe('Issue 9 - Inspect then report')
  })

  it('leads with the identifier for a URL wrapped in markdown emphasis, without leaking fragments', () => {
    const title = deriveGeneratedTabTitle('Review _https://github.com/o/r/pull/5_ now')
    expect(title).toBe('PR 5 - Review now')
    expect(title).not.toMatch(/https|pull/)
  })

  it('leads with a bare ticket key', () => {
    expect(deriveGeneratedTabTitle('implement ENG-456 login flow')).toBe(
      'ENG-456 - Implement login flow'
    )
  })

  it('does not treat a cipher token as an identifier', () => {
    expect(deriveGeneratedTabTitle('implement SHA-256 hashing in the signer')).toBe(
      'Implement SHA 256 hashing in the signer'
    )
  })

  it('bounds titles to the maximum length without adding punctuation', () => {
    const title = deriveGeneratedTabTitle(
      'I want to replace the terminal reconnection hydration flow with a safer retry path'
    )

    expect(title).toBeTruthy()
    expect(title!.length).toBeLessThanOrEqual(GENERATED_TAB_TITLE_MAX_LENGTH)
    expect(title).toMatch(/^[\p{L}\p{N}\s]+$/u)
  })

  it('returns null when the prompt has no useful title text', () => {
    expect(deriveGeneratedTabTitle('please!!!')).toBeNull()
  })

  it('caps paste-sized prompts at the 512-unit preview before deriving', () => {
    const title = deriveGeneratedTabTitle(
      `Please fix \`src/auth.ts\` ${'large pasted text '.repeat(5000)}`
    )

    expect(title).toBeTruthy()
    expect(title!.length).toBeLessThanOrEqual(GENERATED_TAB_TITLE_MAX_LENGTH)
  })
})
