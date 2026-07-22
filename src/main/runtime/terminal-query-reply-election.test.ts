/**
 * #9156 — terminal query-reply authority election (RD-9156).
 *
 * Exactly one party may answer any embedded terminal query. Precedence:
 * elected mobile floor holder → visible host renderer → earliest addressable
 * remote viewer → model responder. The verdict is pushed to renderers via the
 * notifier and to remote streams via subscribeToQueryReplyAuthorityChanges;
 * the multiplex subscribe ack carries it too (see terminal-multiplex tests).
 */
import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import type * as GitUsernameModule from '../git/git-username'
import { OrcaRuntimeService } from './orca-runtime'
import type { RuntimeTerminalQueryReplyAuthority } from '../../shared/runtime-types'
import {
  markHiddenRendererPty,
  _resetHiddenRendererPtyDeliveryGateForTest
} from '../ipc/pty-hidden-delivery-gate'

vi.mock('../git/worktree', () => ({
  listWorktrees: vi.fn().mockResolvedValue([]),
  listWorktreesStrict: vi.fn().mockResolvedValue([])
}))
vi.mock('../hooks', () => ({
  createSetupRunnerScript: vi.fn(),
  getEffectiveHooks: vi.fn().mockReturnValue(null),
  runHook: vi.fn().mockResolvedValue({ success: true, output: '' })
}))
vi.mock('../ipc/worktree-logic', async (importOriginal) => {
  const actual = (await importOriginal()) as Record<string, unknown>
  return { ...actual, computeWorktreePath: vi.fn(), ensurePathWithinWorkspace: vi.fn() }
})
vi.mock('../ipc/filesystem-auth', () => ({ invalidateAuthorizedRootsCache: vi.fn() }))
vi.mock('../git/repo', async (importOriginal) => {
  const actual = (await importOriginal()) as Record<string, unknown>
  return {
    ...actual,
    getDefaultBaseRef: vi.fn().mockReturnValue('origin/main'),
    getBranchConflictKind: vi.fn().mockResolvedValue(null)
  }
})
vi.mock('../git/git-username', async () => {
  const actual = await vi.importActual<typeof GitUsernameModule>('../git/git-username')
  return { ...actual, resolveLocalGitUsername: vi.fn(async () => '') }
})

const store = {
  getRepo: () => ({
    id: 'repo-1',
    path: '/tmp/repo',
    displayName: 'repo',
    badgeColor: 'blue',
    addedAt: 1
  }),
  getRepos: () => [store.getRepo()],
  addRepo: () => {},
  updateRepo: () => undefined as never,
  getAllWorktreeMeta: () => ({}),
  getWorktreeMeta: () => undefined,
  getGitHubCache: () => ({ pr: {}, issue: {} }),
  setWorktreeMeta: () => undefined as never,
  removeWorktreeMeta: () => {},
  getSettings: () => ({
    workspaceDir: '/tmp/workspaces',
    nestWorkspaces: false,
    refreshLocalBaseRefOnWorktreeCreate: false,
    publishRemoteBranchOnWorktreeCreate: false,
    branchPrefix: 'none',
    branchPrefixCustom: '',
    mobileAutoRestoreFitMs: 5_000
  })
}

// Why withNotifier: "headless serve" is exactly the runtime with no attached
// renderer window — the notifier is never set there.
function createRuntime({ withNotifier = true }: { withNotifier?: boolean } = {}) {
  const runtime = new OrcaRuntimeService(store)
  const ptySizes = new Map<string, { cols: number; rows: number }>([
    ['pty-1', { cols: 150, rows: 40 }]
  ])
  const authorityEvents: { ptyId: string; authority: RuntimeTerminalQueryReplyAuthority }[] = []

  runtime.setPtyController({
    write: () => true,
    kill: () => true,
    getForegroundProcess: async () => null,
    resize: (ptyId, cols, rows) => {
      ptySizes.set(ptyId, { cols, rows })
      return true
    },
    getSize: (ptyId) => ptySizes.get(ptyId) ?? null
  })
  if (withNotifier) {
    runtime.setNotifier({
      worktreesChanged: vi.fn(),
      reposChanged: vi.fn(),
      activateWorktree: vi.fn(),
      createTerminal: vi.fn(),
      splitTerminal: vi.fn(),
      renameTerminal: vi.fn(),
      focusTerminal: vi.fn(),
      closeTerminal: vi.fn(),
      sleepWorktree: vi.fn(),
      terminalFitOverrideChanged: vi.fn(),
      terminalDriverChanged: vi.fn(),
      terminalQueryReplyAuthorityChanged: (ptyId, authority) => {
        authorityEvents.push({ ptyId, authority })
      }
    } as never)
  }

  return { runtime, authorityEvents }
}

