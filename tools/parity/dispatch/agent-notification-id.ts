// TS dispatch for the agent-notification-id parity module: maps the shared
// vector function names to the real `src/shared/agent-notification-id.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildAgentNotificationId,
  type BuildAgentNotificationIdArgs
} from '../../../src/shared/agent-notification-id'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildAgentNotificationId': {
      const { worktreeId, paneKey, stateStartedAt } = input as BuildAgentNotificationIdArgs
      return buildAgentNotificationId({ worktreeId, paneKey, stateStartedAt })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
