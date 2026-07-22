import { beforeEach, describe, expect, it, vi } from 'vitest'
import { NATIVE_CHAT_SUBMIT_DELAY_MS } from '../../../../shared/native-chat-answer-stepping'
import { BRACKETED_PASTE_END, BRACKETED_PASTE_START } from './terminal-bracketed-paste'

const mocks = vi.hoisted(() => ({
  recordTerminalUserInputForLeaf: vi.fn(),
  enqueueNativeChatPtySend: vi.fn(),
  enqueueDelays: [] as number[]
}))

vi.mock('./terminal-input-activity', () => ({
  recordTerminalUserInputForLeaf: mocks.recordTerminalUserInputForLeaf
}))

vi.mock('@/lib/connection-context', () => ({
  getConnectionId: () => null
}))

vi.mock('./terminal-paste-ssh-platform', () => ({
  getTerminalPasteSshRemotePlatform: () => null
}))

vi.mock('../native-chat/native-chat-pty-send-queue', () => ({
  enqueueNativeChatPtySend: mocks.enqueueNativeChatPtySend
}))

import { sendComposeBoxDraft } from './terminal-compose-box-send'

type TestPane = {
  id: number
  leafId: string
  terminal: {
    focus: ReturnType<typeof vi.fn>
    paste: ReturnType<typeof vi.fn>
    input: ReturnType<typeof vi.fn>
    options: { ignoreBracketedPasteMode: boolean }
    modes: { bracketedPasteMode: boolean }
  }
  atermController: { encodeKeyForHost: (key: string, mods: number) => string | null } | null
}

function makePane(options: { bracketed?: boolean; encodedEnter?: string | null } = {}): TestPane {
  return {
    id: 1,
    leafId: 'leaf-1',
    terminal: {
      focus: vi.fn(),
      paste: vi.fn(),
      input: vi.fn(),
      options: { ignoreBracketedPasteMode: false },
      modes: { bracketedPasteMode: options.bracketed ?? false }
    },
    atermController:
      options.encodedEnter === undefined
        ? null
        : { encodeKeyForHost: vi.fn(() => options.encodedEnter ?? null) }
  }
}

function makeTransport() {
  return {
    getPtyId: vi.fn(() => 'pty-1'),
    isConnected: vi.fn(() => true),
    sendInput: vi.fn<(data: string) => boolean>(() => true)
  }
}

function makeSendArgs(
  pane: TestPane,
  transport: ReturnType<typeof makeTransport>,
  overrides: Partial<Parameters<typeof sendComposeBoxDraft>[0]> = {}
): Parameters<typeof sendComposeBoxDraft>[0] {
  return {
    text: 'echo hi',
    mode: 'submit',
    pane: pane as never,
    transport: transport as never,
    tabId: 'tab-1',
    worktreeId: 'wt-1',
    forceBracketedMultilineTextPaste: false,
    getManager: () => ({ getPanes: () => [pane] }) as never,
    getPaneTransports: () => new Map([[pane.id, transport]]) as never,
    isAgentPane: () => false,
    ...overrides
  }
}

// Large enough to exceed the 64 KiB direct-write ceiling so chunk framing is
// observable on the transport instead of hidden inside the engine's paste().
const CHUNKED_BODY = `${'x'.repeat(70_000)}\nsecond line`

