// @vitest-environment happy-dom

// #9279 A2/A3 action behaviors: what the menu-open capture reads, what each
// item copies/opens/reveals, and the SSH/remote gating on Reveal.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ManagedPane } from '@/lib/pane-manager/pane-manager'
import {
  captureTerminalMenuTargets,
  copyTerminalMenuLastCommandOutput,
  copyTerminalMenuLinkTarget,
  isTerminalMenuRevealAvailable,
  openTerminalMenuLinkTarget,
  revealTerminalMenuFileTarget
} from './terminal-context-menu-link-target'

const toast = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }))
const copyVerified = vi.hoisted(() => vi.fn<(text: string, source: string) => Promise<boolean>>())
const connectionId = vi.hoisted(() => ({ value: null as string | null }))
const runtimeEnvironmentId = vi.hoisted(() => ({ value: null as string | null }))

vi.mock('sonner', () => ({ toast }))
vi.mock('@/i18n/i18n', () => ({ translate: (_key: string, fallback: string) => fallback }))
vi.mock('./terminal-copy-outcome', () => ({
  copyTerminalTextVerified: copyVerified
}))
vi.mock('@/lib/connection-context', () => ({
  getConnectionId: () => connectionId.value
}))
vi.mock('@/lib/worktree-runtime-owner', () => ({
  getRuntimeEnvironmentIdForWorktree: () => runtimeEnvironmentId.value
}))
vi.mock('@/store', () => ({ useAppStore: { getState: () => ({}) } }))

type PaneDouble = {
  pane: ManagedPane
  focus: ReturnType<typeof vi.fn>
  controller: Record<string, unknown>
}

function makePane(controller: Record<string, unknown> = {}): PaneDouble {
  const focus = vi.fn()
  const pane = {
    atermController: controller,
    terminal: { focus, getSelection: () => '' }
  } as unknown as ManagedPane
  return { pane, focus, controller }
}

