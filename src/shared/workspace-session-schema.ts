// The workspace-session parse/repair (parseWorkspaceSession + its zod schema)
// moved to the Rust orca-config core: the main process drives it through the
// napi addon (src/main/rust-workspace-session-parse.ts). This shared module
// keeps only the result type that boundary and the parity dispatch reference.
//
// Why the schema existed: session JSON written by older builds is read back by
// newer ones, so validating at the read boundary collapses any garbage (a
// field-type flip, a truncated write) to "use defaults" — never letting it
// reach React or throw into main.
import type { WorkspaceSessionState } from './types'

/** Validate raw JSON as a WorkspaceSessionState. Returns a discriminated union
 *  so callers can fall back to defaults on failure without a try/catch. */
export type ParsedWorkspaceSession =
  | { ok: true; value: WorkspaceSessionState }
  | { ok: false; error: string }
