// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render } from '@testing-library/react'
import { create } from 'zustand'
import type { OpenFile } from '@/store/slices/editor'

import { registerLoadedMonaco, type LoadedMonaco } from '@/lib/loaded-monaco'

const disposeSpy = vi.fn()

// Why: the hook reads Monaco from the loaded-instance registry (never a static
// import — chunk budget), so the test registers the fake the same way
// monaco-setup registers the real one.
registerLoadedMonaco({
  Uri: { parse: (value: string) => value },
  editor: {
    getModel: () => ({ dispose: disposeSpy }),
    getModels: () => []
  }
} as unknown as LoadedMonaco)

type TestState = { openFiles: OpenFile[] }
const testStore = create<TestState>(() => ({ openFiles: [] }))

vi.mock('@/store', () => ({
  useAppStore: <T,>(selector: (s: TestState) => T): T => testStore(selector)
}))

import ClosedEditorTabCleanupGate from './ClosedEditorTabCleanupGate'

function makeOpenFile(id: string): OpenFile {
  return {
    id,
    filePath: id,
    relativePath: id,
    worktreeId: 'wt-1',
    language: 'markdown',
    isDirty: false,
    mode: 'edit'
  } as OpenFile
}

describe('ClosedEditorTabCleanupGate', () => {
  beforeEach(() => {
    disposeSpy.mockReset()
    testStore.setState({ openFiles: [] })
  })

  it('disposes a closed tab model with no EditorPanel mounted (the #1476 leak)', () => {
    // The gate is the only mounted editor surface — mirroring "last tab closed",
    // where EditorPanel has already unmounted and can no longer observe closes.
    render(<ClosedEditorTabCleanupGate />)

    act(() => {
      testStore.setState({ openFiles: [makeOpenFile('/repo/a.md')] })
    })
    expect(disposeSpy).not.toHaveBeenCalled()

    act(() => {
      testStore.setState({ openFiles: [] })
    })
    expect(disposeSpy).toHaveBeenCalledTimes(1)
  })
})
