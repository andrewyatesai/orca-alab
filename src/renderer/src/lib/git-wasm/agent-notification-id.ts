// Agent OS-notification id derivation, driven by the Rust orca-core via the
// orca-git wasm (the shared TS impl was deleted). Both consumers already guard
// null, so a null during the ~tens-of-ms wasm boot window just transiently
// misses a dedupe/dismiss id — never a crash or a corrupted notification id.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { BuildAgentNotificationIdArgs } from '../../../../shared/agent-notification-id'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) return null
  return JSON.parse(orcaDispatch('agent-notification-id', fn, JSON.stringify(input ?? null)))
}

export function buildAgentNotificationId(args: BuildAgentNotificationIdArgs): string | null {
  // Rust null (invalid metadata) and the pre-ready null both collapse to the
  // consumers' documented "no id" case.
  const r = op('buildAgentNotificationId', {
    worktreeId: args.worktreeId ?? null,
    paneKey: args.paneKey ?? null,
    stateStartedAt: args.stateStartedAt ?? null
  }) as string | null
  return r ?? null
}
