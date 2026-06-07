// TS dispatch for the open-in-applications parity module: maps the shared
// vector function names to the real `src/shared/open-in-applications.ts`
// exports so the harness compares the live TS reference against the Rust port.

import { normalizeOpenInApplications } from '../../../src/shared/open-in-applications'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeOpenInApplications': {
      const { value, seedDefaults, createIds } = input as {
        value?: unknown
        seedDefaults?: boolean
        createIds?: string[]
      }
      // `createIds` reifies the optional id generator as plain JSON: each blank
      // id pops the next entry (empty string once exhausted == falls back to a
      // positional id), so the closure is reproducible in both adapters.
      let index = 0
      const createId = Array.isArray(createIds)
        ? (): string => createIds[index++] ?? ''
        : undefined
      return normalizeOpenInApplications(value, { seedDefaults, createId })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
