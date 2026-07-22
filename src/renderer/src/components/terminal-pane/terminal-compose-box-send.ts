// Compose-box send: body through the shared paste pipeline (bracketed when the
// pane negotiated mode 2004, chunked/remote-safe), then the submit Enter as a
// SEPARATE protocol-encoded write — a same-write CR inside a framed body is
// treated as paste content by agent TUIs and the text lands without sending.

import type { ManagedPane, PaneManager } from '@/lib/pane-manager/pane-manager'
import type { PtyTransport } from './pty-transport'
import { getConnectionId } from '@/lib/connection-context'
import { NATIVE_CHAT_SUBMIT_DELAY_MS } from '../../../../shared/native-chat-answer-stepping'
import { enqueueNativeChatPtySend } from '../native-chat/native-chat-pty-send-queue'
import { pasteTerminalText } from './terminal-bracketed-paste'
import { recordTerminalUserInputForLeaf } from './terminal-input-activity'
import { executeTerminalPastePlan, planTerminalPasteWithYield } from './terminal-paste-coordinator'
import type { TerminalPasteExecutionReason } from './terminal-paste-model'
import { resolveTerminalPasteRuntime } from './terminal-paste-runtime'
import { getTerminalPasteSshRemotePlatform } from './terminal-paste-ssh-platform'
import { isTerminalPanePasteTargetCurrent } from './terminal-paste-target-state'
import { writeTerminalPastePtyInput } from './terminal-pty-paste-writer'

export type ComposeBoxSubmitMode = 'submit' | 'stage'

export type ComposeBoxSendArgs = {
  text: string
  mode: ComposeBoxSubmitMode
  pane: ManagedPane
  transport: PtyTransport | null
  tabId: string
  worktreeId: string
  forceBracketedMultilineTextPaste: boolean
  getManager: () => PaneManager | null
  getPaneTransports: () => Map<number, PtyTransport>
  /** Why: injected (not read from the store here) so the agent-vs-shell submit policy is unit-testable. */
  isAgentPane: () => boolean
}

export type ComposeBoxSendResult =
  | { status: 'sent'; submitted: boolean }
  | { status: 'rejected'; reason: TerminalPasteExecutionReason }

export async function sendComposeBoxDraft({
  text,
  mode,
  pane,
  transport,
  tabId,
  worktreeId,
  forceBracketedMultilineTextPaste,
  getManager,
  getPaneTransports,
  isAgentPane
}: ComposeBoxSendArgs): Promise<ComposeBoxSendResult> {
  // Why: exactly one trailing newline is trimmed — it would double-execute in the unframed fallback.
  const body = text.replace(/\r?\n$/, '')
  const ptyId = transport?.getPtyId() ?? null
  const connectionId = getConnectionId(worktreeId) ?? null
  const plan = await planTerminalPasteWithYield({
    text: body,
    source: 'programmatic',
    target: {
      kind: 'terminal',
      paneId: pane.id,
      leafId: pane.leafId,
      ptyId,
      runtime: resolveTerminalPasteRuntime({
        platform: getComposeBoxClientPlatform(),
        ptyId,
        connectionId,
        remotePlatform: getTerminalPasteSshRemotePlatform(connectionId),
        transport
      })
    },
    forceBracketedPasteForMultiline: forceBracketedMultilineTextPaste,
    terminalBracketedPasteMode: pane.terminal.modes?.bracketedPasteMode === true
  })
  const isTargetCurrent = (): boolean =>
    isTerminalPanePasteTargetCurrent({
      manager: getManager(),
      paneTransports: getPaneTransports(),
      paneId: pane.id,
      leafId: pane.leafId,
      transport: transport ?? undefined,
      ptyId
    })
  const result = await executeTerminalPastePlan(plan, {
    pasteText: (chunk, options) => pasteTerminalText(pane.terminal, chunk, options),
    writePty: (data) => writeTerminalPastePtyInput(transport ?? undefined, data),
    isTargetCurrent,
    canContinue: isTargetCurrent
  })
  if (result.status !== 'pasted') {
    return { status: 'rejected', reason: result.reason ?? 'paste-rejected' }
  }
  recordTerminalUserInputForLeaf(tabId, pane.leafId)
  const submitted = mode === 'submit' && writeComposeBoxSubmitEnter({ pane, transport, isAgentPane })
  pane.terminal.focus()
  return { status: 'sent', submitted }
}

/** The submit Enter in the pane's negotiated dialect (kitty CSI-u / modifyOtherKeys), '\r' for legacy panes. */
export function encodeComposeBoxSubmitEnter(pane: Pick<ManagedPane, 'atermController'>): string {
  const encoded = pane.atermController?.encodeKeyForHost('Enter', 0)
  return encoded != null && encoded.length > 0 ? encoded : '\r'
}

function writeComposeBoxSubmitEnter({
  pane,
  transport,
  isAgentPane
}: Pick<ComposeBoxSendArgs, 'pane' | 'transport' | 'isAgentPane'>): boolean {
  if (!transport) {
    return false
  }
  const enterBytes = encodeComposeBoxSubmitEnter(pane)
  const ptyId = transport.getPtyId()
  if (ptyId && isAgentPane()) {
    // Why: the shared per-pty queue (not a bare setTimeout) so this Enter serializes with
    // native-chat clear/body/Enter sequences on the same PTY instead of interleaving them.
    // Only the Enter is enqueued; the body above went through the paste executor directly,
    // so a concurrent native-chat send can still order itself between body and Enter.
    enqueueNativeChatPtySend(ptyId, NATIVE_CHAT_SUBMIT_DELAY_MS, ({ delay, markSubmitted }) => {
      delay(NATIVE_CHAT_SUBMIT_DELAY_MS, () => {
        transport.sendInput(enterBytes)
        markSubmitted()
      })
    })
    return true
  }
  return transport.sendInput(enterBytes)
}

function getComposeBoxClientPlatform(
  userAgent = globalThis.navigator?.userAgent ?? ''
): NodeJS.Platform {
  if (userAgent.includes('Mac')) {
    return 'darwin'
  }
  return userAgent.includes('Windows') ? 'win32' : 'linux'
}
