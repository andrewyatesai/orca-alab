// TS dispatch for the worktree-id parity module: maps the shared vector
// function names to the real `src/shared/worktree-id.ts` exports so the harness
// compares the live TS reference against the Rust port. Every case carries the
// single `worktreeId` string argument directly as the input.

import {
  getRepoIdFromWorktreeId,
  getWorktreePathBasenameFromId,
  splitWorktreeId,
  splitWorktreeIdForFilesystem
} from '../../../src/shared/worktree-id'

export function dispatch(fn: string, input: unknown): unknown {
  const worktreeId = input as string
  switch (fn) {
    case 'getRepoIdFromWorktreeId':
      return getRepoIdFromWorktreeId(worktreeId)
    case 'splitWorktreeId':
      return splitWorktreeId(worktreeId)
    case 'splitWorktreeIdForFilesystem':
      return splitWorktreeIdForFilesystem(worktreeId)
    case 'getWorktreePathBasenameFromId':
      return getWorktreePathBasenameFromId(worktreeId)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