describe('sendComposeBoxDraft', () => {
  beforeEach(() => {
    mocks.recordTerminalUserInputForLeaf.mockClear()
    mocks.enqueueNativeChatPtySend.mockClear()
  })

  it('brackets a multiline draft when the pane negotiated mode 2004', async () => {
    const pane = makePane({ bracketed: true })
    const transport = makeTransport()

    const result = await sendComposeBoxDraft(
      makeSendArgs(pane, transport, { text: `${CHUNKED_BODY}\n` })
    )

    expect(result).toEqual({ status: 'sent', submitted: true })
    const writes = transport.sendInput.mock.calls.map(([data]) => data)
    expect(writes[0]).toBe(BRACKETED_PASTE_START)
    expect(writes).toContain(BRACKETED_PASTE_END)
    // Why: the submit Enter must be a separate write AFTER the closing frame.
    expect(writes.indexOf('\r')).toBeGreaterThan(writes.indexOf(BRACKETED_PASTE_END))
  })

  it('leaves a multiline draft unframed for a no-2004 POSIX shell and preserves CR line splits', async () => {
    const pane = makePane({ bracketed: false })
    const transport = makeTransport()
    const body = `echo one\r\necho two\n${'y'.repeat(70_000)}`

    // Why: one trailing newline is trimmed — it would double-execute unframed.
    const result = await sendComposeBoxDraft(makeSendArgs(pane, transport, { text: `${body}\n` }))

    expect(result).toEqual({ status: 'sent', submitted: true })
    const writes = transport.sendInput.mock.calls.map(([data]) => data)
    expect(writes).not.toContain(BRACKETED_PASTE_START)
    expect(writes).not.toContain(BRACKETED_PASTE_END)
    // All writes except the final Enter reassemble the untouched body.
    expect(writes.slice(0, -1).join('')).toBe(body)
    expect(writes.at(-1)).toBe('\r')
  })

  it('force-brackets multiline on Windows ConPTY panes like clipboard paste', async () => {
    const pane = makePane({ bracketed: false })
    const transport = makeTransport()

    const result = await sendComposeBoxDraft(
      makeSendArgs(pane, transport, {
        text: 'echo one\necho two',
        forceBracketedMultilineTextPaste: true
      })
    )

    expect(result).toEqual({ status: 'sent', submitted: true })
    // Forced frames go through terminal.input with LF normalized to CR.
    expect(pane.terminal.input).toHaveBeenCalledWith(
      `${BRACKETED_PASTE_START}echo one\recho two${BRACKETED_PASTE_END}`
    )
    expect(transport.sendInput).toHaveBeenCalledWith('\r')
  })

  it("writes the submit Enter as a separate write using the engine-encoded Enter for kitty/modifyOtherKeys panes, '\\r' fallback for legacy panes", async () => {
    const kittyPane = makePane({ encodedEnter: '[13;1u' })
    const kittyTransport = makeTransport()
    await sendComposeBoxDraft(makeSendArgs(kittyPane, kittyTransport, { text: 'ls' }))
    expect(kittyPane.terminal.paste).toHaveBeenCalledWith('ls')
    expect(kittyTransport.sendInput).toHaveBeenCalledTimes(1)
    expect(kittyTransport.sendInput).toHaveBeenCalledWith('[13;1u')

    // Engine returns nothing for legacy panes → '\r' fallback.
    const legacyPane = makePane({ encodedEnter: null })
    const legacyTransport = makeTransport()
    await sendComposeBoxDraft(makeSendArgs(legacyPane, legacyTransport, { text: 'ls' }))
    expect(legacyTransport.sendInput).toHaveBeenCalledWith('\r')
  })

  it("routes an agent pane's submit Enter through the shared pty queue with NATIVE_CHAT_SUBMIT_DELAY_MS", async () => {
    mocks.enqueueDelays.length = 0
    mocks.enqueueNativeChatPtySend.mockImplementation((_ptyId, _durationMs, start) => {
      start({
        isCancelled: () => false,
        delay: (ms: number, fn: () => void) => {
          mocks.enqueueDelays.push(ms)
          fn()
        },
        markSubmitted: vi.fn()
      })
      return { cancel: vi.fn(), settleAfterMs: 0, bodyStarted: () => true, finished: () => true }
    })
    const pane = makePane()
    const transport = makeTransport()

    const result = await sendComposeBoxDraft(
      makeSendArgs(pane, transport, { text: 'describe this repo', isAgentPane: () => true })
    )

    expect(result).toEqual({ status: 'sent', submitted: true })
    expect(mocks.enqueueNativeChatPtySend).toHaveBeenCalledWith(
      'pty-1',
      NATIVE_CHAT_SUBMIT_DELAY_MS,
      expect.any(Function)
    )
    expect(mocks.enqueueDelays).toEqual([NATIVE_CHAT_SUBMIT_DELAY_MS])
    expect(transport.sendInput).toHaveBeenCalledWith('\r')
  })

  it('stage mode writes the body only and reports submitted:false', async () => {
    const pane = makePane()
    const transport = makeTransport()

    const result = await sendComposeBoxDraft(
      makeSendArgs(pane, transport, { text: 'rm -rf ./build', mode: 'stage' })
    )

    expect(result).toEqual({ status: 'sent', submitted: false })
    expect(pane.terminal.paste).toHaveBeenCalledWith('rm -rf ./build')
    expect(transport.sendInput).not.toHaveBeenCalled()
    expect(pane.terminal.focus).toHaveBeenCalled()
  })

  it('keeps the draft and reports payload-too-large on an oversized draft', async () => {
    const pane = makePane()
    const transport = makeTransport()

    const result = await sendComposeBoxDraft(
      makeSendArgs(pane, transport, { text: 'z'.repeat(16 * 1024 * 1024 + 1) })
    )

    expect(result).toEqual({ status: 'rejected', reason: 'payload-too-large' })
    expect(pane.terminal.paste).not.toHaveBeenCalled()
    expect(transport.sendInput).not.toHaveBeenCalled()
    expect(mocks.recordTerminalUserInputForLeaf).not.toHaveBeenCalled()
  })

  it('cancels the submit when the paste target went stale mid-flight', async () => {
    const pane = makePane({ bracketed: true })
    const transport = makeTransport()
    // Target goes stale after two accepted chunk writes.
    transport.isConnected.mockImplementation(() => transport.sendInput.mock.calls.length < 2)

    const result = await sendComposeBoxDraft(makeSendArgs(pane, transport, { text: CHUNKED_BODY }))

    expect(result).toEqual({ status: 'rejected', reason: 'stale-target' })
    const writes = transport.sendInput.mock.calls.map(([data]) => data)
    expect(writes).not.toContain('\r')
  })
})
