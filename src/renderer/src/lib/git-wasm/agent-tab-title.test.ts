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

  it('strips markup, links, emoji, and punctuation without promoting incidental markers', () => {
    // #9821: a stray "#1" in prose must stay inline, never lead the title.
    expect(
      deriveGeneratedTabTitle(
        'Please fix auth note #1 with `src/auth.ts`!!! https://example.com 🔥'
      )
    ).toBe('Fix auth note 1 with src auth')
  })

  it('preserves non-ASCII title text while folding Unicode whitespace', () => {
    expect(deriveGeneratedTabTitle('Please 修正 résumé\t検索　１２３!!!')).toBe(
      '修正 résumé 検索 １２３'
    )
  })

  it('keeps useful text after common issue prefixes', () => {
    expect(deriveGeneratedTabTitle('Issue #2056: Opt-in generated tab titles for agents')).toBe(
      'Opt in generated tab titles for agents'
    )
  })

  it('strips a URL containing underscores intact', () => {
    // #8238 strip-order bugfix retained: URL removed before markdown, no leak.
    expect(
      deriveGeneratedTabTitle('inspect https://gitlab.com/g/p/-/work_items/9 then report')
    ).toBe('Inspect then report')
  })

  it('strips a URL wrapped in markdown emphasis without leaking fragments', () => {
    const title = deriveGeneratedTabTitle('Review _https://github.com/o/r/pull/5_ now')
    expect(title).toBe('Review now')
    expect(title).not.toMatch(/https|pull/)
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
