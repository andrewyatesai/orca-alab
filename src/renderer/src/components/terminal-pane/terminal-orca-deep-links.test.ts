import { beforeEach, describe, expect, it, vi } from 'vitest'
import { routeTerminalOrcaDeepLink } from './terminal-orca-deep-links'
import { focusRendererTerminalHandle, focusRuntimeTerminalHandle } from './terminal-handle-links'
import {
  showDeepLinkTerminalGoneToast,
  showDeepLinkUnrecognizedToast
} from '@/lib/deep-link-ui-notices'
import { dispatchDeepLinkInRenderer } from '@/lib/deep-link-renderer-dispatch'

vi.mock('./terminal-handle-links', () => ({
  focusRendererTerminalHandle: vi.fn(() => true),
  focusRuntimeTerminalHandle: vi.fn(() => Promise.resolve())
}))

vi.mock('@/lib/deep-link-ui-notices', () => ({
  showDeepLinkTerminalGoneToast: vi.fn(),
  showDeepLinkUnrecognizedToast: vi.fn()
}))

vi.mock('@/lib/deep-link-renderer-dispatch', () => ({
  dispatchDeepLinkInRenderer: vi.fn()
}))

const context = { worktreeId: 'repo::wt-origin', runtimeEnvironmentId: null }

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(focusRendererTerminalHandle).mockReturnValue(true)
  vi.mocked(focusRuntimeTerminalHandle).mockResolvedValue()
})

describe('routeTerminalOrcaDeepLink', () => {
  it('focus prefers renderer-local handle target', () => {
    const consumed = routeTerminalOrcaDeepLink('orca://focus/term_abc', context)

    expect(consumed).toBe(true)
    expect(focusRendererTerminalHandle).toHaveBeenCalledWith('term_abc', null)
    expect(focusRuntimeTerminalHandle).not.toHaveBeenCalled()
  })

  it('falls back to terminal.focus rpc for unknown local handle', () => {
    vi.mocked(focusRendererTerminalHandle).mockReturnValue(false)

    routeTerminalOrcaDeepLink('orca://focus/term_abc', {
      worktreeId: 'repo::wt-origin',
      runtimeEnvironmentId: 'ssh-env-1'
    })

    expect(focusRendererTerminalHandle).toHaveBeenCalledWith('term_abc', 'ssh-env-1')
    expect(focusRuntimeTerminalHandle).toHaveBeenCalledWith('term_abc', 'ssh-env-1')
  })

  it('toasts terminal-gone when the rpc fallback rejects (stale handle)', async () => {
    vi.mocked(focusRendererTerminalHandle).mockReturnValue(false)
    vi.mocked(focusRuntimeTerminalHandle).mockRejectedValue(new Error('terminal_handle_stale'))

    routeTerminalOrcaDeepLink('orca://focus/term_gone', context)
    await flushMicrotasks()

    expect(showDeepLinkTerminalGoneToast).toHaveBeenCalledTimes(1)
  })

  it('malformed orca link toasts and is still consumed', () => {
    const consumed = routeTerminalOrcaDeepLink('orca://unknown-host/whatever', context)

    expect(consumed).toBe(true)
    expect(showDeepLinkUnrecognizedToast).toHaveBeenCalledTimes(1)
    expect(focusRendererTerminalHandle).not.toHaveBeenCalled()
  })

  it('worktree/pair/run dispatch with the terminal-stamped origin (never from the URL)', () => {
    routeTerminalOrcaDeepLink('orca://worktree/repo%3A%3Apath', context)
    routeTerminalOrcaDeepLink('orca://pair?code=YWJj', context)
    routeTerminalOrcaDeepLink('orca://run?worktree=r%3A%3Ap&cmd=ls', context)

    const terminalOrigin = { source: 'terminal', worktreeId: 'repo::wt-origin' }
    expect(vi.mocked(dispatchDeepLinkInRenderer).mock.calls).toEqual([
      [{ kind: 'worktree', worktreeId: 'repo::path' }, terminalOrigin],
      [{ kind: 'pair', code: 'YWJj' }, terminalOrigin],
      [{ kind: 'run', worktreeId: 'r::p', command: 'ls' }, terminalOrigin]
    ])
    expect(focusRendererTerminalHandle).not.toHaveBeenCalled()
  })

  it('run link opens consent with terminal origin (routes through renderer dispatch)', () => {
    const consumed = routeTerminalOrcaDeepLink('orca://run?worktree=r%3A%3Ap&cmd=echo%20hi', {
      worktreeId: 'repo::clicked-in',
      runtimeEnvironmentId: null
    })

    expect(consumed).toBe(true)
    expect(dispatchDeepLinkInRenderer).toHaveBeenCalledWith(
      { kind: 'run', worktreeId: 'r::p', command: 'echo hi' },
      { source: 'terminal', worktreeId: 'repo::clicked-in' }
    )
  })
})
