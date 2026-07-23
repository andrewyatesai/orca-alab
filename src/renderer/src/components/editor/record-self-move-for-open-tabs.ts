import { useAppStore } from '@/store'
import { isPathInsideOrEqual } from '@/lib/remap-open-editor-tabs-for-path-change'
import {
  clearSelfMove,
  recordSelfMove,
  SELF_MOVE_REMOTE_TTL_MS,
  type SelfMoveRole
} from './editor-path-move-inflight'

export type SelfMoveRetraction = () => void

type StampedEntry = {
  role: SelfMoveRole
  absolutePath: string
  runtimeEnvironmentId: string | null
}

// Stamps every open tab's source+target path so the fs watcher recognizes the
// in-app move's own delete(old)+create(new) echo. Call BEFORE the on-disk
// rename so the stamp can't lose the race with a fast local echo. Returns a
// retraction to call if the rename fails — no echo will come, and a live stamp
// must not swallow an unrelated write to those paths within the TTL (#9506).
export function recordSelfMoveForOpenTabs(args: {
  fromPath: string
  toPath: string
  connectionId?: string | undefined
}): SelfMoveRetraction {
  const { fromPath, toPath, connectionId } = args
  const openFiles = useAppStore.getState().openFiles
  const stamped: StampedEntry[] = []
  for (const file of openFiles) {
    if (!isPathInsideOrEqual(fromPath, file.filePath)) {
      continue
    }
    const targetPath = toPath + file.filePath.slice(fromPath.length)
    const runtimeEnvironmentId = file.runtimeEnvironmentId?.trim() || null
    // Why: a tab is remote when it has a runtime owner OR an SSH worktree
    // connection (an SSH tab can carry a null runtime owner); remote echoes lag.
    const ttlMs = runtimeEnvironmentId || connectionId ? SELF_MOVE_REMOTE_TTL_MS : undefined
    recordSelfMove('source', file.filePath, runtimeEnvironmentId, ttlMs)
    recordSelfMove('target', targetPath, runtimeEnvironmentId, ttlMs)
    stamped.push({ role: 'source', absolutePath: file.filePath, runtimeEnvironmentId })
    stamped.push({ role: 'target', absolutePath: targetPath, runtimeEnvironmentId })
  }
  return () => {
    for (const entry of stamped) {
      clearSelfMove(entry.role, entry.absolutePath, entry.runtimeEnvironmentId)
    }
  }
}
