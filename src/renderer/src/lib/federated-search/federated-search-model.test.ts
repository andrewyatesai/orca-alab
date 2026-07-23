import { describe, expect, it } from 'vitest'
import { federatedEffectiveCaseSensitive } from './federated-search-model'

describe('federatedEffectiveCaseSensitive (§1 smart-case)', () => {
  it('stays case-insensitive for all-lowercase queries with the toggle off', () => {
    expect(federatedEffectiveCaseSensitive('needle', false)).toBe(false)
    expect(federatedEffectiveCaseSensitive('needle 42 %', false)).toBe(false)
  })

  it('turns case-sensitive when the query contains an uppercase letter', () => {
    expect(federatedEffectiveCaseSensitive('Needle', false)).toBe(true)
    expect(federatedEffectiveCaseSensitive('error: NULL', false)).toBe(true)
    // Non-ASCII uppercase counts (\p{Lu}).
    expect(federatedEffectiveCaseSensitive('İstanbul', false)).toBe(true)
  })

  it('the explicit toggle forces sensitivity regardless of query casing', () => {
    expect(federatedEffectiveCaseSensitive('needle', true)).toBe(true)
    expect(federatedEffectiveCaseSensitive('', true)).toBe(true)
  })
})
