import { beforeEach, describe, expect, it, vi } from 'vitest'
import { OSC52_CLIPBOARD_SETTING_ID } from './osc52-clipboard-setting-anchor'

const { toastErrorMock, toastSuccessMock, storeMock } = vi.hoisted(() => ({
  toastErrorMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  storeMock: {
    setSettingsSearchQuery: vi.fn(),
    openSettingsTarget: vi.fn(),
    openSettingsPage: vi.fn()
  }
}))

vi.mock('sonner', () => ({
  toast: {
    error: toastErrorMock,
    success: toastSuccessMock
  }
}))

vi.mock('@/store', () => ({
  useAppStore: {
    getState: () => storeMock
  }
}))

import {
  copyTerminalSelectionThenClear,
  copyTerminalTextVerified,
  reportTerminalCopyOutcome,
  resetTerminalCopyOutcomeLatchesForTest
} from './terminal-copy-outcome'

function stubWriteClipboardText(
  impl: (text: string) => Promise<boolean>
): ReturnType<typeof vi.fn> {
  const write = vi.fn(impl)
  vi.stubGlobal('window', { api: { ui: { writeClipboardText: write } } })
  return write
}

beforeEach(() => {
  resetTerminalCopyOutcomeLatchesForTest()
  toastErrorMock.mockReset()
  toastSuccessMock.mockReset()
  storeMock.setSettingsSearchQuery.mockReset()
  storeMock.openSettingsTarget.mockReset()
  storeMock.openSettingsPage.mockReset()
  vi.unstubAllGlobals()
})

describe('reportTerminalCopyOutcome', () => {
  it('shows one failure toast per source per session (dedup + copy-on-select rate limit)', () => {
    reportTerminalCopyOutcome(false, 'copy-on-select')
    reportTerminalCopyOutcome(false, 'copy-on-select')
    reportTerminalCopyOutcome(false, 'copy-on-select')
    expect(toastErrorMock).toHaveBeenCalledTimes(1)

    // A different source gets its own (single) toast.
    reportTerminalCopyOutcome(false, 'shortcut')
    reportTerminalCopyOutcome(false, 'shortcut')
    expect(toastErrorMock).toHaveBeenCalledTimes(2)
  })

  it('host-initiated success stays silent', () => {
    reportTerminalCopyOutcome(true, 'shortcut')
    reportTerminalCopyOutcome(true, 'context-menu')
    reportTerminalCopyOutcome(true, 'copy-on-select')
    expect(toastSuccessMock).not.toHaveBeenCalled()
    expect(toastErrorMock).not.toHaveBeenCalled()
  })

  it('an osc52 failure toast carries the Open Setting deep link', () => {
    reportTerminalCopyOutcome(false, 'osc52')
    expect(toastErrorMock).toHaveBeenCalledTimes(1)
    const options = toastErrorMock.mock.calls[0]?.[1]
    expect(options?.action?.label).toBe('Open Setting')

    options.action.onClick()
    expect(storeMock.openSettingsTarget).toHaveBeenCalledWith({
      pane: 'terminal',
      repoId: null,
      sectionId: OSC52_CLIPBOARD_SETTING_ID
    })
    expect(storeMock.openSettingsPage).toHaveBeenCalled()
  })

  it('non-osc52 failure toasts carry no action', () => {
    reportTerminalCopyOutcome(false, 'context-menu')
    expect(toastErrorMock.mock.calls[0]?.[1]?.action).toBeUndefined()
  })

  it('only the FIRST osc52 success shows the passive toast; later successes stay silent', () => {
    reportTerminalCopyOutcome(true, 'osc52')
    reportTerminalCopyOutcome(true, 'osc52')
    expect(toastSuccessMock).toHaveBeenCalledTimes(1)
    expect(toastSuccessMock.mock.calls[0]?.[0]).toContain('copied')
  })
})

describe('copyTerminalTextVerified', () => {
  it('resolves true and stays silent on a verified write', async () => {
    const write = stubWriteClipboardText(() => Promise.resolve(true))
    await expect(copyTerminalTextVerified('text', 'shortcut')).resolves.toBe(true)
    expect(write).toHaveBeenCalledWith('text')
    expect(toastErrorMock).not.toHaveBeenCalled()
  })

  it('surfaces an unverified write (resolved false) as a failure toast', async () => {
    stubWriteClipboardText(() => Promise.resolve(false))
    await expect(copyTerminalTextVerified('text', 'shortcut')).resolves.toBe(false)
    expect(toastErrorMock).toHaveBeenCalledTimes(1)
  })

  it('surfaces a rejected IPC write as a failure toast', async () => {
    stubWriteClipboardText(() => Promise.reject(new Error('ipc down')))
    await expect(copyTerminalTextVerified('text', 'context-menu')).resolves.toBe(false)
    expect(toastErrorMock).toHaveBeenCalledTimes(1)
  })

  it('stays silent when the IPC surface is absent (hidden/e2e windows)', async () => {
    vi.stubGlobal('window', {})
    await expect(copyTerminalTextVerified('text', 'copy-on-select')).resolves.toBe(false)
    expect(toastErrorMock).not.toHaveBeenCalled()
  })
})

describe('copyTerminalSelectionThenClear (right-click copy)', () => {
  it('clears the selection only AFTER the write verified', async () => {
    let resolveWrite: (ok: boolean) => void = () => {}
    stubWriteClipboardText(() => new Promise<boolean>((resolve) => (resolveWrite = resolve)))
    const clearSelection = vi.fn()

    const pending = copyTerminalSelectionThenClear('sel', clearSelection)
    expect(clearSelection, 'must not clear before the write resolves').not.toHaveBeenCalled()

    resolveWrite(true)
    await expect(pending).resolves.toBe(true)
    expect(clearSelection).toHaveBeenCalledTimes(1)
  })

  it('never clears the selection when the write does not verify', async () => {
    stubWriteClipboardText(() => Promise.resolve(false))
    const clearSelection = vi.fn()
    await expect(copyTerminalSelectionThenClear('sel', clearSelection)).resolves.toBe(false)
    expect(clearSelection).not.toHaveBeenCalled()
    expect(toastErrorMock).toHaveBeenCalledTimes(1)
  })
})
