// TS dispatch for the workspace-name parity module: maps the shared vector
// function names to the real `src/shared/workspace-name.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { slugifyForWorkspaceName } from '../../../src/shared/workspace-name'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'slugifyForWorkspaceName':
      return slugifyForWorkspaceName(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
