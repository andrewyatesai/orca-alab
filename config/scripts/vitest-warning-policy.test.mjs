import { describe, expect, it } from 'vitest'
import { isExpectedNodeSqliteWarning } from '../vitest-warning-policy'

describe('Vitest Node warning policy', () => {
  it('recognizes only the known node:sqlite experimental warning', () => {
    expect(
      isExpectedNodeSqliteWarning(
        'SQLite is an experimental feature and might change at any time',
        'ExperimentalWarning'
      )
    ).toBe(true)

    const warning = new Error('SQLite is an experimental feature and might change at any time')
    warning.name = 'ExperimentalWarning'
    expect(isExpectedNodeSqliteWarning(warning)).toBe(true)
  })

  it('keeps unexpected warning messages and classes visible', () => {
    expect(
      isExpectedNodeSqliteWarning(
        'A different feature is experimental and might change at any time',
        'ExperimentalWarning'
      )
    ).toBe(false)
    expect(
      isExpectedNodeSqliteWarning(
        'SQLite is an experimental feature and might change at any time',
        'DeprecationWarning'
      )
    ).toBe(false)
  })
})
