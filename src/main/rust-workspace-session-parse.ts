// Main-process workspace-session parser, driven by the Rust orca-config core via
// napi (the shared TS zod schema was deleted). Same parse/repair persistence.ts
// relied on — one source of truth with the parity-proven Rust port.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { ParsedWorkspaceSession } from '../shared/workspace-session-schema'

export function parseWorkspaceSession(raw: unknown): ParsedWorkspaceSession {
  // Coerce undefined -> null so JSON.stringify yields a string the napi boundary
  // parses back to a non-object the schema rejects (matching zod on undefined).
  return JSON.parse(
    requireRustGitBinding().parseWorkspaceSession(JSON.stringify(raw ?? null))
  ) as ParsedWorkspaceSession
}
