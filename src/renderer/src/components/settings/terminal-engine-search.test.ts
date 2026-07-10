import { describe, expect, it } from 'vitest'
import { matchesSettingsSearch } from './settings-search'
import { getTerminalEngineSearchEntries } from './terminal-engine-search'

describe('getTerminalEngineSearchEntries', () => {
  it('indexes literal Matrix Rain and its live-output source', () => {
    const entries = getTerminalEngineSearchEntries()
    const matrixRain = entries.find((entry) => entry.title === 'Matrix Rain')

    expect(matrixRain).toBeDefined()
    expect(matrixRain?.description).toContain('real glyphs')
    expect(matchesSettingsSearch('rain', [matrixRain!])).toBe(true)
    expect(matchesSettingsSearch('live output', [matrixRain!])).toBe(true)
  })
})
