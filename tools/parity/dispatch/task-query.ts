// TS dispatch for the task-query parity module: maps the shared vector function
// names to the real `src/shared/task-query.ts` exports so the harness compares
// the live TS reference against the Rust port.

import {
  parseTaskQuery,
  serializeTaskQuery,
  stripRepoQualifiers,
  tokenizeSearchQuery,
  withQualifier,
  type ParsedTaskQuery,
  type TaskQueryFilterKey
} from '../../../src/shared/task-query'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'tokenizeSearchQuery':
      return tokenizeSearchQuery(input as string)
    case 'parseTaskQuery':
      return parseTaskQuery(input as string)
    case 'serializeTaskQuery':
      return serializeTaskQuery(input as ParsedTaskQuery)
    case 'withQualifier': {
      const { rawQuery, key, value } = input as {
        rawQuery: string
        key: TaskQueryFilterKey
        value: string | string[] | null
      }
      return withQualifier(rawQuery, key, value)
    }
    case 'stripRepoQualifiers':
      return stripRepoQualifiers(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
