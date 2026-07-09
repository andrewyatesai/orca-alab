// The agent OS-notification id derivation (buildAgentNotificationId) moved to
// the Rust orca-core: the renderer drives it through the orca-git wasm
// (src/renderer/src/lib/git-wasm/agent-notification-id.ts). This shared module
// keeps only the arg type those boundaries and the parity dispatch reference.

export type BuildAgentNotificationIdArgs = {
  worktreeId?: string | null
  paneKey?: string | null
  stateStartedAt?: number | null
}
