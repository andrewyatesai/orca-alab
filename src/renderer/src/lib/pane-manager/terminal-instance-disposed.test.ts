import { describe, expect, it } from 'vitest'
import { isTerminalInstanceDisposed } from './terminal-instance-disposed'

describe('isTerminalInstanceDisposed', () => {
  it('reads the facade isDisposed flag', () => {
    expect(isTerminalInstanceDisposed({ isDisposed: true })).toBe(true)
    expect(isTerminalInstanceDisposed({ isDisposed: false })).toBe(false)
  })

  it('degrades to false (treated as live) for non-facade shapes', () => {
    expect(isTerminalInstanceDisposed(null)).toBe(false)
    expect(isTerminalInstanceDisposed(undefined)).toBe(false)
    expect(isTerminalInstanceDisposed({})).toBe(false)
    // A truthy-but-non-boolean flag must not read as disposed (strict === true).
    expect(isTerminalInstanceDisposed({ isDisposed: 1 })).toBe(false)
  })
})
