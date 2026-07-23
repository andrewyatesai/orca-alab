/* eslint-disable max-lines -- Why: remote runtime PTY behavior spans JSON fallback, binary stream, lifecycle, and parser coverage; keeping the matrix together catches transport regressions. */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  decodeTerminalStreamText,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson,
  encodeTerminalStreamText
} from '../../../../shared/terminal-stream-protocol'
import {
  TERMINAL_INPUT_CHUNK_MAX_BYTES,
  TERMINAL_INPUT_MAX_BYTES
} from '../../../../shared/terminal-input'
import { CLIPBOARD_TEXT_MEASURE_YIELD_CODE_UNITS } from '../../../../shared/clipboard-text'
import { remapRemoteSearchRow } from '../../../../shared/terminal-remote-search-protocol'
import type { PtyTransport } from './pty-transport-types'

describe('createRemoteRuntimePtyTransport', () => {
  const runtimeCall = vi.fn()
  const runtimeSubscribe = vi.fn()
  const subscriptionSendBinary = vi.fn()
  let subscriptionCallbacks: {
    onResponse: (response: unknown) => void
    onBinary?: (bytes: Uint8Array<ArrayBufferLike>) => void
    onError?: (error: { code: string; message: string }) => void
    onClose?: () => void
  } | null = null

  function emitMultiplexReady(): void {
    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'ready' }
    })
  }

  function latestSubscribePayload(): {
    streamId: number
    terminal: string
    client: { id: string; type: string }
    viewport?: { cols: number; rows: number }
    capabilities?: { desktopViewportClaims?: 1 }
  } {
    const frames = subscriptionSendBinary.mock.calls
      .map((call) => decodeTerminalStreamFrame(call[0]))
      .filter((frame) => frame?.opcode === TerminalStreamOpcode.Subscribe)
    const frame = frames.at(-1)
    if (!frame) {
      throw new Error('missing terminal subscribe frame')
    }
    const payload = decodeTerminalStreamJson<{
      streamId: number
      terminal: string
      client: { id: string; type: string }
      viewport?: { cols: number; rows: number }
      capabilities?: { desktopViewportClaims?: 1 }
    }>(frame.payload)
    if (!payload) {
      throw new Error('invalid terminal subscribe payload')
    }
    return payload
  }

  function emitOutput(streamId: number, data: string): void {
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.Output,
        streamId,
        seq: 1,
        payload: encodeTerminalStreamText(data)
      })
    )
  }

  function emitSnapshot(streamId: number, data: string): void {
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotStart,
        streamId,
        seq: 1,
        payload: encodeTerminalStreamJson({ kind: 'scrollback' })
      })
    )
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotChunk,
        streamId,
        seq: 2,
        payload: encodeTerminalStreamText(data)
      })
    )
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotEnd,
        streamId,
        seq: 3,
        payload: new Uint8Array()
      })
    )
  }

  function latestFrameForOpcode(opcode: TerminalStreamOpcode) {
    return subscriptionSendBinary.mock.calls
      .map((call) => decodeTerminalStreamFrame(call[0]))
      .findLast((frame) => frame?.opcode === opcode)
  }

  function emitSnapshotFrame(
    streamId: number,
    opcode:
      | TerminalStreamOpcode.SnapshotStart
      | TerminalStreamOpcode.SnapshotChunk
      | TerminalStreamOpcode.SnapshotEnd,
    payload: Uint8Array<ArrayBufferLike>
  ): void {
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode,
        streamId,
        seq: 1,
        payload
      })
    )
  }

  beforeEach(() => {
    vi.resetModules()
    vi.doUnmock('../../runtime/remote-runtime-terminal-multiplexer')
    vi.clearAllMocks()
    subscriptionCallbacks = null
    subscriptionSendBinary.mockReset()
    runtimeCall.mockResolvedValue({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
    runtimeSubscribe.mockImplementation(
      async (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
        subscriptionCallbacks = callbacks
        queueMicrotask(emitMultiplexReady)
        return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
      }
    )
    vi.stubGlobal('window', {
      api: {
        runtimeEnvironments: {
          call: runtimeCall,
          subscribe: runtimeSubscribe
        }
      }
    })
  })

  it('attaches to an existing remote runtime terminal handle', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onError = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    transport.attach({
      existingPtyId: 'remote:terminal-1',
      cols: 120,
      rows: 40,
      callbacks: { onError }
    })

    await vi.waitFor(() => {
      expect(runtimeSubscribe).toHaveBeenCalled()
    })

    expect(onError).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-1')
    expect(transport.getRuntimeEnvironmentId?.()).toBe('env-1')
    await vi.waitFor(() =>
      expect(latestSubscribePayload().capabilities).toEqual({
        ackOutput: 1,
        desktopViewportClaims: 1
      })
    )
    expect(runtimeSubscribe).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'terminal.multiplex',
        params: {}
      }),
      expect.any(Object)
    )
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    expect(latestSubscribePayload()).toMatchObject({
      terminal: 'terminal-1',
      client: { id: expect.stringMatching(/^desktop:tab-1:pane:1:/), type: 'desktop' },
      viewport: { cols: 120, rows: 40 }
    })
  })

  it('scopes the same legacy handle independently for each runtime environment', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const first = createRemoteRuntimePtyTransport('env-1', { worktreeId: 'wt-1' })
    const second = createRemoteRuntimePtyTransport('env-2', { worktreeId: 'wt-2' })

    first.attach({ existingPtyId: 'remote:terminal-1', callbacks: {} })
    second.attach({ existingPtyId: 'remote:terminal-1', callbacks: {} })

    expect(first.getPtyId()).toBe('remote:env-1@@terminal-1')
    expect(second.getPtyId()).toBe('remote:env-2@@terminal-1')
  })

  it('parks passive peers when another remote desktop owns the grid', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const { getFitOverrideForPty, setFitOverride } =
      await import('@/lib/pane-manager/mobile-fit-overrides')
    const transport = createRemoteRuntimePtyTransport('env-1', { worktreeId: 'wt-1' })
    await transport.connect({ url: '', cols: 120, rows: 40, callbacks: {} })
    const { streamId } = latestSubscribePayload()
    const ptyId = transport.getPtyId()
    expect(ptyId).not.toBeNull()

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: {
        type: 'fit-override-changed',
        streamId,
        mode: 'remote-desktop-fit',
        cols: 96,
        rows: 32
      }
    })

    expect(ptyId ? getFitOverrideForPty(ptyId) : null).toEqual({
      mode: 'remote-desktop-fit',
      cols: 96,
      rows: 32
    })
    if (ptyId) {
      setFitOverride(ptyId, 'desktop-fit', 0, 0)
    }
  })

  it('gives separate paired viewers of the same host pane distinct refresh identities', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const first = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })
    const second = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    first.attach({ existingPtyId: 'remote:terminal-1', cols: 80, rows: 24, callbacks: {} })
    second.attach({ existingPtyId: 'remote:terminal-1', cols: 120, rows: 40, callbacks: {} })

    await vi.waitFor(() => {
      const subscribeFrames = subscriptionSendBinary.mock.calls
        .map((call) => decodeTerminalStreamFrame(call[0]))
        .filter((frame) => frame?.opcode === TerminalStreamOpcode.Subscribe)
      expect(subscribeFrames).toHaveLength(2)
      const clientIds = subscribeFrames.map((frame) => {
        const payload = frame
          ? decodeTerminalStreamJson<{ client: { id: string } }>(frame.payload)
          : null
        return payload?.client.id
      })
      expect(clientIds[0]).toMatch(/^desktop:tab-1:pane:1:/)
      expect(clientIds[1]).toMatch(/^desktop:tab-1:pane:1:/)
      expect(clientIds[0]).not.toBe(clientIds[1])
    })

    first.destroy?.()
    second.destroy?.()
  })

  it('routes encoded restored terminal ids to their owning runtime environment', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-2', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-1',
      cols: 120,
      rows: 40,
      callbacks: {}
    })

    await vi.waitFor(() => {
      expect(runtimeSubscribe).toHaveBeenCalled()
    })

    expect(runtimeSubscribe).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'terminal.multiplex'
      }),
      expect.any(Object)
    )
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    expect(latestSubscribePayload()).toMatchObject({
      terminal: 'terminal-1',
      viewport: { cols: 120, rows: 40 }
    })
  })

  it('re-derives the host session handle after a transport close instead of resubscribing the stale one', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const { getAllOverrides, setFitOverride } =
      await import('@/lib/pane-manager/mobile-fit-overrides')
    const { getAllDrivers, setDriverForPty } =
      await import('@/lib/pane-manager/mobile-driver-state')
    const onPtySpawn = vi.fn()
    const onPtyRebind = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtySpawn,
      onPtyRebind
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-1',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-1' })
    setFitOverride('remote:env-1@@terminal-1', 'mobile-fit', 49, 20)
    setDriverForPty('remote:env-1@@terminal-1', { kind: 'mobile', clientId: 'phone-1' })

    // Why: while the tunnel was down the host re-minted this pane's handle;
    // resubscribing the stale closure handle would bind the mirror to a
    // different PTY (#7718). The transport must re-derive from the snapshot.
    runtimeCall.mockImplementation(async (args: { method: string }) =>
      args.method === 'session.tabs.list'
        ? {
            ok: true,
            result: {
              worktree: 'wt-1',
              publicationEpoch: 'epoch-1',
              snapshotVersion: 2,
              activeGroupId: null,
              activeTabId: 'tab-1::pane:1',
              activeTabType: 'terminal',
              tabs: [
                {
                  type: 'terminal',
                  id: 'tab-1::pane:1',
                  parentTabId: 'tab-1',
                  leafId: 'pane:1',
                  title: 'Terminal',
                  isActive: true,
                  status: 'ready',
                  terminal: 'terminal-2'
                }
              ]
            }
          }
        : { ok: true, result: {} }
    )
    const subscribeCallsBefore = runtimeSubscribe.mock.calls.length

    // The dedicated multiplex socket dies (liveness/close) → onTransportClose.
    subscriptionCallbacks?.onClose?.()

    await vi.waitFor(() =>
      expect(runtimeSubscribe.mock.calls.length).toBeGreaterThan(subscribeCallsBefore)
    )
    await vi.waitFor(() =>
      expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-2' })
    )
    expect(transport.getPtyId()).toContain('terminal-2')
    expect(onPtySpawn).not.toHaveBeenCalled()
    expect(onPtyRebind).toHaveBeenCalledWith(
      expect.stringContaining('terminal-2'),
      expect.stringContaining('terminal-1')
    )
    expect([...getAllOverrides().keys()]).toEqual(['remote:env-1@@terminal-2'])
    expect([...getAllDrivers().keys()]).toEqual(['remote:env-1@@terminal-2'])
  })

  it('retires the mirror when the host no longer publishes the surface after a transport close', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onPtyExit = vi.fn()
    const onError = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-1',
      cols: 80,
      rows: 24,
      callbacks: { onError }
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())

    runtimeCall.mockImplementation(async (args: { method: string }) =>
      args.method === 'session.tabs.list'
        ? {
            ok: true,
            result: {
              worktree: 'wt-1',
              publicationEpoch: 'epoch-1',
              snapshotVersion: 2,
              activeGroupId: null,
              activeTabId: null,
              activeTabType: null,
              tabs: []
            }
          }
        : { ok: true, result: {} }
    )

    subscriptionCallbacks?.onClose?.()

    // Why: no red xterm error — retire quietly and let the next session-tabs
    // snapshot drive respawn/removal.
    await vi.waitFor(() => expect(onPtyExit).toHaveBeenCalledWith('remote:env-1@@terminal-1', 1))
    expect(transport.getPtyId()).toBeNull()
    expect(onError).not.toHaveBeenCalled()
  })

  it('does not close host-owned terminal handles attached from session snapshots', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1'
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-1',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    runtimeCall.mockClear()

    transport.destroy?.()

    expect(runtimeCall).not.toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'terminal.close'
      })
    )
  })

  it('detaches laptop-created remote runtime terminals without closing the server session', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    runtimeCall.mockClear()

    transport.destroy?.()

    expect(runtimeCall).not.toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'terminal.close'
      })
    )
  })

  it('keeps the regular TUI and draft through inventory failure and stale-handle reconnect', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onError = vi.fn()
    const onPtyExit = vi.fn()
    const onPtySpawn = vi.fn()
    const onPtyRebind = vi.fn()
    const onExit = vi.fn()
    const onDisconnect = vi.fn()
    const renderedScreen: string[] = []
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtyExit,
      onPtySpawn,
      onPtyRebind
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-stale',
      cols: 80,
      rows: 24,
      callbacks: {
        onError,
        onExit,
        onDisconnect,
        onData: (data) => renderedScreen.push(data),
        onReplayData: (data) => renderedScreen.push(data)
      }
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const initialStreamId = latestSubscribePayload().streamId
    const draft = 'QA regular reconnect draft - keep this unsent'
    emitOutput(initialStreamId, draft)

    let hostListCalls = 0
    runtimeCall.mockImplementation(async (args: { method: string }) => {
      if (args.method === 'session.tabs.list') {
        hostListCalls += 1
        if (hostListCalls === 1) {
          throw new Error('runtime reconnect in progress')
        }
        const terminal =
          hostListCalls === 2
            ? 'terminal-stale'
            : hostListCalls === 3
              ? null
              : 'terminal-reconnected'
        return {
          ok: true,
          result: {
            worktree: 'wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: hostListCalls + 1,
            activeGroupId: null,
            activeTabId: 'tab-1::pane:1',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'tab-1::pane:1',
                parentTabId: 'tab-1',
                leafId: 'pane:1',
                title: 'Claude Code',
                isActive: true,
                status: terminal ? 'ready' : 'pending-handle',
                terminal
              }
            ]
          }
        }
      }
      return { ok: true, result: {} }
    })

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'error', streamId: initialStreamId, message: 'terminal_handle_stale' }
    })

    await vi.waitFor(() => expect(hostListCalls).toBeGreaterThanOrEqual(1))
    expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-stale' })
    expect(onPtyExit).not.toHaveBeenCalled()
    await vi.waitFor(
      () => expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-reconnected' }),
      { timeout: 2_000 }
    )
    const replacementStreamId = latestSubscribePayload().streamId
    emitSnapshot(replacementStreamId, draft)

    expect(onError).not.toHaveBeenCalled()
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(onPtySpawn).not.toHaveBeenCalled()
    expect(onPtyRebind).toHaveBeenCalledOnce()
    expect(onPtyRebind).toHaveBeenCalledWith(
      'remote:env-1@@terminal-reconnected',
      'remote:env-1@@terminal-stale'
    )
    expect(onExit).not.toHaveBeenCalled()
    expect(onDisconnect).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-reconnected')
    expect(transport.isConnected()).toBe(true)
    expect(renderedScreen.at(-1)).toBe(draft)
    expect(hostListCalls).toBe(4)
    const subscribedTerminals = subscriptionSendBinary.mock.calls
      .map((call) => decodeTerminalStreamFrame(call[0]))
      .flatMap((frame) => {
        if (frame?.opcode !== TerminalStreamOpcode.Subscribe) {
          return []
        }
        const payload = decodeTerminalStreamJson<{ terminal: string }>(frame.payload)
        return payload ? [payload.terminal] : []
      })
    expect(subscribedTerminals).toEqual(['terminal-stale', 'terminal-reconnected'])
  })

  it('reattaches from a later host snapshot after bounded replacement polling stops', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const onError = vi.fn()
      const onPtyExit = vi.fn()
      const onPtyRebind = vi.fn()
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'web-terminal-tab-1',
        leafId: 'pane:1',
        onPtyExit,
        onPtyRebind
      })

      transport.attach({
        existingPtyId: 'remote:env-1@@terminal-stale',
        cols: 80,
        rows: 24,
        callbacks: { onError }
      })
      await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())

      let hostListCalls = 0
      runtimeCall.mockImplementation(async (args: { method: string }) => {
        if (args.method === 'terminal.send') {
          return {
            ok: false,
            error: { code: 'terminal_handle_stale', message: 'terminal_handle_stale' }
          }
        }
        if (args.method !== 'session.tabs.list') {
          return { ok: true, result: {} }
        }
        hostListCalls += 1
        return {
          ok: true,
          result: {
            worktree: 'wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: hostListCalls + 1,
            activeGroupId: null,
            activeTabId: 'tab-1::pane:1',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'tab-1::pane:1',
                parentTabId: 'tab-1',
                leafId: 'pane:1',
                title: 'Claude Code',
                isActive: true,
                status: 'ready',
                terminal: 'terminal-stale'
              }
            ]
          }
        }
      })

      subscriptionCallbacks?.onResponse({
        ok: true,
        result: {
          type: 'error',
          streamId: latestSubscribePayload().streamId,
          message: 'terminal_handle_stale'
        }
      })
      await vi.advanceTimersByTimeAsync(16_000)

      expect(hostListCalls).toBeGreaterThan(1)
      expect(hostListCalls).toBeLessThan(25)
      const listTimeouts = runtimeCall.mock.calls
        .map(([args]) => args)
        .filter((args) => args.method === 'session.tabs.list')
        .map((args) => args.timeoutMs as number)
      expect(listTimeouts[0]).toBe(15_000)
      expect(listTimeouts.every((timeoutMs) => timeoutMs > 0 && timeoutMs <= 15_000)).toBe(true)
      expect(listTimeouts.at(-1)).toBeLessThanOrEqual(1_000)
      expect(onError).not.toHaveBeenCalled()
      expect(onPtyExit).not.toHaveBeenCalled()
      expect(transport.getPtyId()).toBe('remote:env-1@@terminal-stale')
      expect(transport.isConnected()).toBe(true)
      const handleEvents = await import('../../runtime/web-session-terminal-handle-events')
      expect(handleEvents.getWebSessionTerminalHandleSubscriberCountForTests()).toBe(1)

      const listCallsAfterBound = hostListCalls
      await expect(transport.sendInputAccepted?.('retry while reconnecting')).resolves.toBe(false)
      await vi.advanceTimersByTimeAsync(16_000)

      // The accepted-snapshot listener already owns recovery. User input must
      // not turn a bounded reconnect into recurring host-inventory polling.
      expect(hostListCalls).toBe(listCallsAfterBound)
      expect(handleEvents.getWebSessionTerminalHandleSubscriberCountForTests()).toBe(1)

      handleEvents.queueAcceptedWebSessionTerminalSnapshot(
        {
          worktree: 'wt-1',
          publicationEpoch: 'epoch-2',
          snapshotVersion: 1,
          activeGroupId: null,
          activeTabId: 'tab-1::pane:1',
          activeTabType: 'terminal',
          tabs: [
            {
              type: 'terminal',
              id: 'tab-1::pane:1',
              parentTabId: 'tab-1',
              leafId: 'pane:1',
              title: 'Claude Code',
              isActive: true,
              status: 'ready',
              terminal: 'terminal-after-timeout'
            }
          ]
        },
        'env-1'
      )
      await vi.advanceTimersByTimeAsync(0)
      await vi.waitFor(() =>
        expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-after-timeout' })
      )

      expect(onPtyRebind).toHaveBeenCalledWith(
        'remote:env-1@@terminal-after-timeout',
        'remote:env-1@@terminal-stale'
      )
      expect(onPtyExit).not.toHaveBeenCalled()
      expect(transport.getPtyId()).toBe('remote:env-1@@terminal-after-timeout')
      expect(handleEvents.getWebSessionTerminalHandleSubscriberCountForTests()).toBe(0)
      const subscribedTerminals = subscriptionSendBinary.mock.calls
        .map((call) => decodeTerminalStreamFrame(call[0]))
        .flatMap((frame) => {
          if (frame?.opcode !== TerminalStreamOpcode.Subscribe) {
            return []
          }
          const payload = decodeTerminalStreamJson<{ terminal: string }>(frame.payload)
          return payload ? [payload.terminal] : []
        })
      expect(subscribedTerminals).toEqual(['terminal-stale', 'terminal-after-timeout'])
    } finally {
      vi.useRealTimers()
    }
  })

  it('coalesces concurrent stale errors for the handle that was replaced', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-stale',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())

    let resolveHostList: (response: unknown) => void = () => {}
    const hostListResponse = new Promise((resolve) => {
      resolveHostList = resolve
    })
    let hostListCalls = 0
    runtimeCall.mockImplementation((args: { method: string }) => {
      if (args.method === 'terminal.send') {
        return Promise.resolve({
          ok: false,
          error: { code: 'terminal_handle_stale', message: 'terminal_handle_stale' }
        })
      }
      if (args.method === 'session.tabs.list') {
        hostListCalls += 1
        return hostListResponse
      }
      return Promise.resolve({ ok: true, result: {} })
    })

    const sendInputAccepted = transport.sendInputAccepted
    if (!sendInputAccepted) {
      throw new Error('Expected acknowledged remote terminal input')
    }
    const sends = Promise.all([sendInputAccepted('first'), sendInputAccepted('second')])
    await vi.waitFor(() => expect(hostListCalls).toBe(1))
    await expect(sends).resolves.toEqual([false, false])

    resolveHostList({
      ok: true,
      result: {
        worktree: 'wt-1',
        publicationEpoch: 'epoch-1',
        snapshotVersion: 2,
        activeGroupId: null,
        activeTabId: 'tab-1::pane:1',
        activeTabType: 'terminal',
        tabs: [
          {
            type: 'terminal',
            id: 'tab-1::pane:1',
            parentTabId: 'tab-1',
            leafId: 'pane:1',
            title: 'Claude Code',
            isActive: true,
            status: 'ready',
            terminal: 'terminal-reconnected'
          }
        ]
      }
    })

    await vi.waitFor(() =>
      expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-reconnected' })
    )
    await Promise.resolve()

    // Why: the second stale response belonged to terminal-stale. Replaying it
    // against the replacement would add another polling loop and retire it.
    expect(hostListCalls).toBe(1)
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-reconnected')
    expect(transport.isConnected()).toBe(true)
  })

  it('still retires the regular TUI surface after an explicit terminal exit', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-exited',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: {
        type: 'error',
        streamId: latestSubscribePayload().streamId,
        message: 'terminal_exited'
      }
    })

    expect(onPtyExit).toHaveBeenCalledWith('remote:env-1@@terminal-exited', 1)
    expect(transport.getPtyId()).toBeNull()
    expect(transport.isConnected()).toBe(false)
  })

  it('propagates pty-exit for stale non-mirror remote terminals', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-stale',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'error', streamId, message: 'terminal_handle_stale' }
    })

    expect(onPtyExit).toHaveBeenCalledWith('remote:env-1@@terminal-stale', 1)
    expect(transport.getPtyId()).toBeNull()
    expect(transport.isConnected()).toBe(false)
  })

  it('ignores a late gone error for a replaced handle instead of retiring the successor', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-stale',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const initialStreamId = latestSubscribePayload().streamId

    let rejectSend: (error: unknown) => void = () => {}
    const pendingSend = new Promise((_resolve, reject) => {
      rejectSend = reject
    })
    runtimeCall.mockImplementation((args: { method: string }) => {
      if (args.method === 'terminal.send') {
        return pendingSend
      }
      if (args.method === 'session.tabs.list') {
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: 2,
            activeGroupId: null,
            activeTabId: 'tab-1::pane:1',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'tab-1::pane:1',
                parentTabId: 'tab-1',
                leafId: 'pane:1',
                title: 'Claude Code',
                isActive: true,
                status: 'ready',
                terminal: 'terminal-reconnected'
              }
            ]
          }
        })
      }
      return Promise.resolve({ ok: true, result: {} })
    })

    const sendInputAccepted = transport.sendInputAccepted
    if (!sendInputAccepted) {
      throw new Error('Expected acknowledged remote terminal input')
    }
    const send = sendInputAccepted('late input')

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'error', streamId: initialStreamId, message: 'terminal_handle_stale' }
    })
    await vi.waitFor(() =>
      expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-reconnected' })
    )

    rejectSend(new Error('terminal_gone'))
    await expect(send).resolves.toBe(false)
    await Promise.resolve()

    // Why (#8871): the gone error belonged to terminal-stale; it must not retire the rebound live handle.
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-reconnected')
    expect(transport.isConnected()).toBe(true)
  })

  it('routes no_connected_pty on a host mirror to a disconnected state, not an exit', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onError = vi.fn()
    const onDisconnect = vi.fn()
    const onExit = vi.fn()
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-1',
      cols: 80,
      rows: 24,
      callbacks: { onError, onDisconnect, onExit }
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()

    // Why: the paired host dropped off the runtime — its agent may still be
    // working, so this must not read as a PTY exit (#9151).
    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'error', streamId, message: 'no_connected_pty' }
    })
    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'end', streamId }
    })
    await Promise.resolve()

    expect(onError).not.toHaveBeenCalled()
    expect(onExit).not.toHaveBeenCalled()
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(onDisconnect).toHaveBeenCalledTimes(1)
    expect(transport.isConnected()).toBe(false)
    // Why: the PTY binding survives so the pane keeps its agent evidence and a
    // later session snapshot or reattach can revive the mirror.
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-1')
  })

  it('reports exit for a stream end only after the runtime confirms the host exited', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onExit = vi.fn()
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    await transport.connect({ url: '', callbacks: { onExit } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'end', streamId }
    })

    await vi.waitFor(() => expect(onPtyExit).toHaveBeenCalledWith('remote:env-1@@terminal-1', 0))
    expect(onExit).toHaveBeenCalledWith(0)
    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'terminal.wait',
        params: expect.objectContaining({ terminal: 'terminal-1', for: 'exit' })
      })
    )
    expect(transport.getPtyId()).toBeNull()
    expect(transport.isConnected()).toBe(false)
  })

  it('resubscribes instead of reporting exit when a stream end is not a confirmed host exit', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onExit = vi.fn()
    const onPtyExit = vi.fn()
    runtimeCall.mockImplementation(async (args: { method: string }) =>
      args.method === 'terminal.wait'
        ? // Why: the terminal is still running — the stream end was a
          // server-side cleanup/eviction, not a host exit (#9151).
          { ok: false, error: { code: 'timeout', message: 'timeout' } }
        : { ok: true, result: { terminal: { handle: 'terminal-1' } } }
    )
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    await transport.connect({ url: '', callbacks: { onExit } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()
    const subscribeCallsBefore = runtimeSubscribe.mock.calls.length

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'end', streamId }
    })

    await vi.waitFor(() =>
      expect(runtimeSubscribe.mock.calls.length).toBeGreaterThan(subscribeCallsBefore)
    )
    expect(onExit).not.toHaveBeenCalled()
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-1')
    expect(transport.isConnected()).toBe(true)
  })

  it('ignores stale stream end after reattaching a newer remote terminal', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-old',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const oldStreamId = latestSubscribePayload().streamId

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-new',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'end', streamId: oldStreamId }
    })

    expect(onPtyExit).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-new')
    expect(transport.isConnected()).toBe(true)

    await vi.waitFor(() => {
      expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-new' })
    })
    const newStreamId = latestSubscribePayload().streamId

    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'end', streamId: newStreamId }
    })

    // Why: exit is reported only after the runtime confirms the host terminal
    // exited (terminal.wait), so the report lands asynchronously (#9151).
    await vi.waitFor(() =>
      expect(onPtyExit).toHaveBeenCalledWith('remote:env-1@@terminal-new', 0)
    )
    expect(transport.getPtyId()).toBeNull()
    expect(transport.isConnected()).toBe(false)
  })

  it('applies the subscribe-ack query-reply authority and clears it on stream end (#9156)', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const {
      getQueryReplyAuthorityForPty,
      isQueryReplyAuthorityForThisView,
      _resetQueryReplyAuthorityStateForTest
    } = await import('@/lib/pane-manager/query-reply-authority-state')
    try {
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })
      transport.attach({
        existingPtyId: 'remote:terminal-1',
        cols: 120,
        rows: 40,
        callbacks: {}
      })
      await vi.waitFor(() => latestSubscribePayload())
      const payload = latestSubscribePayload()
      const ptyId = 'remote:env-1@@terminal-1'
      // The gate matches this view against the clientId the election names.
      expect(transport.getQueryReplyViewerClientId?.()).toBe(payload.client.id)

      // Critic note: the verdict rides the subscribe ack, so a headless-serve
      // viewer knows it answers before the first drained query (no zero-answerer window).
      subscriptionCallbacks?.onResponse({
        ok: true,
        result: {
          type: 'subscribed',
          streamId: payload.streamId,
          queryReplyAuthority: { kind: 'remote-viewer', clientId: payload.client.id }
        }
      })
      expect(getQueryReplyAuthorityForPty(ptyId)).toEqual({
        kind: 'remote-viewer',
        clientId: payload.client.id
      })
      expect(isQueryReplyAuthorityForThisView(ptyId, payload.client.id)).toBe(true)

      // A re-election event (host pane revealed) silences this viewer.
      subscriptionCallbacks?.onResponse({
        ok: true,
        result: {
          type: 'query-reply-authority-changed',
          streamId: payload.streamId,
          authority: { kind: 'host-renderer' }
        }
      })
      expect(isQueryReplyAuthorityForThisView(ptyId, payload.client.id)).toBe(false)

      // Stream end clears the stale verdict so the next attach fails open.
      subscriptionCallbacks?.onResponse({
        ok: true,
        result: { type: 'end', streamId: payload.streamId }
      })
      await vi.waitFor(() => expect(getQueryReplyAuthorityForPty(ptyId)).toBeNull())
      expect(isQueryReplyAuthorityForThisView(ptyId, payload.client.id)).toBe(true)
    } finally {
      _resetQueryReplyAuthorityStateForTest()
    }
  })

  it('drops pending input when attaching a different remote terminal handle', async () => {
    vi.useFakeTimers()
    runtimeSubscribe.mockImplementation(
      async (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
        subscriptionCallbacks = callbacks
        return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
      }
    )
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      transport.attach({
        existingPtyId: 'remote:env-1@@terminal-old',
        cols: 80,
        rows: 24,
        callbacks: {}
      })
      expect(transport.sendInput('queued-for-old')).toBe(true)

      transport.attach({
        existingPtyId: 'remote:env-1@@terminal-new',
        cols: 80,
        rows: 24,
        callbacks: {}
      })
      runtimeCall.mockClear()

      await vi.advanceTimersByTimeAsync(10)

      expect(runtimeCall).not.toHaveBeenCalledWith(
        expect.objectContaining({
          method: 'terminal.send'
        })
      )
    } finally {
      vi.useRealTimers()
    }
  })

  it('ignores stale attach subscription rejection after reattaching a newer remote terminal', async () => {
    const oldSubscription = {
      reject: null as ((error: Error) => void) | null
    }
    const newStream = {
      streamId: 2,
      sendInput: vi.fn(() => true),
      resize: vi.fn(() => true),
      serializeBuffer: vi.fn(async () => null),
      close: vi.fn()
    }
    const subscribeTerminal = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, reject) => {
            oldSubscription.reject = reject
          })
      )
      .mockResolvedValueOnce(newStream)
    vi.doMock('../../runtime/remote-runtime-terminal-multiplexer', () => ({
      getRemoteRuntimeTerminalMultiplexer: vi.fn(() => ({ subscribeTerminal }))
    }))
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onError = vi.fn()
    const onPtyExit = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-old',
      cols: 80,
      rows: 24,
      callbacks: { onError }
    })
    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-new',
      cols: 80,
      rows: 24,
      callbacks: { onError }
    })
    await vi.waitFor(() => expect(subscribeTerminal).toHaveBeenCalledTimes(2))

    oldSubscription.reject?.(new Error('terminal_handle_stale'))
    await Promise.resolve()
    await Promise.resolve()

    expect(onError).not.toHaveBeenCalled()
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(transport.getPtyId()).toBe('remote:env-1@@terminal-new')
    expect(transport.isConnected()).toBe(true)
  })

  it('does not send queued input through a stale stream during remote handle replacement', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-old',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())

    vi.useFakeTimers()
    try {
      subscriptionSendBinary.mockClear()
      runtimeCall.mockClear()

      transport.attach({
        existingPtyId: 'remote:env-1@@terminal-new',
        cols: 80,
        rows: 24,
        callbacks: {}
      })
      subscriptionSendBinary.mockClear()

      expect(transport.sendInput('x')).toBe(true)
      vi.advanceTimersByTime(8)

      const inputFrames = subscriptionSendBinary.mock.calls
        .map((call) => decodeTerminalStreamFrame(call[0]))
        .filter((frame) => frame?.opcode === TerminalStreamOpcode.Input)
      expect(inputFrames).toEqual([])
      expect(runtimeCall).toHaveBeenCalledWith({
        selector: 'env-1',
        method: 'terminal.send',
        params: {
          terminal: 'terminal-new',
          text: 'x',
          client: { id: expect.stringMatching(/^desktop:tab-1:pane:1:/), type: 'desktop' },
          viewport: { cols: 80, rows: 24 },
          claimViewport: true
        },
        timeoutMs: 15_000
      })
    } finally {
      vi.useRealTimers()
    }
  })

  it('closes a remote terminal created after the pane was destroyed', async () => {
    let resolveCreate: (value: unknown) => void = () => {}
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'terminal.create') {
        return new Promise((resolve) => {
          resolveCreate = resolve
        })
      }
      return Promise.resolve({ ok: true, result: {} })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    const connect = transport.connect({ url: '', callbacks: {} })
    transport.destroy?.()
    resolveCreate({ ok: true, result: { terminal: { handle: 'terminal-late' } } })
    await connect

    expect(runtimeCall).toHaveBeenCalledWith({
      selector: 'env-1',
      method: 'terminal.close',
      params: { terminal: 'terminal-late' },
      timeoutMs: 15_000
    })
  })

  it('passes activation intent when creating the remote runtime terminal', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      activate: true
    })

    await transport.connect({ url: '', callbacks: {} })

    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'terminal.create',
        params: expect.objectContaining({
          tabId: 'tab-1',
          leafId: 'pane:1',
          focus: false,
          presentation: 'background',
          activate: true
        })
      })
    )
  })

  it('scopes ephemeral setup terminals to the floating-terminal selector (#6789)', async () => {
    const { brandEphemeralSetupTerminalWorktreeId } =
      await import('../../../../shared/ephemeral-setup-terminal-worktree-id')
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: brandEphemeralSetupTerminalWorktreeId(
        'feature-wall-orchestration-skill-terminal'
      ),
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })

    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'terminal.create',
        params: expect.objectContaining({
          worktree: 'id:global-floating-terminal'
        })
      })
    )
  })

  it('passes startup command delivery when creating the remote runtime terminal', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      command: "codex 'linked issue context'",
      startupCommandDelivery: 'shell-ready'
    })

    await transport.connect({ url: '', callbacks: {} })

    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'terminal.create',
        params: expect.objectContaining({
          command: "codex 'linked issue context'",
          startupCommandDelivery: 'shell-ready'
        })
      })
    )
  })

  it('prefers connect-time launch metadata when creating the remote runtime terminal', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      command: "codex 'old'",
      launchConfig: { agentArgs: '--old', agentEnv: {} },
      launchToken: 'old-token',
      launchAgent: 'codex'
    })

    await transport.connect({
      url: '',
      command: "codex '--model' 'gpt-5' 'resume' 'session-1'",
      env: { CODEX_PROFILE: 'captured', ORCA_AGENT_LAUNCH_TOKEN: 'fresh-token' },
      launchConfig: {
        agentArgs: '--model gpt-5',
        agentEnv: { CODEX_PROFILE: 'captured' }
      },
      launchToken: 'fresh-token',
      launchAgent: 'codex',
      callbacks: {}
    })

    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        selector: 'env-1',
        method: 'terminal.create',
        params: expect.objectContaining({
          command: "codex '--model' 'gpt-5' 'resume' 'session-1'",
          env: { CODEX_PROFILE: 'captured', ORCA_AGENT_LAUNCH_TOKEN: 'fresh-token' },
          launchConfig: {
            agentArgs: '--model gpt-5',
            agentEnv: { CODEX_PROFILE: 'captured' }
          },
          launchToken: 'fresh-token',
          launchAgent: 'codex'
        })
      })
    )
  })

  it('activates pending host session mirrors instead of creating duplicate terminals', async () => {
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'session.tabs.activate') {
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'id:wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: 1,
            activeGroupId: 'group-1',
            activeTabId: 'host-tab-1::leaf-1',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-1',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-1',
                title: 'Terminal 1',
                isActive: true,
                status: 'pending-handle',
                terminal: null
              }
            ]
          }
        })
      }
      if (args.method === 'session.tabs.list') {
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'id:wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: 2,
            activeGroupId: 'group-1',
            activeTabId: 'host-tab-1::leaf-1',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-1',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-1',
                title: 'Terminal 1',
                isActive: true,
                status: 'ready',
                terminal: 'terminal-1'
              }
            ]
          }
        })
      }
      return Promise.resolve({ ok: true, result: { terminal: { handle: 'duplicate-terminal' } } })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-host-tab-1',
      leafId: 'leaf-1'
    })

    const result = await transport.connect({ url: '', callbacks: {} })

    expect(result).toEqual({ id: 'remote:env-1@@terminal-1', replay: '' })
    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'session.tabs.activate',
        params: { worktree: 'id:wt-1', tabId: 'host-tab-1', leafId: 'leaf-1' }
      })
    )
    expect(runtimeCall).not.toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'terminal.create'
      })
    )
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    expect(latestSubscribePayload()).toMatchObject({
      terminal: 'terminal-1',
      viewport: { cols: 80, rows: 24 }
    })
  })

  it('activates the requested split leaf for pending host session mirrors', async () => {
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'session.tabs.activate') {
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'id:wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: 1,
            activeGroupId: 'group-1',
            activeTabId: 'host-tab-1::leaf-2',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-1',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-1',
                title: 'Terminal 1',
                isActive: false,
                status: 'pending-handle',
                terminal: null
              },
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-2',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-2',
                title: 'Terminal 2',
                isActive: true,
                status: 'pending-handle',
                terminal: null
              }
            ]
          }
        })
      }
      if (args.method === 'session.tabs.list') {
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'id:wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: 2,
            activeGroupId: 'group-1',
            activeTabId: 'host-tab-1::leaf-2',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-1',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-1',
                title: 'Terminal 1',
                isActive: false,
                status: 'ready',
                terminal: 'terminal-1'
              },
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-2',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-2',
                title: 'Terminal 2',
                isActive: true,
                status: 'ready',
                terminal: 'terminal-2'
              }
            ]
          }
        })
      }
      return Promise.resolve({ ok: true, result: { terminal: { handle: 'duplicate-terminal' } } })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-host-tab-1',
      leafId: 'leaf-2'
    })

    const result = await transport.connect({ url: '', callbacks: {} })

    expect(result).toEqual({ id: 'remote:env-1@@terminal-2', replay: '' })
    expect(runtimeCall).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'session.tabs.activate',
        params: { worktree: 'id:wt-1', tabId: 'host-tab-1', leafId: 'leaf-2' }
      })
    )
    expect(runtimeCall).not.toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'terminal.create'
      })
    )
  })

  it('does not attach a pending split leaf to a ready sibling', async () => {
    let listCount = 0
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'session.tabs.activate') {
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'id:wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: 1,
            activeGroupId: 'group-1',
            activeTabId: 'host-tab-1::leaf-2',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-1',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-1',
                title: 'Terminal 1',
                isActive: true,
                status: 'ready',
                terminal: 'terminal-1'
              },
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-2',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-2',
                title: 'Terminal 2',
                isActive: false,
                status: 'pending-handle',
                terminal: null
              }
            ]
          }
        })
      }
      if (args.method === 'session.tabs.list') {
        listCount += 1
        return Promise.resolve({
          ok: true,
          result: {
            worktree: 'id:wt-1',
            publicationEpoch: 'epoch-1',
            snapshotVersion: listCount + 1,
            activeGroupId: 'group-1',
            activeTabId: 'host-tab-1::leaf-2',
            activeTabType: 'terminal',
            tabs: [
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-1',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-1',
                title: 'Terminal 1',
                isActive: false,
                status: 'ready',
                terminal: 'terminal-1'
              },
              {
                type: 'terminal',
                id: 'host-tab-1::leaf-2',
                parentTabId: 'host-tab-1',
                leafId: 'leaf-2',
                title: 'Terminal 2',
                isActive: true,
                status: 'ready',
                terminal: 'terminal-2'
              }
            ]
          }
        })
      }
      return Promise.resolve({ ok: true, result: { terminal: { handle: 'duplicate-terminal' } } })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'web-terminal-host-tab-1',
      leafId: 'leaf-2'
    })

    const result = await transport.connect({ url: '', callbacks: {} })

    expect(result).toEqual({ id: 'remote:env-1@@terminal-2', replay: '' })
    expect(latestSubscribePayload()).toMatchObject({ terminal: 'terminal-2' })
  })

  it('stops polling when a requested split leaf disappears but siblings remain', async () => {
    vi.useFakeTimers()
    try {
      runtimeCall.mockImplementation((args) => {
        if (args.method === 'session.tabs.activate') {
          return Promise.resolve({
            ok: true,
            result: {
              worktree: 'id:wt-1',
              publicationEpoch: 'epoch-1',
              snapshotVersion: 1,
              activeGroupId: 'group-1',
              activeTabId: 'host-tab-1::leaf-2',
              activeTabType: 'terminal',
              tabs: [
                {
                  type: 'terminal',
                  id: 'host-tab-1::leaf-1',
                  parentTabId: 'host-tab-1',
                  leafId: 'leaf-1',
                  title: 'Terminal 1',
                  isActive: false,
                  status: 'ready',
                  terminal: 'terminal-1'
                },
                {
                  type: 'terminal',
                  id: 'host-tab-1::leaf-2',
                  parentTabId: 'host-tab-1',
                  leafId: 'leaf-2',
                  title: 'Terminal 2',
                  isActive: true,
                  status: 'pending-handle',
                  terminal: null
                }
              ]
            }
          })
        }
        if (args.method === 'session.tabs.list') {
          return Promise.resolve({
            ok: true,
            result: {
              worktree: 'id:wt-1',
              publicationEpoch: 'epoch-1',
              snapshotVersion: 2,
              activeGroupId: 'group-1',
              activeTabId: 'host-tab-1::leaf-1',
              activeTabType: 'terminal',
              tabs: [
                {
                  type: 'terminal',
                  id: 'host-tab-1::leaf-1',
                  parentTabId: 'host-tab-1',
                  leafId: 'leaf-1',
                  title: 'Terminal 1',
                  isActive: true,
                  status: 'ready',
                  terminal: 'terminal-1'
                }
              ]
            }
          })
        }
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'duplicate-terminal' } } })
      })
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const onError = vi.fn()
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'web-terminal-host-tab-1',
        leafId: 'leaf-2'
      })

      const connect = transport.connect({ url: '', callbacks: { onError } })
      await vi.advanceTimersByTimeAsync(150)

      await expect(connect).resolves.toBeUndefined()
      expect(onError).toHaveBeenCalledWith('Remote terminal was closed.')
      expect(
        runtimeCall.mock.calls.filter((call) => call[0].method === 'session.tabs.list')
      ).toHaveLength(1)
      await Promise.resolve()
      await Promise.resolve()
      expect(
        runtimeCall.mock.calls.some((call) => call[0].method.startsWith('session.tabs.close'))
      ).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not close a split parent when the requested leaf times out but a sibling is ready', async () => {
    vi.useFakeTimers()
    try {
      const splitSnapshot = {
        worktree: 'id:wt-1',
        publicationEpoch: 'epoch-1',
        snapshotVersion: 1,
        activeGroupId: 'group-1',
        activeTabId: 'host-tab-1::leaf-2',
        activeTabType: 'terminal',
        tabs: [
          {
            type: 'terminal',
            id: 'host-tab-1::leaf-1',
            parentTabId: 'host-tab-1',
            leafId: 'leaf-1',
            title: 'Terminal 1',
            isActive: false,
            status: 'ready',
            terminal: 'terminal-1'
          },
          {
            type: 'terminal',
            id: 'host-tab-1::leaf-2',
            parentTabId: 'host-tab-1',
            leafId: 'leaf-2',
            title: 'Terminal 2',
            isActive: true,
            status: 'pending-handle',
            terminal: null
          }
        ]
      }
      runtimeCall.mockImplementation((args) => {
        if (args.method === 'session.tabs.activate' || args.method === 'session.tabs.list') {
          return Promise.resolve({ ok: true, result: splitSnapshot })
        }
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'duplicate-terminal' } } })
      })
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const onError = vi.fn()
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'web-terminal-host-tab-1',
        leafId: 'leaf-2'
      })

      const connect = transport.connect({ url: '', callbacks: { onError } })
      await vi.advanceTimersByTimeAsync(15_000)

      await expect(connect).resolves.toBeUndefined()
      expect(onError).toHaveBeenCalledWith('Remote terminal was closed.')
      expect(
        runtimeCall.mock.calls.some((call) => call[0].method.startsWith('session.tabs.close'))
      ).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('stops polling without closing the host tab when a mirror never publishes a ready handle', async () => {
    vi.useFakeTimers()
    try {
      const pendingSnapshot = {
        worktree: 'id:wt-1',
        publicationEpoch: 'epoch-1',
        snapshotVersion: 1,
        activeGroupId: 'group-1',
        activeTabId: 'host-tab-1::leaf-1',
        activeTabType: 'terminal',
        tabs: [
          {
            type: 'terminal',
            id: 'host-tab-1::leaf-1',
            parentTabId: 'host-tab-1',
            leafId: 'leaf-1',
            title: 'Terminal 1',
            isActive: true,
            status: 'pending-handle',
            terminal: null
          }
        ]
      }
      runtimeCall.mockImplementation((args) => {
        if (args.method === 'session.tabs.activate' || args.method === 'session.tabs.list') {
          return Promise.resolve({ ok: true, result: pendingSnapshot })
        }
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'duplicate-terminal' } } })
      })
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const onError = vi.fn()
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'web-terminal-host-tab-1',
        leafId: 'leaf-1'
      })

      const connect = transport.connect({ url: '', callbacks: { onError } })
      await vi.advanceTimersByTimeAsync(15_000)

      await expect(connect).resolves.toBeUndefined()
      expect(onError).toHaveBeenCalledWith('Remote terminal was closed.')
      expect(runtimeCall).toHaveBeenCalledWith(
        expect.objectContaining({ method: 'session.tabs.activate' })
      )
      const listCalls = runtimeCall.mock.calls.filter(
        (call) => call[0].method === 'session.tabs.list'
      )
      expect(listCalls.length).toBeGreaterThan(0)
      expect(listCalls.length).toBeLessThanOrEqual(101)
      expect(runtimeCall).not.toHaveBeenCalledWith(
        expect.objectContaining({
          method: 'terminal.create'
        })
      )
      const closeCalls = runtimeCall.mock.calls.filter((call) =>
        String(call[0].method).startsWith('session.tabs.close')
      )
      expect(closeCalls).toEqual([])
    } finally {
      vi.useRealTimers()
    }
  })

  it('unsubscribes a remote terminal subscription that resolves after destroy', async () => {
    let resolveSubscribe: (value: {
      unsubscribe: () => void
      sendBinary: typeof subscriptionSendBinary
    }) => void = () => {}
    const unsubscribe = vi.fn()
    runtimeSubscribe.mockImplementation(
      (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
        subscriptionCallbacks = callbacks
        return new Promise<{ unsubscribe: () => void; sendBinary: typeof subscriptionSendBinary }>(
          (resolve) => {
            resolveSubscribe = (value) => {
              resolve(value)
              queueMicrotask(emitMultiplexReady)
            }
          }
        )
      }
    )
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    const connect = transport.connect({ url: '', callbacks: {} })
    await vi.waitFor(() => {
      expect(runtimeSubscribe).toHaveBeenCalled()
    })
    transport.destroy?.()
    resolveSubscribe({ unsubscribe, sendBinary: subscriptionSendBinary })
    await connect

    expect(unsubscribe).toHaveBeenCalled()
    expect(transport.getPtyId()).toBeNull()
  })

  it('delivers cleaned remote data before deferred title, bell, and OSC 9999 handlers', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onData = vi.fn()
    const onTitleChange = vi.fn()
    const onBell = vi.fn()
    const onAgentStatus = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      onTitleChange,
      onBell,
      onAgentStatus
    })

    await transport.connect({ url: '', callbacks: { onData } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()
    emitOutput(
      streamId,
      'before\x1b]9999;{"state":"working","prompt":"ship it","agentType":"codex"}\x07after\x1b]0;. Claude working\x07\x07'
    )

    expect(onData).toHaveBeenCalledWith(
      'beforeafter\x1b]0;. Claude working\x07\x07',
      expect.objectContaining({ seq: 1 })
    )
    await vi.waitFor(() =>
      expect(onAgentStatus).toHaveBeenCalledWith({
        state: 'working',
        prompt: 'ship it',
        agentType: 'codex'
      })
    )
    expect(onTitleChange).toHaveBeenCalledWith('. Claude working', '. Claude working')
    expect(onBell).toHaveBeenCalledTimes(1)
  })

  it('processes binary remote data chunks through the terminal parser', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onData = vi.fn()
    const onTitleChange = vi.fn()
    const onBell = vi.fn()
    const onAgentStatus = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      onTitleChange,
      onBell,
      onAgentStatus
    })

    await transport.connect({ url: '', callbacks: { onData } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()
    emitOutput(
      streamId,
      'before\x1b]9999;{"state":"working","prompt":"ship it","agentType":"codex"}\x07after'
    )

    expect(onData).toHaveBeenCalledWith('beforeafter', expect.objectContaining({ seq: 1 }))
    await vi.waitFor(() =>
      expect(onAgentStatus).toHaveBeenCalledWith({
        state: 'working',
        prompt: 'ship it',
        agentType: 'codex'
      })
    )
  })

  it('resubscribes without surfacing a PTY error when the remote runtime subscription closes', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onExit = vi.fn()
    const onDisconnect = vi.fn()
    const onPtyExit = vi.fn()
    const onError = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1',
      onPtyExit
    })

    await transport.connect({ url: '', callbacks: { onExit, onDisconnect, onError } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    subscriptionCallbacks?.onClose?.()

    expect(onExit).not.toHaveBeenCalled()
    expect(onDisconnect).not.toHaveBeenCalled()
    expect(onPtyExit).not.toHaveBeenCalled()
    expect(onError).not.toHaveBeenCalled()
    await vi.waitFor(() => expect(runtimeSubscribe).toHaveBeenCalledTimes(2))
  })

  it('releases pending claimed input when reconnect subscription fails', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onError = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })
    await transport.connect({ url: '', callbacks: { onError } })
    let rejectReconnect = (_error: Error): void => {}
    runtimeSubscribe.mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          rejectReconnect = reject
        })
    )

    subscriptionCallbacks?.onClose?.()
    await vi.waitFor(() => expect(runtimeSubscribe).toHaveBeenCalledTimes(2))
    expect(transport.claimViewport?.(101, 33)).toBe(true)
    const accepted = transport.sendInputAccepted?.('\x03')
    await Promise.resolve()
    rejectReconnect(new Error('reconnect failed'))

    await expect(accepted).resolves.toBe(false)
    expect(onError).toHaveBeenCalledWith('reconnect failed')
  })

  it('releases pending claimed input when the remote terminal ends', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })
    await transport.connect({ url: '', callbacks: {} })
    const { streamId } = latestSubscribePayload()

    expect(transport.claimViewport?.(101, 33)).toBe(true)
    const accepted = transport.sendInputAccepted?.('x')
    subscriptionCallbacks?.onResponse({
      ok: true,
      result: { type: 'end', streamId }
    })

    await expect(accepted).resolves.toBe(false)
  })

  it('retries when a replacement transport closes before its stream installs', async () => {
    const transportCallbacks: NonNullable<typeof subscriptionCallbacks>[] = []
    runtimeSubscribe.mockImplementation(
      async (_args: unknown, callbacks: NonNullable<typeof subscriptionCallbacks>) => {
        transportCallbacks.push(callbacks)
        subscriptionCallbacks = callbacks
        return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
      }
    )
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })
    const connected = transport.connect({ url: '', callbacks: {} })
    await vi.waitFor(() => expect(transportCallbacks).toHaveLength(1))
    transportCallbacks[0].onResponse({ ok: true, result: { type: 'ready' } })
    await connected

    transportCallbacks[0].onClose?.()
    await vi.waitFor(() => expect(transportCallbacks).toHaveLength(2))
    transportCallbacks[1].onResponse({ ok: true, result: { type: 'ready' } })
    transportCallbacks[1].onClose?.()

    await vi.waitFor(() => expect(transportCallbacks).toHaveLength(3))
    transport.destroy?.()
  })

  it('resubscribes with the latest pane viewport after the remote stream closes', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', cols: 80, rows: 24, callbacks: {} })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    expect(latestSubscribePayload().viewport).toEqual({ cols: 80, rows: 24 })

    expect(transport.resize(132, 43)).toBe(true)
    subscriptionCallbacks?.onClose?.()

    await vi.waitFor(() => expect(runtimeSubscribe).toHaveBeenCalledTimes(2))
    await vi.waitFor(() => {
      expect(latestSubscribePayload().viewport).toEqual({ cols: 132, rows: 43 })
    })
  })

  it('replays a viewport that changed during the subscribe round-trip once the stream is current', async () => {
    // Why: a resize landing while the subscribe is in flight takes the one-shot
    // RPC fallback, which is refresh-only (no leak) and no-ops before the stream
    // floor exists. The transport must replay the latest viewport over the
    // now-current stream so the PTY does not stall at the subscribe-time width.
    // Hold the multiplex "ready" to keep the round-trip open across the resize.
    runtimeSubscribe.mockImplementation(
      async (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
        subscriptionCallbacks = callbacks
        return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
      }
    )
    // Drain microtasks WITHOUT advancing timers, so the 33ms viewport batcher
    // cannot fire — the replayed Resize frame must come from the round-trip
    // flush alone (this test fails if that flush is removed).
    const flushMicrotasks = async (): Promise<void> => {
      for (let i = 0; i < 20; i += 1) {
        await Promise.resolve()
      }
    }

    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    transport.attach({
      existingPtyId: 'remote:terminal-1',
      cols: 80,
      rows: 24,
      callbacks: {}
    })
    await vi.waitFor(() => expect(runtimeSubscribe).toHaveBeenCalled())

    // Resize while the stream is not yet current (subscribe still pending).
    expect(transport.resize(132, 43)).toBe(true)

    // Release readiness and drain the resolution chain by microtasks only.
    emitMultiplexReady()
    await flushMicrotasks()

    // The Subscribe frame still carries the subscribe-time viewport...
    expect(latestSubscribePayload().viewport).toEqual({ cols: 80, rows: 24 })
    // ...and the newer viewport is replayed as a Resize frame over the stream,
    // before the batcher's 33ms timer could have produced it.
    const resizeFrame = latestFrameForOpcode(TerminalStreamOpcode.Resize)
    expect(resizeFrame && decodeTerminalStreamJson(resizeFrame.payload)).toEqual({
      cols: 132,
      rows: 43
    })

    transport.destroy?.()
  })

  it('replays a claim before input typed during the subscribe round-trip', async () => {
    vi.useFakeTimers()
    try {
      runtimeSubscribe.mockImplementation(
        async (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
          subscriptionCallbacks = callbacks
          return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
        }
      )
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      transport.attach({
        existingPtyId: 'remote:terminal-1',
        cols: 80,
        rows: 24,
        callbacks: {}
      })
      await vi.waitFor(() => expect(runtimeSubscribe).toHaveBeenCalled())
      expect(transport.claimViewport?.(101, 33)).toBe(true)
      expect(transport.sendInput('x')).toBe(true)
      await vi.advanceTimersByTimeAsync(8)
      expect(runtimeCall).not.toHaveBeenCalledWith(
        expect.objectContaining({ method: 'terminal.send' })
      )

      emitMultiplexReady()
      await vi.waitFor(() => {
        const opcodes = subscriptionSendBinary.mock.calls
          .map((call) => decodeTerminalStreamFrame(call[0])?.opcode)
          .filter((opcode) => opcode !== undefined)
        expect(opcodes).toEqual([
          TerminalStreamOpcode.Subscribe,
          TerminalStreamOpcode.ClaimViewport,
          TerminalStreamOpcode.Resize,
          TerminalStreamOpcode.Input
        ])
      })
      transport.destroy?.()
    } finally {
      vi.useRealTimers()
    }
  })

  it('coalesces rapid remote terminal input before sending it to the runtime', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      const { streamId } = latestSubscribePayload()
      runtimeCall.mockClear()
      subscriptionSendBinary.mockClear()

      expect(transport.sendInput('a')).toBe(true)
      expect(transport.sendInput('b')).toBe(true)
      expect(runtimeCall).not.toHaveBeenCalled()

      await vi.runOnlyPendingTimersAsync()

      expect(runtimeCall).not.toHaveBeenCalled()
      expect(subscriptionSendBinary).toHaveBeenCalledTimes(1)
      const frame = decodeTerminalStreamFrame(subscriptionSendBinary.mock.calls[0][0])
      expect(frame?.opcode).toBe(TerminalStreamOpcode.Input)
      expect(frame?.streamId).toBe(streamId)
      expect(frame ? decodeTerminalStreamText(frame.payload) : '').toBe('ab')
    } finally {
      vi.useRealTimers()
    }
  })

  it('sends coalesced terminal input as binary frames once the stream is established', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      const { streamId } = latestSubscribePayload()
      runtimeCall.mockClear()
      subscriptionSendBinary.mockClear()

      expect(transport.sendInput('a')).toBe(true)
      expect(transport.sendInput('b')).toBe(true)
      await vi.runOnlyPendingTimersAsync()

      expect(runtimeCall).not.toHaveBeenCalled()
      expect(subscriptionSendBinary).toHaveBeenCalledTimes(1)
      const frame = decodeTerminalStreamFrame(subscriptionSendBinary.mock.calls[0][0])
      expect(frame?.opcode).toBe(TerminalStreamOpcode.Input)
      expect(frame?.streamId).toBe(streamId)
      expect(frame ? decodeTerminalStreamText(frame.payload) : '').toBe('ab')
    } finally {
      vi.useRealTimers()
    }
  })

  it('does not coalesce large remote terminal input chunks above the terminal ceiling', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      const { streamId } = latestSubscribePayload()
      runtimeCall.mockClear()
      subscriptionSendBinary.mockClear()

      const chunk = 'x'.repeat(TERMINAL_INPUT_CHUNK_MAX_BYTES)
      expect(transport.sendInput(chunk)).toBe(true)
      expect(subscriptionSendBinary).toHaveBeenCalledTimes(1)
      let frame = decodeTerminalStreamFrame(subscriptionSendBinary.mock.calls[0][0])
      expect(frame?.opcode).toBe(TerminalStreamOpcode.Input)
      expect(frame?.streamId).toBe(streamId)
      expect(frame ? decodeTerminalStreamText(frame.payload) : '').toBe(chunk)

      expect(transport.sendInput('tail')).toBe(true)
      await vi.runOnlyPendingTimersAsync()

      expect(runtimeCall).not.toHaveBeenCalled()
      expect(subscriptionSendBinary).toHaveBeenCalledTimes(2)
      frame = decodeTerminalStreamFrame(subscriptionSendBinary.mock.calls[1][0])
      expect(frame?.opcode).toBe(TerminalStreamOpcode.Input)
      expect(frame?.streamId).toBe(streamId)
      expect(frame ? decodeTerminalStreamText(frame.payload) : '').toBe('tail')
    } finally {
      vi.useRealTimers()
    }
  })

  it('returns runtime acceptance for acknowledged terminal input', async () => {
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'terminal.create') {
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
      }
      if (args.method === 'terminal.send') {
        return Promise.resolve({
          ok: true,
          result: { send: { handle: 'terminal-1', accepted: true, bytesWritten: 1 } }
        })
      }
      return Promise.resolve({ ok: true, result: {} })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })

    await expect(transport.sendInputAccepted?.('\x03')).resolves.toBe(true)
    expect(runtimeCall).toHaveBeenCalledWith({
      selector: 'env-1',
      method: 'terminal.send',
      params: {
        terminal: 'terminal-1',
        text: '\x03',
        client: { id: expect.stringMatching(/^desktop:tab-1:pane:1:/), type: 'desktop' },
        viewport: { cols: 80, rows: 24 },
        claimViewport: true
      },
      timeoutMs: 15_000
    })
  })

  it('preserves queued remote input order before acknowledged terminal input', async () => {
    vi.useFakeTimers()
    try {
      runtimeCall.mockImplementation((args) => {
        if (args.method === 'terminal.create') {
          return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
        }
        if (args.method === 'terminal.send') {
          return Promise.resolve({
            ok: true,
            result: { send: { handle: 'terminal-1', accepted: true, bytesWritten: 2 } }
          })
        }
        return Promise.resolve({ ok: true, result: {} })
      })
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      subscriptionSendBinary.mockClear()

      expect(transport.sendInput('a')).toBe(true)
      await expect(transport.sendInputAccepted?.('\x03')).resolves.toBe(true)
      await vi.runOnlyPendingTimersAsync()

      expect(runtimeCall).toHaveBeenCalledWith({
        selector: 'env-1',
        method: 'terminal.send',
        params: {
          terminal: 'terminal-1',
          text: 'a\x03',
          client: { id: expect.stringMatching(/^desktop:tab-1:pane:1:/), type: 'desktop' },
          viewport: { cols: 80, rows: 24 },
          claimViewport: true
        },
        timeoutMs: 15_000
      })
      expect(subscriptionSendBinary).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })

  it('returns false when acknowledged terminal input is rejected by the runtime', async () => {
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'terminal.create') {
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
      }
      if (args.method === 'terminal.send') {
        return Promise.resolve({
          ok: true,
          result: { send: { handle: 'terminal-1', accepted: false, bytesWritten: 0 } }
        })
      }
      return Promise.resolve({ ok: true, result: {} })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })

    await expect(transport.sendInputAccepted?.('\x03')).resolves.toBe(false)
  })

  it('splits large acknowledged remote input before terminal.send RPCs', async () => {
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'terminal.create') {
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
      }
      if (args.method === 'terminal.send') {
        return Promise.resolve({
          ok: true,
          result: {
            send: {
              handle: 'terminal-1',
              accepted: true,
              bytesWritten: args.params.text.length
            }
          }
        })
      }
      return Promise.resolve({ ok: true, result: {} })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })

    const chunk = '😀'.repeat(TERMINAL_INPUT_CHUNK_MAX_BYTES / 4)
    await expect(transport.sendInputAccepted?.(`${chunk}tail`)).resolves.toBe(true)

    const sendCalls = runtimeCall.mock.calls.filter((call) => call[0].method === 'terminal.send')
    expect(sendCalls).toHaveLength(2)
    expect(sendCalls[0]?.[0].params.text).toBe(chunk)
    expect(sendCalls[1]?.[0].params.text).toBe('tail')
  })

  it('yields while validating accepted large acknowledged remote input before terminal.send RPCs', async () => {
    vi.useFakeTimers()
    try {
      runtimeCall.mockImplementation((args) => {
        if (args.method === 'terminal.create') {
          return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
        }
        if (args.method === 'terminal.send') {
          return Promise.resolve({
            ok: true,
            result: {
              send: {
                handle: 'terminal-1',
                accepted: true,
                bytesWritten: args.params.text.length
              }
            }
          })
        }
        return Promise.resolve({ ok: true, result: {} })
      })
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })
      const text = 'é'.repeat(CLIPBOARD_TEXT_MEASURE_YIELD_CODE_UNITS + 1)

      await transport.connect({ url: '', callbacks: {} })
      runtimeCall.mockClear()

      const accepted = transport.sendInputAccepted?.(text)
      await Promise.resolve()

      expect(runtimeCall).not.toHaveBeenCalled()

      await vi.runAllTimersAsync()

      await expect(accepted).resolves.toBe(true)
      const sendTexts = runtimeCall.mock.calls
        .filter((call) => call[0].method === 'terminal.send')
        .map((call) => call[0].params.text)
      expect(sendTexts.join('')).toBe(text)
    } finally {
      vi.useRealTimers()
    }
  })

  it('stops large acknowledged remote input after a rejected chunk', async () => {
    const firstChunk = 'x'.repeat(TERMINAL_INPUT_CHUNK_MAX_BYTES)
    const rejectedChunk = `tail${'y'.repeat(TERMINAL_INPUT_CHUNK_MAX_BYTES - 4)}`
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'terminal.create') {
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
      }
      if (args.method === 'terminal.send') {
        return Promise.resolve({
          ok: true,
          result: {
            send: {
              handle: 'terminal-1',
              accepted: args.params.text !== rejectedChunk,
              bytesWritten: args.params.text === rejectedChunk ? 0 : args.params.text.length
            }
          }
        })
      }
      return Promise.resolve({ ok: true, result: {} })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })

    await expect(transport.sendInputAccepted?.(`${firstChunk}${rejectedChunk}after`)).resolves.toBe(
      false
    )

    const sendTexts = runtimeCall.mock.calls
      .filter((call) => call[0].method === 'terminal.send')
      .map((call) => call[0].params.text)
    expect(sendTexts).toEqual([firstChunk, rejectedChunk])
  })

  it('rejects oversized acknowledged remote input before runtime RPCs', async () => {
    runtimeCall.mockImplementation((args) => {
      if (args.method === 'terminal.create') {
        return Promise.resolve({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
      }
      return Promise.resolve({ ok: true, result: {} })
    })
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })

    await transport.connect({ url: '', callbacks: {} })
    runtimeCall.mockClear()

    await expect(
      transport.sendInputAccepted?.('😀'.repeat(Math.floor(TERMINAL_INPUT_MAX_BYTES / 4) + 1))
    ).resolves.toBe(false)
    expect(runtimeCall).not.toHaveBeenCalled()
  })

  it('preserves literal LF input when sending remote PTY binary frames', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      const { streamId } = latestSubscribePayload()
      runtimeCall.mockClear()
      subscriptionSendBinary.mockClear()

      expect(transport.sendInput('echo one\necho two\r\n')).toBe(true)
      await vi.runOnlyPendingTimersAsync()

      expect(runtimeCall).not.toHaveBeenCalled()
      expect(subscriptionSendBinary).toHaveBeenCalledTimes(1)
      const frame = decodeTerminalStreamFrame(subscriptionSendBinary.mock.calls[0][0])
      expect(frame?.opcode).toBe(TerminalStreamOpcode.Input)
      expect(frame?.streamId).toBe(streamId)
      expect(frame ? decodeTerminalStreamText(frame.payload) : '').toBe('echo one\necho two\r\n')
    } finally {
      vi.useRealTimers()
    }
  })

  it('coalesces rapid remote viewport updates before sending the latest size', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      const { streamId } = latestSubscribePayload()
      runtimeCall.mockClear()
      subscriptionSendBinary.mockClear()

      expect(transport.resize(80, 24)).toBe(true)
      expect(transport.resize(120, 40)).toBe(true)
      expect(runtimeCall).not.toHaveBeenCalled()

      await vi.runOnlyPendingTimersAsync()

      expect(runtimeCall).not.toHaveBeenCalled()
      expect(subscriptionSendBinary).toHaveBeenCalledTimes(1)
      const frame = decodeTerminalStreamFrame(subscriptionSendBinary.mock.calls[0][0])
      expect(frame?.opcode).toBe(TerminalStreamOpcode.Resize)
      expect(frame?.streamId).toBe(streamId)
      expect(frame ? decodeTerminalStreamJson(frame.payload) : null).toEqual({
        cols: 120,
        rows: 40
      })
    } finally {
      vi.useRealTimers()
    }
  })

  it('sends an activity claim before the user input it sizes', async () => {
    vi.useFakeTimers()
    try {
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-1',
        leafId: 'pane:1'
      })

      await transport.connect({ url: '', callbacks: {} })
      const { streamId } = latestSubscribePayload()
      subscriptionSendBinary.mockClear()

      expect(transport.claimViewport?.(101, 33)).toBe(true)
      expect(transport.sendInput('x')).toBe(true)
      await vi.runOnlyPendingTimersAsync()

      const frames = subscriptionSendBinary.mock.calls.map((call) =>
        decodeTerminalStreamFrame(call[0])
      )
      expect(frames.map((frame) => frame?.opcode)).toEqual([
        TerminalStreamOpcode.ClaimViewport,
        TerminalStreamOpcode.Resize,
        TerminalStreamOpcode.Input
      ])
      expect(frames[0]?.streamId).toBe(streamId)
      expect(frames[0] ? decodeTerminalStreamJson(frames[0].payload) : null).toEqual({
        cols: 101,
        rows: 33
      })
    } finally {
      vi.useRealTimers()
    }
  })

  it('replays remote scrollback through the parser without firing stale attention events', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onReplayData = vi.fn()
    const onTitleChange = vi.fn()
    const onBell = vi.fn()
    const onAgentStatus = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      onTitleChange,
      onBell,
      onAgentStatus
    })

    await transport.connect({ url: '', callbacks: { onReplayData } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()
    emitSnapshot(
      streamId,
      'before\x1b]9999;{"state":"working","prompt":"old","agentType":"codex"}\x07after\x1b]0;Remote title\x07\x07'
    )

    expect(onReplayData).toHaveBeenCalledWith('beforeafter\x1b]0;Remote title\x07\x07')
    await vi.waitFor(() =>
      expect(onTitleChange).toHaveBeenCalledWith('Remote title', 'Remote title')
    )
    expect(onAgentStatus).not.toHaveBeenCalled()
    expect(onBell).not.toHaveBeenCalled()
  })

  it('replays binary snapshot chunks without firing stale attention events', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onReplayData = vi.fn()
    const onTitleChange = vi.fn()
    const onBell = vi.fn()
    const onAgentStatus = vi.fn()
    const onConnect = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      onTitleChange,
      onBell,
      onAgentStatus
    })

    await transport.connect({ url: '', callbacks: { onReplayData, onConnect } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()
    emitSnapshot(
      streamId,
      'before\x1b]9999;{"state":"working","prompt":"old","agentType":"codex"}\x07after'
    )

    expect(onReplayData).toHaveBeenCalledWith('beforeafter')
    expect(onAgentStatus).not.toHaveBeenCalled()
    expect(onBell).not.toHaveBeenCalled()
    expect(onConnect).toHaveBeenCalled()
  })

  it('resolves explicit binary snapshot requests without replaying into xterm', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onReplayData = vi.fn()
    const onData = vi.fn()
    const onConnect = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1'
    })

    await transport.connect({ url: '', callbacks: { onReplayData, onData, onConnect } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()
    emitSnapshot(streamId, 'initial')
    expect(onReplayData).toHaveBeenCalledWith('initial')
    expect(onConnect).toHaveBeenCalled()

    const snapshotPromise = transport.serializeBuffer?.({ scrollbackRows: 5000 })
    const snapshotRequestFrame = latestFrameForOpcode(TerminalStreamOpcode.SnapshotRequest)
    const snapshotRequestPayload = snapshotRequestFrame
      ? decodeTerminalStreamJson<{ requestId?: number; scrollbackRows?: number }>(
          snapshotRequestFrame.payload
        )
      : null
    expect(snapshotRequestFrame?.streamId).toBe(streamId)
    expect(snapshotRequestPayload).toMatchObject({ requestId: 1, scrollbackRows: 5000 })

    emitSnapshotFrame(
      streamId,
      TerminalStreamOpcode.SnapshotStart,
      encodeTerminalStreamJson({
        kind: 'scrollback',
        requestId: snapshotRequestPayload?.requestId,
        cols: 132,
        rows: 43,
        seq: 17,
        source: 'headless'
      })
    )
    emitSnapshotFrame(
      streamId,
      TerminalStreamOpcode.SnapshotChunk,
      encodeTerminalStreamText('requested snapshot')
    )
    emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotEnd, new Uint8Array())

    await expect(snapshotPromise).resolves.toEqual({
      data: 'requested snapshot',
      cols: 132,
      rows: 43,
      seq: 17,
      source: 'headless'
    })
    expect(onReplayData).toHaveBeenCalledTimes(1)
    expect(onData).not.toHaveBeenCalledWith('requested snapshot', expect.anything())
  })

  it('keeps initial replay separate from in-flight explicit binary snapshot requests', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onReplayData = vi.fn()
    const onConnect = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1'
    })

    await transport.connect({ url: '', callbacks: { onReplayData, onConnect } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()

    const snapshotPromise = transport.serializeBuffer?.({ scrollbackRows: 5000 })
    const snapshotRequestFrame = latestFrameForOpcode(TerminalStreamOpcode.SnapshotRequest)
    const snapshotRequestPayload = snapshotRequestFrame
      ? decodeTerminalStreamJson<{ requestId?: number }>(snapshotRequestFrame.payload)
      : null
    expect(snapshotRequestPayload?.requestId).toBe(1)

    emitSnapshot(streamId, 'initial replay')
    expect(onReplayData).toHaveBeenCalledWith('initial replay')
    expect(onConnect).toHaveBeenCalled()

    emitSnapshotFrame(
      streamId,
      TerminalStreamOpcode.SnapshotStart,
      encodeTerminalStreamJson({
        kind: 'scrollback',
        requestId: snapshotRequestPayload?.requestId,
        cols: 100,
        rows: 20
      })
    )
    emitSnapshotFrame(
      streamId,
      TerminalStreamOpcode.SnapshotChunk,
      encodeTerminalStreamText('requested replay')
    )
    emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotEnd, new Uint8Array())

    await expect(snapshotPromise).resolves.toEqual({
      data: 'requested replay',
      cols: 100,
      rows: 20,
      seq: undefined,
      source: undefined
    })
    expect(onReplayData).toHaveBeenCalledTimes(1)
  })

  it('bounds oversized binary snapshots without closing the live stream', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const onReplayData = vi.fn()
    const onData = vi.fn()
    const onError = vi.fn()
    const onConnect = vi.fn()
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1'
    })

    await transport.connect({ url: '', callbacks: { onReplayData, onData, onError, onConnect } })
    await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
    const { streamId } = latestSubscribePayload()

    emitSnapshotFrame(
      streamId,
      TerminalStreamOpcode.SnapshotStart,
      encodeTerminalStreamJson({ kind: 'scrollback' })
    )
    emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotChunk, new Uint8Array(1024 * 1024))
    emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotChunk, new Uint8Array(1024 * 1024))
    emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotChunk, new Uint8Array(1))
    emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotEnd, new Uint8Array())
    emitOutput(streamId, 'live-after-overflow')

    expect(onReplayData).not.toHaveBeenCalled()
    // Why: an oversized snapshot is skipped but live output continues, so the
    // transport classifies it as benign and never surfaces a fatal red banner.
    expect(onError).not.toHaveBeenCalled()
    expect(onConnect).toHaveBeenCalled()
    expect(onData).toHaveBeenCalledWith('live-after-overflow', expect.objectContaining({ seq: 1 }))
  })

  describe('federated replay geometry (fed §2.4)', () => {
    // A real terminal-leaf UUID so the transport self-registers in the remote
    // federated-search registry (makePaneKey requires a UUID leaf).
    const LEAF = '33333333-3333-4333-8333-333333333333'

    function emitAnchoredSnapshot(
      streamId: number,
      data: string,
      info: { cols: number; rows: number; hostRowAnchor?: number; anchorGen?: number }
    ): void {
      emitSnapshotFrame(
        streamId,
        TerminalStreamOpcode.SnapshotStart,
        encodeTerminalStreamJson(info)
      )
      emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotChunk, encodeTerminalStreamText(data))
      emitSnapshotFrame(streamId, TerminalStreamOpcode.SnapshotEnd, new Uint8Array())
    }

    // Drives the REAL asynchronous replay path (fed §2.4): onReplayData queues an
    // async buffer mutation the way production's replay-write queue does, and
    // awaitReplayApplied resolves only after it settles. readClientReplayGeometry
    // reads the LIVE simulated buffer, so a synchronous freeze in onSnapshot would
    // capture the PRE-replay depth — the exact bug these tests pin. Call
    // setReplayedBaseY(depth) before each emit: that is the scrollback depth the
    // engine buffer reaches once THAT snapshot's clear+replay drain applies.
    async function attachedTransport(initialBaseY: number, cols = 80) {
      const rows = 24
      const buffer = { baseY: initialBaseY }
      let nextReplayedBaseY = initialBaseY
      let replayQueue = Promise.resolve()
      const readClientReplayGeometry = (): { baseY: number; rows: number; cols: number } => ({
        baseY: buffer.baseY,
        rows,
        cols
      })
      const awaitReplayApplied = (): Promise<void> => replayQueue
      const onReplayData = (): void => {
        const applied = nextReplayedBaseY
        replayQueue = replayQueue.then(async () => {
          // Two async turns so a synchronous freeze cannot observe the result.
          await Promise.resolve()
          await Promise.resolve()
          buffer.baseY = applied
        })
      }
      const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
      const transport = createRemoteRuntimePtyTransport('env-1', {
        worktreeId: 'wt-1',
        tabId: 'tab-geo',
        leafId: LEAF,
        readClientReplayGeometry,
        awaitReplayApplied
      })
      transport.attach({
        existingPtyId: 'remote:terminal-1',
        cols,
        rows,
        callbacks: { onReplayData }
      })
      await vi.waitFor(() => expect(subscriptionSendBinary).toHaveBeenCalled())
      return {
        transport,
        setReplayedBaseY: (value: number): void => {
          nextReplayedBaseY = value
        }
      }
    }

    const STABLE_ANCHOR = { hostRowAnchor: 100, anchorGen: 7, anchorHostCols: 80 }

    function remapAgainstFrozenGeometry(transport: PtyTransport, matchHostRow: number) {
      const geometry = transport.getReplayedSearchGeometry?.()
      if (!geometry) {
        throw new Error('expected frozen replay geometry')
      }
      return remapRemoteSearchRow({
        matchHostRow,
        responseAnchor: STABLE_ANCHOR,
        replayedAnchor: geometry.replayedAnchor,
        replayOriginRow: geometry.replayOriginRow,
        replayedRowCount: geometry.replayedRowCount,
        clientCols: geometry.clientCols
      })
    }

    it('fresh attach: an in-window replayed-history match is NOT demoted to inline-only', async () => {
      // Fresh engine buffer (baseY 0). The replayed snapshot carries 40 history
      // rows, so once its drain applies the buffer is 40 + 24 = 64 rows deep.
      const { transport, setReplayedBaseY } = await attachedTransport(0)
      setReplayedBaseY(40)
      const { streamId } = latestSubscribePayload()
      emitAnchoredSnapshot(streamId, 'restored history', {
        cols: 80,
        rows: 24,
        hostRowAnchor: 100,
        anchorGen: 7
      })

      // Post-drain: geometry reflects the 64-row replayed window, not the empty
      // pre-replay buffer (a synchronous read would have frozen replayedRowCount
      // at 24 and demoted the match below).
      await vi.waitFor(() => {
        expect(transport.getReplayedSearchGeometry?.()).toEqual({
          replayedAnchor: { hostRowAnchor: 100, anchorGen: 7 },
          replayOriginRow: 0,
          replayedRowCount: 64,
          clientCols: 80
        })
      })

      // Host row 150 = offset 50, INSIDE the 64-row replayed window: it must jump
      // in-window, not fall out (the pre-fix under-count made offset 50 ≥ 24 and
      // demoted this to deeper-history inline-only).
      expect(remapAgainstFrozenGeometry(transport, 150)).toEqual({
        kind: 'in-window',
        clientRow: 50,
        approximate: false
      })
      transport.destroy?.()
    })

    it('reconnect vs a deep prior buffer: a post-window host row is not accepted as in-window', async () => {
      // Reconnect over a DEEP prior buffer (baseY 1000 from the previous session).
      // The new snapshot replays a SHORTER window; its clear+replay resets the
      // buffer to 40 + 24 = 64 rows.
      const { transport, setReplayedBaseY } = await attachedTransport(1000)
      setReplayedBaseY(40)
      const { streamId } = latestSubscribePayload()
      emitAnchoredSnapshot(streamId, 'reconnect window', {
        cols: 80,
        rows: 24,
        hostRowAnchor: 100,
        anchorGen: 7
      })

      // Post-drain: the window is 64 rows — NOT the 1024 a synchronous read of the
      // deep prior buffer would have frozen.
      await vi.waitFor(() => {
        expect(transport.getReplayedSearchGeometry?.()?.replayedRowCount).toBe(64)
      })

      // Host row 600 = offset 500 is PAST the 64-row replayed window (post-snapshot
      // live output). It must stay out-of-window; the pre-fix over-count (1024)
      // accepted it and jumped to bogus client row 500.
      expect(remapAgainstFrozenGeometry(transport, 600)).toEqual({ kind: 'out-of-window' })

      // A row genuinely inside the window still lands on the CORRECT client row.
      expect(remapAgainstFrozenGeometry(transport, 130)).toEqual({
        kind: 'in-window',
        clientRow: 30,
        approximate: false
      })
      transport.destroy?.()
    })

    it('has no in-window geometry before any anchored snapshot is replayed', async () => {
      const { transport } = await attachedTransport(10)
      // Subscribed, but no snapshot yet → the multiplexer holds no replayed anchor.
      expect(transport.getReplayedSearchGeometry?.()).toBeNull()
      transport.destroy?.()
    })

    it('an anchor-less recovery replay (skew) clears the in-window geometry', async () => {
      const { transport, setReplayedBaseY } = await attachedTransport(0)
      setReplayedBaseY(40)
      const { streamId } = latestSubscribePayload()
      emitAnchoredSnapshot(streamId, 'history', {
        cols: 80,
        rows: 24,
        hostRowAnchor: 100,
        anchorGen: 7
      })
      await vi.waitFor(() => expect(transport.getReplayedSearchGeometry?.()).not.toBeNull())

      // A server-pushed recovery snapshot with NO anchor: the multiplexer replaces
      // engine state and drops its replayed anchor (the window it described is
      // gone), so the frozen geometry no longer reconciles → inline-only.
      emitAnchoredSnapshot(streamId, 'recovered screen', { cols: 80, rows: 24 })
      expect(transport.getReplayedSearchGeometry?.()).toBeNull()
      transport.destroy?.()
    })

    it('re-freezes geometry when a new-generation snapshot is replayed', async () => {
      const { transport, setReplayedBaseY } = await attachedTransport(0)
      const { streamId } = latestSubscribePayload()
      setReplayedBaseY(40)
      emitAnchoredSnapshot(streamId, 'gen7', {
        cols: 80,
        rows: 24,
        hostRowAnchor: 100,
        anchorGen: 7
      })
      // The gen-8 recovery snapshot replays a deeper window (200 scrollback rows).
      setReplayedBaseY(200)
      emitAnchoredSnapshot(streamId, 'gen8', {
        cols: 80,
        rows: 24,
        hostRowAnchor: 500,
        anchorGen: 8
      })
      // Only the LATEST snapshot's post-drain freeze wins (freeze-seq guard).
      await vi.waitFor(() => {
        expect(transport.getReplayedSearchGeometry?.()).toEqual({
          replayedAnchor: { hostRowAnchor: 500, anchorGen: 8 },
          replayOriginRow: 0,
          replayedRowCount: 224,
          clientCols: 80
        })
      })
      transport.destroy?.()
    })

    it('registers a live binding in the remote federated-pane registry and unregisters on destroy', async () => {
      const { transport, setReplayedBaseY } = await attachedTransport(0)
      const { streamId } = latestSubscribePayload()
      setReplayedBaseY(40)
      emitAnchoredSnapshot(streamId, 'history', {
        cols: 80,
        rows: 24,
        hostRowAnchor: 100,
        anchorGen: 7
      })
      await vi.waitFor(() => expect(transport.getReplayedSearchGeometry?.()).not.toBeNull())
      // Same post-reset module graph as the transport, so it sees the registration.
      const { listRemoteFederatedPaneBindings } = await import(
        '@/lib/federated-search/remote-federated-pane-registry'
      )
      const entries = listRemoteFederatedPaneBindings()
      const entry = entries.find((e) => e.paneKey === `tab-geo:${LEAF}`)
      expect(entry).toBeDefined()
      expect(entry?.binding.environmentId()).toBe('env-1')
      expect(entry?.binding.hostTerminalId()).toBe('terminal-1')
      expect(entry?.binding.sessionId()).toBe('remote:env-1@@terminal-1')
      expect(entry?.binding.replayGeometry()).toEqual({
        replayedAnchor: { hostRowAnchor: 100, anchorGen: 7 },
        replayOriginRow: 0,
        replayedRowCount: 64,
        clientCols: 80
      })

      transport.destroy?.()
      expect(
        listRemoteFederatedPaneBindings().some((e) => e.paneKey === `tab-geo:${LEAF}`)
      ).toBe(false)
    })
  })
})
