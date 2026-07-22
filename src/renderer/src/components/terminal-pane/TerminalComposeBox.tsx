import { useMemo, useRef, useState } from 'react'
import type { ManagedPane, PaneManager } from '@/lib/pane-manager/pane-manager'
import type { PtyTransport } from './pty-transport'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import { useAppStore } from '@/store'
import {
  keybindingMatchesAction,
  type KeybindingOverrides,
  type TerminalShortcutPolicy
} from '../../../../shared/keybindings'
import {
  pushHistory,
  recallNext,
  recallPrevious
} from '../native-chat/native-chat-composer-state'
import {
  getComposeBoxDraftEntry,
  setComposeBoxDraftEntry
} from './terminal-compose-box-draft-cache'
import { createComposeBoxImeEnterGuard } from './terminal-compose-box-ime-guard'
import {
  sendComposeBoxDraft,
  type ComposeBoxSubmitMode
} from './terminal-compose-box-send'
import type { TerminalPasteExecutionReason } from './terminal-paste-model'

export type TerminalComposeBoxProps = {
  paneKey: string
  pane: ManagedPane
  transport: PtyTransport | null
  tabId: string
  worktreeId: string
  forceBracketedMultilineTextPaste: boolean
  keybindings?: KeybindingOverrides
  terminalShortcutPolicy: TerminalShortcutPolicy
  getManager: () => PaneManager | null
  getPaneTransports: () => Map<number, PtyTransport>
  onClose: () => void
}

function sendRejectionLabel(reason: TerminalPasteExecutionReason): string {
  if (reason === 'payload-too-large') {
    return translate(
      'components.terminal-pane.compose-box.rejectedTooLarge',
      'Draft is too large to send'
    )
  }
  return translate('components.terminal-pane.compose-box.rejectedGeneric', 'Send failed — draft kept')
}

/**
 * Bottom-anchored multi-line drafting overlay for the active terminal pane.
 * Enter sends the draft through the paste pipeline; the submit Enter is a
 * separate protocol-encoded write (see terminal-compose-box-send.ts).
 */
