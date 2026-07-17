import { describe, expect, it } from 'vitest'
import { buildBranchNamePrompt } from './branch-name-from-work'

// The 4 string helpers moved to the Rust branch-name-from-work core; their
// behavior is now covered by the parity harness. Only buildBranchNamePrompt
// stays in TS, so only its cases remain here.
describe('buildBranchNamePrompt', () => {
  it('includes the user prompt and omits the assistant section when absent', () => {
    const prompt = buildBranchNamePrompt({ firstPrompt: 'Add a logout button' })
    expect(prompt).toContain('Add a logout button')
    expect(prompt).not.toContain("Agent's initial response")
  })

  it('keeps the default prompt general without style rules', () => {
    const prompt = buildBranchNamePrompt({ firstPrompt: 'Add a logout button' })
    expect(prompt).toContain('Generate a short git branch name')
    expect(prompt).toContain('Output ONLY the branch name on a single line')
    expect(prompt).not.toContain('Rules:')
    expect(prompt).not.toMatch(/kebab-case/i)
    expect(prompt).not.toMatch(/between \d+ and \d+ words/i)
    expect(prompt).not.toMatch(/no prefixes/i)
  })

  it('includes the assistant response when present', () => {
    const prompt = buildBranchNamePrompt({
      firstPrompt: 'Add a logout button',
      assistantMessage: "I'll wire it into the header."
    })
    expect(prompt).toContain("Agent's initial response")
    expect(prompt).toContain("I'll wire it into the header.")
  })

  it('leads with a custom naming prompt so overrides can own style', () => {
    const prompt = buildBranchNamePrompt(
      { firstPrompt: 'Add a logout button' },
      'Prefer product nouns.'
    )
    expect(prompt.startsWith('Prefer product nouns.')).toBe(true)
    expect(prompt).not.toContain('Additional user prompt:')
    expect(prompt).not.toContain('Rules:')
    expect(prompt).toContain('Generate a git branch name')
    expect(prompt).toContain('Output ONLY the branch name on a single line')
    expect(prompt).toContain('Add a logout button')
  })
})