describe('terminal query-reply election (#9156)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    _resetHiddenRendererPtyDeliveryGateForTest()
  })
  afterEach(() => {
    vi.useRealTimers()
    _resetHiddenRendererPtyDeliveryGateForTest()
  })

  it('elects the visible host renderer by default', () => {
    const { runtime } = createRuntime()
    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({ kind: 'host-renderer' })
  })

  it('elects mobile over visible host', async () => {
    const { runtime } = createRuntime()
    await runtime.handleMobileSubscribe('pty-1', 'phone-A', { cols: 45, rows: 20 })
    expect(runtime.getDriver('pty-1').kind).toBe('mobile')

    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'mobile',
      clientId: 'phone-A'
    })
  })

  it('elects visible host over remote viewer', () => {
    const { runtime } = createRuntime()
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')

    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({ kind: 'host-renderer' })
  })

  it('elects remote viewer when host pane is hidden', () => {
    const { runtime } = createRuntime()
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')
    markHiddenRendererPty('pty-1')

    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'remote-viewer',
      clientId: 'viewer-A'
    })
  })

  it('elects remote viewer on headless serve', () => {
    const { runtime } = createRuntime({ withNotifier: false })
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')

    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'remote-viewer',
      clientId: 'viewer-A'
    })
  })

  it('falls back to the model responder when no host and no addressable viewer exist', () => {
    const { runtime } = createRuntime({ withNotifier: false })
    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({ kind: 'model' })
  })

  it('skips identity-less legacy subscribers when electing a remote viewer', () => {
    const { runtime } = createRuntime({ withNotifier: false })
    // Legacy stream without a clientId subscribed first — it cannot be told it
    // won, so the earliest addressable viewer must win instead.
    runtime.registerRemoteTerminalViewSubscriber('pty-1')
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-B')

    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'remote-viewer',
      clientId: 'viewer-B'
    })
    // Model suppression (Phase-5) still sees the legacy subscriber.
    expect(runtime.hasRemoteTerminalViewSubscriber('pty-1')).toBe(true)
  })

  it('re-elects on viewer release', () => {
    const { runtime, authorityEvents } = createRuntime({ withNotifier: false })
    const releaseA = runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-B')
    const listenerEvents: RuntimeTerminalQueryReplyAuthority[] = []
    runtime.subscribeToQueryReplyAuthorityChanges('pty-1', (authority) => {
      listenerEvents.push(authority)
    })
    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'remote-viewer',
      clientId: 'viewer-A'
    })

    releaseA()

    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'remote-viewer',
      clientId: 'viewer-B'
    })
    expect(listenerEvents).toEqual([{ kind: 'remote-viewer', clientId: 'viewer-B' }])
    // Release is idempotent — a double release must not re-notify or re-elect.
    releaseA()
    expect(listenerEvents).toHaveLength(1)
    expect(authorityEvents).toHaveLength(0) // no notifier on headless serve
  })

  it('pushes the verdict through the notifier and dedupes unchanged elections', () => {
    const { runtime, authorityEvents } = createRuntime()
    const releaseA = runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')
    // Host stays authoritative while visible: first verdict is host-renderer.
    expect(authorityEvents).toEqual([{ ptyId: 'pty-1', authority: { kind: 'host-renderer' } }])

    // A second subscriber does not change the winner — deduped, no new event.
    const releaseB = runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-B')
    expect(authorityEvents).toHaveLength(1)

    // Hiding the host pane re-elects the earliest addressable viewer.
    markHiddenRendererPty('pty-1')
    runtime.notifyTerminalQueryReplyAuthorityMayHaveChanged('pty-1')
    expect(authorityEvents.at(-1)).toEqual({
      ptyId: 'pty-1',
      authority: { kind: 'remote-viewer', clientId: 'viewer-A' }
    })

    releaseA()
    expect(authorityEvents.at(-1)).toEqual({
      ptyId: 'pty-1',
      authority: { kind: 'remote-viewer', clientId: 'viewer-B' }
    })
    releaseB()
    expect(authorityEvents.at(-1)).toEqual({
      ptyId: 'pty-1',
      authority: { kind: 'model' }
    })
  })

  it('emits a mobile verdict when the phone takes the floor and re-elects when it leaves', async () => {
    const { runtime, authorityEvents } = createRuntime()
    await runtime.handleMobileSubscribe('pty-1', 'phone-A', { cols: 45, rows: 20 })
    expect(authorityEvents.at(-1)).toEqual({
      ptyId: 'pty-1',
      authority: { kind: 'mobile', clientId: 'phone-A' }
    })

    runtime.handleMobileUnsubscribe('pty-1', 'phone-A')
    // Soft-leave grace holds the floor; the host verdict lands once the driver flips.
    await vi.runAllTimersAsync()
    expect(runtime.getDriver('pty-1').kind).not.toBe('mobile')
    expect(authorityEvents.at(-1)).toEqual({
      ptyId: 'pty-1',
      authority: { kind: 'host-renderer' }
    })
  })

  it('keeps isMobileTerminalQueryReplyAuthority semantics for the mobile floor election', async () => {
    const { runtime } = createRuntime()
    await runtime.handleMobileSubscribe('pty-1', 'phone-A', { cols: 45, rows: 20 })
    await runtime.handleMobileSubscribe('pty-1', 'phone-B', { cols: 45, rows: 20 })

    expect(runtime.isMobileTerminalQueryReplyAuthority('pty-1', 'phone-A')).toBe(true)
    expect(runtime.isMobileTerminalQueryReplyAuthority('pty-1', 'phone-B')).toBe(false)
    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({
      kind: 'mobile',
      clientId: 'phone-A'
    })
  })

  it('re-elects watched PTYs when the renderer window detaches', () => {
    const { runtime } = createRuntime()
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')
    const listenerEvents: RuntimeTerminalQueryReplyAuthority[] = []
    runtime.subscribeToQueryReplyAuthorityChanges('pty-1', (authority) => {
      listenerEvents.push(authority)
    })
    expect(runtime.getTerminalQueryReplyAuthority('pty-1')).toEqual({ kind: 'host-renderer' })

    // Window closed / renderer reloading: the host-renderer rung disappears.
    runtime.setNotifier(null)
    expect(listenerEvents.at(-1)).toEqual({ kind: 'remote-viewer', clientId: 'viewer-A' })

    // Window reattached: host wins again.
    runtime.setNotifier({
      worktreesChanged: vi.fn(),
      reposChanged: vi.fn(),
      activateWorktree: vi.fn(),
      createTerminal: vi.fn(),
      splitTerminal: vi.fn(),
      renameTerminal: vi.fn(),
      focusTerminal: vi.fn(),
      closeTerminal: vi.fn(),
      sleepWorktree: vi.fn(),
      terminalFitOverrideChanged: vi.fn(),
      terminalDriverChanged: vi.fn()
    } as never)
    expect(listenerEvents.at(-1)).toEqual({ kind: 'host-renderer' })
  })

  it('clears election state on PTY exit', () => {
    const { runtime, authorityEvents } = createRuntime()
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')
    expect(authorityEvents).toHaveLength(1)

    runtime.onPtyExit('pty-1', 0)

    expect(runtime.hasRemoteTerminalViewSubscriber('pty-1')).toBe(false)
    // The dedupe fingerprint is dropped with the PTY, so a fresh registration re-notifies.
    runtime.registerRemoteTerminalViewSubscriber('pty-1', 'viewer-A')
    expect(authorityEvents.at(-1)).toEqual({
      ptyId: 'pty-1',
      authority: { kind: 'host-renderer' }
    })
  })
})
