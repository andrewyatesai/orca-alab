// TS dispatch for the gitlab-projects parity module: maps the shared vector
// function names to the real `src/shared/gitlab-projects.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { computeNextGitLabRecents } from '../../../src/shared/gitlab-projects'
import type { GitLabProjectSettings } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'computeNextGitLabRecents': {
      // nowIso is the persisted ISO string; rehydrate the Date the TS signature
      // expects so the function's own toISOString() round-trips it identically.
      const { existing, host, path, nowIso, max } = input as {
        existing: GitLabProjectSettings['recent']
        host: string
        path: string
        nowIso: string
        max: number
      }
      return computeNextGitLabRecents(existing, host, path, new Date(nowIso), max)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
