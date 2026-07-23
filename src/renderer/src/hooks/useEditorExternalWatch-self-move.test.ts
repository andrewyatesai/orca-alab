import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type * as EditorAutosaveModule from '@/components/editor/editor-autosave'
import type { FsChangedPayload } from '../../../shared/types'

vi.mock('@/store', () => ({
  useAppStore: {
    getState: vi.fn()
  }
}))
vi.mock('@/components/editor/editor-autosave', async (importOriginal) => {
  const actual = await importOriginal<typeof EditorAutosaveModule>()
  return {
    ...actual,
    notifyEditorExternalFileChange: vi.fn(),
    getOpenFilesForExternalFileChange: vi.fn(() => [])
  }
})

import { createExternalWatchEventHandler } from './useEditorExternalWatch'
import { useAppStore } from '@/store'
import { getOpenFilesForExternalFileChange } from '@/components/editor/editor-autosave'
import {
  __clearSelfMoveRegistryForTests,
  recordSelfMove
} from '@/components/editor/editor-path-move-inflight'

// The move re-homes the tab to the destination path, then the watcher echoes a
// create/update there; without the self-move stamp that echo raises a spurious
// changed-on-disk banner on the dirty tab (#9506).
const DEST_PATH = '/repo/renamed.md'

const setExternalMutation = vi.fn()

const findTarget = (worktreePath: string, runtimeEnvironmentId: string | null) =>
  worktreePath === '/repo'
    ? {
        worktreeId: 'wt-1',
        worktreePath: '/repo',
        connectionId: undefined,
        runtimeEnvironmentId
      }
    : undefined

const dirtyDestTab = {
  id: 'file-1',
  worktreeId: 'wt-1',
  worktreePath: '/repo',
  filePath: DEST_PATH,
  relativePath: 'renamed.md',
  mode: 'edit' as const,
  isDirty: true
}

function payload(events: FsChangedPayload['events']): FsChangedPayload {
  return { worktreePath: '/repo', events }
}

describe('useEditorExternalWatch self-move suppression (#9506)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.useFakeTimers()
    __clearSelfMoveRegistryForTests()
    vi.mocked(useAppStore.getState).mockReturnValue({
      openFiles: [dirtyDestTab],
      setExternalMutation
    } as never)
    vi.mocked(getOpenFilesForExternalFileChange).mockReturnValue([dirtyDestTab] as never)
  })
  afterEach(() => {
    vi.useRealTimers()
    __clearSelfMoveRegistryForTests()
  })

  it('does not flag the re-homed dirty tab changed-on-disk for the move echo', () => {
    const { handleFsChanged, dispose } = createExternalWatchEventHandler(findTarget)

    recordSelfMove('target', DEST_PATH)
    handleFsChanged(payload([{ kind: 'update', absolutePath: DEST_PATH }]))
    vi.advanceTimersByTime(200)

    expect(setExternalMutation).not.toHaveBeenCalledWith('file-1', 'changed')
    dispose()
  })

  it('still flags a genuine external write once the self-move stamp is gone (control)', () => {
    const { handleFsChanged, dispose } = createExternalWatchEventHandler(findTarget)

    // No self-move stamp: a real external write must surface the banner.
    handleFsChanged(payload([{ kind: 'update', absolutePath: DEST_PATH }]))
    vi.advanceTimersByTime(200)

    expect(setExternalMutation).toHaveBeenCalledWith('file-1', 'changed')
    dispose()
  })

  it('does not tombstone the source path for the move echo delete', () => {
    const sourceTab = {
      ...dirtyDestTab,
      filePath: '/repo/original.md',
      relativePath: 'original.md',
      isDirty: false
    }
    vi.mocked(useAppStore.getState).mockReturnValue({
      openFiles: [sourceTab],
      setExternalMutation
    } as never)
    const { handleFsChanged, dispose } = createExternalWatchEventHandler(findTarget)

    recordSelfMove('source', '/repo/original.md')
    handleFsChanged(payload([{ kind: 'delete', absolutePath: '/repo/original.md' }]))
    vi.advanceTimersByTime(200)

    expect(setExternalMutation).not.toHaveBeenCalledWith('file-1', 'deleted')
    dispose()
  })
})
