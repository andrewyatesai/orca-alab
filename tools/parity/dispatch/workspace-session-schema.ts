// TS dispatch for the workspace-session-schema parity module: maps the vector
// function name to the real `src/shared/workspace-session-schema.ts` export
// (the parse/repair entry `src/main/persistence.ts` calls) so the harness
// compares the live TS reference against the Rust port
// (`orca-config::workspace_session_schema`).

import { parseWorkspaceSession } from '../../../src/shared/workspace-session-schema'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseWorkspaceSession':
      // The vector input is the raw session JSON value; the return is the
      // discriminated union ({ok:true,value} | {ok:false,error}) as plain JSON.
      return parseWorkspaceSession(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
