// TS dispatch for the repo-badge-color parity module: maps the shared vector
// function names to the real `src/shared/repo-badge-color.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  normalizeRepoBadgeColor,
  resolveRepoBadgeColor
} from '../../../src/shared/repo-badge-color'

export function dispatch(fn: string, input: unknown): unknown {
  const { value } = input as { value: unknown }
  switch (fn) {
    case 'normalizeRepoBadgeColor':
      return normalizeRepoBadgeColor(value)
    case 'resolveRepoBadgeColor':
      return resolveRepoBadgeColor(value)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
