import { toast } from 'sonner'
import { useAppStore } from '@/store'
import { basename, dirname, joinPath } from '@/lib/path'
import { getConnectionId } from '@/lib/connection-context'
import { requestEditorSaveQuiesce } from '@/components/editor/editor-autosave'
import { commitFileExplorerOp } from '@/components/right-sidebar/fileExplorerUndoRedo'
import { renameRuntimePath } from '@/runtime/runtime-file-client'
import { remapOpenEditorTabsForPathChange } from '@/lib/remap-open-editor-tabs-for-path-change'
import { recordSelfMoveForOpenTabs } from '@/components/editor/record-self-move-for-open-tabs'

/**
 * Electron's ipcRenderer.invoke wraps errors as:
 *   "Error invoking remote method 'channel': Error: actual message"
 * Strip the wrapper so users see only the meaningful part.
 */
export function extractIpcErrorMessage(err: unknown, fallback: string): string {
  if (!(err instanceof Error)) {
    return fallback
  }
  const match = err.message.match(/Error invoking remote method '[^']*': (?:Error: )?(.+)/)
  return match ? match[1] : err.message
}

type RenameFileArgs = {
  oldPath: string
  /** just the new filename (no directory) */
  newName: string
  worktreeId: string
  worktreePath: string
  /** refresh the parent directory in the explorer tree, if caller tracks one */
  refreshDir?: (dirPath: string) => Promise<void>
}

/**
 * Rename a file or directory on disk. Handles:
 *   - no-op when the name is unchanged
 *   - quiescing any in-flight autosave on open tabs under `oldPath`
 *     (so a trailing write can't recreate the old path post-rename)
 *   - remapping every affected open editor tab to the new path
 *   - committing an undo/redo pair via the file-explorer undo stack
 *   - unwrapped toast on IPC failure
 *
 * Used by the file-explorer inline rename and by double-click-rename
 * from an editor tab. Both entry points should go through here so
 * the tab-remap + quiesce behavior stays consistent.
 */
export async function renameFileOnDisk(args: RenameFileArgs): Promise<void> {
  const { oldPath, newName, worktreeId, worktreePath, refreshDir } = args
  const trimmed = newName.trim()
  if (!trimmed) {
    return
  }
  const existingName = basename(oldPath)
  if (trimmed === existingName) {
    return
  }
  const parentDir = dirname(oldPath)
  const newPath = joinPath(parentDir, trimmed)
  const connectionId = getConnectionId(worktreeId) ?? undefined

  // Let any in-flight autosave under `oldPath` finish first — a trailing
  // write to the old path after rename would silently recreate it.
  const state = useAppStore.getState()
  const filesToQuiesce = state.openFiles.filter(
    (file) =>
      file.filePath === oldPath ||
      file.filePath.startsWith(`${oldPath}/`) ||
      file.filePath.startsWith(`${oldPath}\\`)
  )
  await Promise.all(filesToQuiesce.map((file) => requestEditorSaveQuiesce({ fileId: file.id })))
  const fileContext = {
    settings: state.settings,
    worktreeId,
    worktreePath,
    connectionId
  }

  // Why: stamp the move before the on-disk rename so the watcher's own
  // delete(old)+create(new) echo isn't mistaken for an external write on the
  // re-homed dirty tab (#9506); retract if the rename never happens.
  const retractSelfMove = recordSelfMoveForOpenTabs({
    fromPath: oldPath,
    toPath: newPath,
    connectionId
  })
  try {
    await renameRuntimePath(fileContext, oldPath, newPath)
    // Re-stamp after the rename resolves: a slow SSH/runtime rename can outlive
    // the pre-rename TTL, so restart the window from when the file actually moved.
    recordSelfMoveForOpenTabs({ fromPath: oldPath, toPath: newPath, connectionId })
    remapOpenEditorTabsForPathChange({ fromPath: oldPath, toPath: newPath, worktreePath })
    commitFileExplorerOp({
      undo: async () => {
        recordSelfMoveForOpenTabs({ fromPath: newPath, toPath: oldPath, connectionId })
        await renameRuntimePath(fileContext, newPath, oldPath)
        recordSelfMoveForOpenTabs({ fromPath: newPath, toPath: oldPath, connectionId })
        if (refreshDir) {
          await refreshDir(parentDir)
        }
        remapOpenEditorTabsForPathChange({ fromPath: newPath, toPath: oldPath, worktreePath })
      },
      redo: async () => {
        recordSelfMoveForOpenTabs({ fromPath: oldPath, toPath: newPath, connectionId })
        await renameRuntimePath(fileContext, oldPath, newPath)
        recordSelfMoveForOpenTabs({ fromPath: oldPath, toPath: newPath, connectionId })
        if (refreshDir) {
          await refreshDir(parentDir)
        }
        remapOpenEditorTabsForPathChange({ fromPath: oldPath, toPath: newPath, worktreePath })
      }
    })
  } catch (err) {
    retractSelfMove()
    toast.error(extractIpcErrorMessage(err, `Failed to rename '${existingName}'.`))
  }
  if (refreshDir) {
    await refreshDir(parentDir)
  }
}