export function TerminalComposeBox({
  paneKey,
  pane,
  transport,
  tabId,
  worktreeId,
  forceBracketedMultilineTextPaste,
  keybindings,
  terminalShortcutPolicy,
  getManager,
  getPaneTransports,
  onClose
}: TerminalComposeBoxProps): React.JSX.Element {
  // Why: the portal keys this component by paneKey, so lazy init re-reads the cache per pane.
  const [{ draft, history }, setEntry] = useState(() => getComposeBoxDraftEntry(paneKey))
  const [sendError, setSendError] = useState<TerminalPasteExecutionReason | null>(null)
  const [sending, setSending] = useState(false)
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)
  const [imeGuard] = useState(() => createComposeBoxImeEnterGuard())
  const isMac = navigator.userAgent.includes('Mac')
  const platform: NodeJS.Platform = isMac
    ? 'darwin'
    : navigator.userAgent.includes('Windows')
      ? 'win32'
      : 'linux'

  const updateEntry = (nextDraft: string, nextHistory = history): void => {
    setEntry({ draft: nextDraft, history: nextHistory })
    // Why: persist on every change so pane switches / close need no unmount flush.
    setComposeBoxDraftEntry(paneKey, { draft: nextDraft, history: nextHistory })
  }

  const lineCount = useMemo(() => draft.split('\n').length, [draft])
  // Why: framing keys off what the app actually negotiated (mode 2004) or the Windows ConPTY force policy.
  const bracketed =
    pane.terminal.modes?.bracketedPasteMode === true ||
    (forceBracketedMultilineTextPaste && lineCount > 1)
  const showSequentialRunWarning = lineCount > 1 && !bracketed

  const submit = (mode: ComposeBoxSubmitMode): void => {
    if (sending || draft.trim() === '') {
      return
    }
    setSending(true)
    setSendError(null)
    const sentDraft = draft
    void sendComposeBoxDraft({
      text: sentDraft,
      mode,
      pane,
      transport,
      tabId,
      worktreeId,
      forceBracketedMultilineTextPaste,
      getManager,
      getPaneTransports,
      // Why: agent TUIs need the delayed queued Enter; foreground-agent identity lives in the store.
      isAgentPane: () =>
        Boolean(useAppStore.getState().paneForegroundAgentByPaneKey[paneKey]?.agent)
    }).then((result) => {
      setSending(false)
      if (result.status === 'rejected') {
        setSendError(result.reason)
        return
      }
      updateEntry('', pushHistory(history, sentDraft))
      onClose()
    })
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): void => {
    if (
      e.key === 'Enter' &&
      imeGuard.shouldAbsorbEnter({ isComposing: e.nativeEvent.isComposing, keyCode: e.keyCode })
    ) {
      // Why: the commit Enter of an IME composition (incl. macOS Hangul's re-dispatch) must never submit.
      e.preventDefault()
      return
    }
    if (e.nativeEvent.isComposing || e.keyCode === 229) {
      return
    }
    if (
      !e.repeat &&
      keybindingMatchesAction('terminal.composeBox', e, platform, keybindings, {
        context: 'terminal',
        terminalShortcutPolicy
      })
    ) {
      // Why: the window-level chord handler skips editable targets, so the box re-matches its own toggle to close.
      e.preventDefault()
      e.stopPropagation()
      onClose()
      return
    }
    if (e.key === 'Escape') {
      // Why: stopPropagation so tab-level Esc handlers never also act on this key; draft is kept.
      e.preventDefault()
      e.stopPropagation()
      onClose()
      return
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      submit(isMac ? (e.metaKey ? 'stage' : 'submit') : e.ctrlKey ? 'stage' : 'submit')
      return
    }
    if (e.key === 'ArrowUp' && (draft === '' || history.index !== null)) {
      const recall = recallPrevious(history)
      if (recall.draft !== null) {
        e.preventDefault()
        updateEntry(recall.draft, recall.history)
      }
      return
    }
    if (e.key === 'ArrowDown' && history.index !== null) {
      const recall = recallNext(history)
      if (recall.draft !== null) {
        e.preventDefault()
        updateEntry(recall.draft, recall.history)
      }
    }
  }

  const modGlyph = isMac ? '⌘' : 'Ctrl+'
  const shiftGlyph = isMac ? '⇧' : 'Shift+'

  return (
    <div
      data-terminal-compose-box
      className={cn(
        // Why: overlay, not layout push — resizing the grid would SIGWINCH-reflow a running TUI on every toggle.
        'absolute inset-x-2 bottom-2 z-40 rounded-lg border border-border p-1.5 shadow-xs',
        'bg-muted/50 dark:bg-input/40 backdrop-blur-sm'
      )}
    >
      <textarea
        ref={textareaRef}
        autoFocus
        value={draft}
        aria-label={translate('components.terminal-pane.compose-box.inputLabel', 'Compose command')}
        // Why: mono is the one deliberate divergence from the chat composer — this drafts shell input.
        className="scrollbar-sleek min-h-12 max-h-48 w-full resize-none bg-transparent px-2 py-1 font-mono text-sm outline-none"
        onChange={(e) => updateEntry(e.target.value)}
        onKeyDown={onKeyDown}
        onCompositionStart={imeGuard.onCompositionStart}
        onCompositionEnd={imeGuard.onCompositionEnd}
      />
      <div className="flex items-center justify-between gap-2 px-2 pt-1">
        <div className="min-w-0 truncate text-xs text-muted-foreground">
          {sendError ? (
            <span className="text-destructive">{sendRejectionLabel(sendError)}</span>
          ) : showSequentialRunWarning ? (
            translate(
              'components.terminal-pane.compose-box.sequentialRunWarning',
              "App didn't enable bracketed paste — {count} lines will run one by one"
            ).replace('{count}', String(lineCount))
          ) : (
            translate('components.terminal-pane.compose-box.targetHint', 'Drafting for this pane')
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">
          <span>
            {shiftGlyph}↩ {translate('components.terminal-pane.compose-box.newlineHint', 'newline')}{' '}
            · {modGlyph}↩ {translate('components.terminal-pane.compose-box.stageHint', 'stage')} · ↩{' '}
            {translate('components.terminal-pane.compose-box.sendHint', 'send')}
          </span>
          <Button
            size="sm"
            disabled={sending || draft.trim() === ''}
            onClick={() => submit('submit')}
          >
            {translate('components.terminal-pane.compose-box.sendButton', 'Send')}
          </Button>
        </div>
      </div>
    </div>
  )
}
