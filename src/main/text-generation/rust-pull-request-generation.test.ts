// Behavioural coverage for the PR-fields generator, now driven through the napi
// binding to the Rust orca-agents core (the shared TS bodies were deleted). Lives
// in the node project because it loads the native addon (the src/shared → src/main
// relocation precedent from the workspace-session parse cutover). The prompt-string
// shape + parse edge cases are pinned by the parity vectors; these assert the
// production napi surface end-to-end.
import { describe, expect, it } from 'vitest'
import {
  buildPullRequestFieldsPrompt,
  parseGeneratedPullRequestFields
} from './rust-pull-request-generation'
import type { PullRequestDraftContext } from '../../shared/pull-request-generation'

const context: PullRequestDraftContext = {
  branch: 'feature/pr-details',
  base: 'main',
  branchChangedByPreparation: false,
  currentTitle: 'Feature pr details',
  currentBody: '- Add form',
  currentDraft: false,
  commitSummary: '- feat: add generated PR details',
  changeSummary: 'M\tsrc/file.ts',
  patch: 'diff --git a/src/file.ts b/src/file.ts\n+export const value = true'
}

describe('buildPullRequestFieldsPrompt', () => {
  it('asks for compact JSON and includes PR context', () => {
    const prompt = buildPullRequestFieldsPrompt(context, 'Use conventional PR titles.')

    expect(prompt).toContain('Return ONLY compact JSON')
    expect(prompt).toContain('Head branch: feature/pr-details')
    expect(prompt).toContain('Current base: main')
    expect(prompt).toContain('Additional user prompt:')
    expect(prompt).toContain('Use conventional PR titles.')
  })

  it('tells the agent to preserve existing review templates', () => {
    const prompt = buildPullRequestFieldsPrompt(
      { ...context, currentBody: '## Summary\n\n## Testing\n\n- [ ] Required checks' },
      ''
    )

    expect(prompt).toContain('preserve its headings, required sections, and checklists')
    expect(prompt).toContain('Leave genuinely unknown template items as TODO or unchecked')
  })
})

describe('parseGeneratedPullRequestFields', () => {
  it('parses fenced JSON output and strips the trailing period from the title', () => {
    const fields = parseGeneratedPullRequestFields(
      '```json\n{"base":"main","title":"fix: add details.","body":"Summary","draft":true}\n```',
      context
    )

    expect(fields).toEqual({ base: 'main', title: 'fix: add details', body: 'Summary', draft: true })
  })

  it('parses CRLF fenced JSON output', () => {
    const fields = parseGeneratedPullRequestFields(
      '```JSON\r\n{"base":"main","title":"fix: add details.","body":"Summary","draft":true}\r\n```',
      context
    )

    expect(fields.title).toBe('fix: add details')
  })

  it('falls back for missing optional values', () => {
    const fields = parseGeneratedPullRequestFields('{"title":""}', context)

    expect(fields).toEqual({
      base: 'main',
      title: 'Feature pr details',
      body: '- Add form',
      draft: false
    })
  })

  it('treats a JSON array as an empty object (all fallbacks, no throw)', () => {
    const fields = parseGeneratedPullRequestFields('[1, 2, 3]', context)

    expect(fields).toEqual({
      base: 'main',
      title: 'Feature pr details',
      body: '- Add form',
      draft: false
    })
  })

  it('throws on a non-object, non-array payload', () => {
    expect(() => parseGeneratedPullRequestFields('42', context)).toThrow()
  })
})
