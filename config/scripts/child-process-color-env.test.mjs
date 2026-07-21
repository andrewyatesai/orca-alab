import { describe, expect, it } from 'vitest'
import { normalizeChildColorEnv } from './child-process-color-env.mjs'

describe('normalizeChildColorEnv', () => {
  it('translates NO_COLOR into an explicit FORCE_COLOR=0 for child CLIs', () => {
    expect(normalizeChildColorEnv({ NO_COLOR: '1', PATH: '/bin' })).toEqual({
      DEBUG_COLORS: '0',
      FORCE_COLOR: '0',
      PATH: '/bin'
    })
  })

  it('keeps an explicit FORCE_COLOR choice and removes the conflicting variable', () => {
    expect(normalizeChildColorEnv({ NO_COLOR: '1', FORCE_COLOR: '3' })).toEqual({
      FORCE_COLOR: '3'
    })
  })

  it('does not mutate the source environment', () => {
    const source = { NO_COLOR: '' }
    normalizeChildColorEnv(source)
    expect(source).toEqual({ NO_COLOR: '' })
  })
})
