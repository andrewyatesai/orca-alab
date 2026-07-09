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

  it('includes the assistant response when present', () => {
    const prompt = buildBranchNamePrompt({
      firstPrompt: 'Add a logout button',
      assistantMessage: "I'll wire it into the header."
    })
    expect(prompt).toContain("Agent's initial response")
    expect(prompt).toContain("I'll wire it into the header.")
  })

  it('appends a custom branch-name prompt when present', () => {
    const prompt = buildBranchNamePrompt(
      { firstPrompt: 'Add a logout button' },
      'Prefer product nouns.'
    )
    expect(prompt).toContain('Additional user prompt:')
    expect(prompt).toContain('Prefer product nouns.')
  })
})
