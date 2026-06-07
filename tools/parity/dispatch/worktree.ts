// TS dispatch for the worktree parity module: maps the shared vector function
// names to the real `src/main/git/worktree.ts` exports so the harness compares
// the live TS reference against the Rust port. Only the pure porcelain parser
// is covered; the rest of the module is git IO/orchestration.

import { parseWorktreeList } from '../../../src/main/git/worktree'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseWorktreeList': {
      const { output, nulDelimited } = input as { output: string; nulDelimited?: boolean }
      return parseWorktreeList(output, { nulDelimited })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
