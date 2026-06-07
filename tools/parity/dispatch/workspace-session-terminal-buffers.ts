// TS dispatch for the workspace-session-terminal-buffers parity module: maps the
// shared vector function names to the real
// `src/shared/workspace-session-terminal-buffers.ts` exports so the harness
// compares the live TS reference against the Rust port.

import {
  pruneLocalTerminalScrollbackBuffers,
  shouldPreserveTerminalScrollbackBuffers,
  type RepoConnection
} from '../../../src/shared/workspace-session-terminal-buffers'
import type { WorkspaceSessionState } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'shouldPreserveTerminalScrollbackBuffers': {
      const { worktreeId, repos } = input as {
        worktreeId?: string
        repos: RepoConnection[]
      }
      return shouldPreserveTerminalScrollbackBuffers(worktreeId, repos)
    }
    case 'pruneLocalTerminalScrollbackBuffers': {
      const { session, repos } = input as {
        session: WorkspaceSessionState
        repos: RepoConnection[]
      }
      return pruneLocalTerminalScrollbackBuffers(session, repos)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
