// TS dispatch for the project-groups parity module: maps the shared vector
// function names to the real `src/shared/project-groups.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  getNextProjectGroupOrder,
  normalizeProjectGroupName
} from '../../../src/shared/project-groups'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeProjectGroupName': {
      // Absent `fallback` is passed as `undefined`, so the TS default param applies.
      const { name, fallback } = input as { name: string; fallback?: string }
      return normalizeProjectGroupName(name, fallback)
    }
    case 'getNextProjectGroupOrder': {
      const { repos, groupId } = input as { repos: unknown; groupId: string | null }
      return getNextProjectGroupOrder(repos as never, groupId)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