beforeEach(() => {
  copyVerified.mockResolvedValue(true)
  connectionId.value = null
  runtimeEnvironmentId.value = null
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('captureTerminalMenuTargets', () => {
  it('reads the selection synchronously and applies link/output when they resolve', async () => {
    const { pane } = makePane({
      selectionText: () => 'picked at open',
      contextLinkTargetAt: () => Promise.resolve({ kind: 'url', url: 'https://a/' }),
      lastCommandOutputAsync: () => Promise.resolve({ status: 'ok', text: 'out', exitCode: 0 })
    })
    const applied: unknown[] = []
    const captured = captureTerminalMenuTargets(pane, { clientX: 10, clientY: 20 }, (partial) =>
      applied.push(partial)
    )
    // Synchronous: the selection is captured at the open EVENT, before any await.
    expect(captured.selectionText).toBe('picked at open')
    await Promise.resolve()
    await Promise.resolve()
    expect(applied).toContainEqual({ linkTarget: { kind: 'url', url: 'https://a/' } })
    expect(applied).toContainEqual({ hasCommandOutput: true })
  })

  it('degrades to no selection / no targets when the controller seams are absent', async () => {
    const { pane } = makePane({ selectionText: () => '' })
    const apply = vi.fn()
    const captured = captureTerminalMenuTargets(pane, { clientX: 0, clientY: 0 }, apply)
    expect(captured.selectionText).toBe('')
    await Promise.resolve()
    await Promise.resolve()
    expect(apply).not.toHaveBeenCalled() // feature-detected seams missing → nothing applies
  })
})

describe('isTerminalMenuRevealAvailable', () => {
  it('hides Reveal for SSH connections and remote runtimes, allows local panes', () => {
    expect(isTerminalMenuRevealAvailable('wt-1')).toBe(true)
    connectionId.value = 'ssh-conn'
    expect(isTerminalMenuRevealAvailable('wt-1')).toBe(false)
    connectionId.value = null
    runtimeEnvironmentId.value = 'runtime-9'
    expect(isTerminalMenuRevealAvailable('wt-1')).toBe(false)
  })
})

describe('copyTerminalMenuLinkTarget', () => {
  it('Copy Path copies the raw matched span for file targets', async () => {
    const { pane, focus } = makePane()
    await copyTerminalMenuLinkTarget(pane, { kind: 'file', rawPathText: './src/app.ts:42' })
    expect(copyVerified).toHaveBeenCalledWith('./src/app.ts:42', 'context-menu')
    expect(toast.success).toHaveBeenCalledWith('Path copied')
    expect(focus).toHaveBeenCalled()
  })

  it('copies the resolved url for links and the link text for provider targets', async () => {
    const { pane } = makePane()
    await copyTerminalMenuLinkTarget(pane, { kind: 'url', url: 'https://example.test/' })
    expect(copyVerified).toHaveBeenCalledWith('https://example.test/', 'context-menu')
    expect(toast.success).toHaveBeenCalledWith('Link copied')

    await copyTerminalMenuLinkTarget(pane, {
      kind: 'provider',
      text: 'term_abc',
      activate: vi.fn()
    })
    expect(copyVerified).toHaveBeenCalledWith('term_abc', 'context-menu')
  })

  it('stays silent (no toast) when the verified clipboard write fails', async () => {
    copyVerified.mockResolvedValue(false)
    const { pane } = makePane()
    await copyTerminalMenuLinkTarget(pane, { kind: 'url', url: 'https://x/' })
    expect(toast.success).not.toHaveBeenCalled()
  })
})

describe('openTerminalMenuLinkTarget', () => {
  it('routes through the controller opener and refocuses the terminal', () => {
    const openContextLinkTarget = vi.fn()
    const { pane, focus } = makePane({ openContextLinkTarget })
    openTerminalMenuLinkTarget(pane, { kind: 'osc8', url: 'file:///x' })
    expect(openContextLinkTarget).toHaveBeenCalledWith(
      { kind: 'osc8', url: 'file:///x' },
      { openWithSystemDefault: false }
    )
    expect(focus).toHaveBeenCalled()
  })
})

describe('revealTerminalMenuFileTarget', () => {
  const stubShell = (result: { ok: boolean }): ReturnType<typeof vi.fn> => {
    const openInFileManager = vi.fn(() => Promise.resolve(result))
    ;(window as unknown as { api: unknown }).api = { shell: { openInFileManager } }
    return openInFileManager
  }

  it('resolves the raw span to an absolute path and reveals it', async () => {
    const openInFileManager = stubShell({ ok: true })
    const { pane } = makePane({
      contextFileLinkAbsolutePath: (raw: string) => `/repo/${raw}`
    })
    await revealTerminalMenuFileTarget(pane, { kind: 'file', rawPathText: 'src/app.ts' })
    expect(openInFileManager).toHaveBeenCalledWith('/repo/src/app.ts')
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('surfaces a toast when the span cannot resolve or the reveal fails', async () => {
    const openInFileManager = stubShell({ ok: true })
    const { pane } = makePane({ contextFileLinkAbsolutePath: () => null })
    await revealTerminalMenuFileTarget(pane, { kind: 'file', rawPathText: 'gone.txt' })
    expect(openInFileManager).not.toHaveBeenCalled()
    expect(toast.error).toHaveBeenCalledTimes(1)

    toast.error.mockClear()
    stubShell({ ok: false })
    const resolved = makePane({ contextFileLinkAbsolutePath: () => '/repo/gone.txt' })
    await revealTerminalMenuFileTarget(resolved.pane, { kind: 'file', rawPathText: 'gone.txt' })
    expect(toast.error).toHaveBeenCalledTimes(1)
  })
})

describe('copyTerminalMenuLastCommandOutput', () => {
  it('copies the block text on ok and toasts the eviction marker honestly', async () => {
    const { pane } = makePane({
      lastCommandOutputAsync: () => Promise.resolve({ status: 'ok', text: 'built ok\n', exitCode: 0 })
    })
    await copyTerminalMenuLastCommandOutput(pane)
    expect(copyVerified).toHaveBeenCalledWith('built ok\n', 'context-menu')
    expect(toast.success).toHaveBeenCalledWith('Command output copied')

    const evicted = makePane({
      lastCommandOutputAsync: () => Promise.resolve({ status: 'evicted' })
    })
    copyVerified.mockClear()
    await copyTerminalMenuLastCommandOutput(evicted.pane)
    expect(copyVerified).not.toHaveBeenCalled()
    expect(toast.error).toHaveBeenCalledWith('Command output scrolled past the scrollback limit')
  })

  it('is a defensive no-op when no block completed (item should be hidden)', async () => {
    const { pane } = makePane({ lastCommandOutputAsync: () => Promise.resolve(null) })
    await copyTerminalMenuLastCommandOutput(pane)
    expect(copyVerified).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })
})
