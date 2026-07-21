// Env Orca's own process can carry that must never leak into spawned
// terminals: it describes Orca's runtime and ancestry, not the pane the user
// asked for. All three spawn-env builders (local provider, daemon subprocess,
// SSH relay) clone the host env through here; the daemon host additionally
// forwards these keys via envToDelete because the Rust daemon merges its own
// inherited env engine-side.

// Why: a `claude` CLI exports these to its children. If Orca (or its terminal
// daemon) starts under one, a claude launched in a tab chains itself to the
// dead ancestor session and silently skips persisting its transcript (#9155).
export const CLAUDE_CODE_CHILD_SESSION_ENV_KEYS = [
  'CLAUDECODE',
  'CLAUDE_CODE_CHILD_SESSION',
  'CLAUDE_CODE_SESSION_ID',
  'CLAUDE_CODE_EXECPATH',
  'CLAUDE_CODE_ENTRYPOINT'
] as const

// Why: NODE_ENV is Orca's build mode (`development` in dev builds), not the
// user's; a leaked value flips tools that key off it — `next build` crashes
// prerender and Vitest skips its `test` default (upstream PR 9058).
export const INHERITED_ONLY_SPAWN_ENV_KEYS = [
  ...CLAUDE_CODE_CHILD_SESSION_ENV_KEYS,
  'NODE_ENV'
] as const

/** Clone of the host process env with inherited-only keys removed. An explicit
 *  renderer/session env merged over the clone still wins for every key. */
export function cloneInheritedSpawnEnv(
  processEnv: NodeJS.ProcessEnv = process.env
): NodeJS.ProcessEnv {
  const env = { ...processEnv }
  for (const key of INHERITED_ONLY_SPAWN_ENV_KEYS) {
    delete env[key]
  }
  return env
}
