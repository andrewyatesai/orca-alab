import { describe, expect, it } from 'vitest'
import { getAgentsPaneSearchEntries } from './agents-search'

describe('getAgentsPaneSearchEntries', () => {
  it('indexes custom instruction settings', () => {
    const haystack = JSON.stringify(getAgentsPaneSearchEntries()).toLowerCase()

    expect(haystack).toContain('custom instructions')
    expect(haystack).toContain('personalization')
    expect(haystack).toContain('system prompt')
  })
})
