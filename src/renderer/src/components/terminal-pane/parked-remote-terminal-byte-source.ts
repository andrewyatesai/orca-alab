/**
 * Shared byte source for a parked `remote:` pane's watcher (ssh-pane-parking.md §3.3).
 *
 * Why: parked remote panes have no transport, and `subscribeToPtyData` never
 * sees remote bytes (they bypass local main). This source feeds the watcher's
 * byte parser and mode-2031 responder from ONE live-tail stream subscription,
 * classifies stream end against the runtime before reporting an exit (#9151),
 * and resubscribes with backoff on routine transport churn.
 */
import type { GlobalSettings } from '../../../../shared/types'
import { createBrowserUuid } from '@/lib/browser-uuid'
import { callRuntimeRpc, getActiveRuntimeTarget } from '@/runtime/runtime-rpc-client'
import {
  getRemoteRuntimePtyEnvironmentId,
  getRemoteRuntimeTerminalHandle,
  runtimeTerminalErrorMessage,
  subscribeToRuntimeTerminalData,
  type RuntimeTerminalStreamEndReason
} from '@/runtime/runtime-terminal-stream'

// Why: mirrors the live transport's stream-end confirmation window
// (remote-runtime-pty-transport.ts STREAM_END_EXIT_CONFIRM_TIMEOUT_MS).
const PARKED_STREAM_END_EXIT_CONFIRM_TIMEOUT_MS = 1_000
const PARKED_RESUBSCRIBE_INITIAL_DELAY_MS = 1_000
const PARKED_RESUBSCRIBE_MAX_DELAY_MS = 30_000

function isRemoteTerminalExitMessage(message: string): boolean {
  // Why: terminal_handle_stale / no_connected_pty are recoverable (re-mint or
  // host disconnect, #9151) — only these two confirm the terminal is gone.
  return message.includes('terminal_exited') || message.includes('terminal_gone')
}

export type ParkedRemoteTerminalByteSource = {
  /** Watcher-injectable byte subscription; all subscribers share one stream. */
  subscribeBytes: (cb: (data: string) => void) => () => void
  /** Owner runtime environment resolved at park time, null when unresolvable. */
  runtimeEnvironmentId: string | null
  dispose: () => void
}

export type ParkedRemoteTerminalByteSourceOptions = {
  ptyId: string
  settings: Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null | undefined
  /** Runtime-confirmed host exit while parked; caller runs the same teardown as a pty:exit. */
  onExitConfirmed: () => void
}

export function createParkedRemoteTerminalByteSource(
  options: ParkedRemoteTerminalByteSourceOptions
): ParkedRemoteTerminalByteSource {
  const { ptyId, onExitConfirmed } = options
  const terminal = getRemoteRuntimeTerminalHandle(ptyId)
  // Why: pin the owner environment at park time — the pane must not follow an
  // active-environment switch that happens while it is parked.
  const activeTarget = getActiveRuntimeTarget(options.settings)
  const environmentId =
    getRemoteRuntimePtyEnvironmentId(ptyId) ??
    (activeTarget.kind === 'environment' ? activeTarget.environmentId : null)
  const pinnedSettings = { activeRuntimeEnvironmentId: environmentId }
  // Why: one identity per park cycle keeps this sidecar's stream off peer viewer records.
  const clientId = `parked:${createBrowserUuid()}`

  const subscribers = new Set<(data: string) => void>()
  let disposed = false
  let exitReported = false
  let closeStream: (() => void) | null = null
  let subscribing = false
  let resubscribeTimer: ReturnType<typeof setTimeout> | null = null
  let resubscribeDelayMs = PARKED_RESUBSCRIBE_INITIAL_DELAY_MS

  const reportExitConfirmed = (): void => {
    if (disposed || exitReported) {
      return
    }
    exitReported = true
    onExitConfirmed()
  }

  const scheduleResubscribe = (): void => {
    if (disposed || exitReported || resubscribeTimer !== null || subscribers.size === 0) {
      return
    }
    resubscribeTimer = setTimeout(() => {
      resubscribeTimer = null
      ensureSubscribed()
    }, resubscribeDelayMs)
    resubscribeDelayMs = Math.min(resubscribeDelayMs * 2, PARKED_RESUBSCRIBE_MAX_DELAY_MS)
  }

  // Why: the multiplex protocol delivers genuine host exits and server-side
  // stream cleanup as the same bare end frame; confirm with the runtime before
  // tearing the parked leaf down (#9151), else resubscribe like the transport.
  const classifyStreamEnd = async (): Promise<void> => {
    if (disposed || exitReported || environmentId === null || !terminal) {
      scheduleResubscribe()
      return
    }
    let hostExitConfirmed = false
    try {
      await callRuntimeRpc(
        { kind: 'environment', environmentId },
        'terminal.wait',
        { terminal, for: 'exit', timeoutMs: PARKED_STREAM_END_EXIT_CONFIRM_TIMEOUT_MS },
        { timeoutMs: 15_000 }
      )
      hostExitConfirmed = true
    } catch (error) {
      hostExitConfirmed = isRemoteTerminalExitMessage(runtimeTerminalErrorMessage(error))
    }
    if (hostExitConfirmed) {
      reportExitConfirmed()
      return
    }
    scheduleResubscribe()
  }

  const handleStreamDown = (reason: RuntimeTerminalStreamEndReason): void => {
    closeStream = null
    if (disposed || exitReported) {
      return
    }
    if (reason === 'end') {
      void classifyStreamEnd()
      return
    }
    scheduleResubscribe()
  }

  function ensureSubscribed(): void {
    if (disposed || exitReported || subscribing || closeStream !== null || subscribers.size === 0) {
      return
    }
    subscribing = true
    // Why: one down-handler run per attempt — a live-tail failure both rejects
    // the subscribe promise and fires onStreamEnd for the same event.
    let attemptDownHandled = false
    const noteStreamDown = (reason: RuntimeTerminalStreamEndReason): void => {
      if (attemptDownHandled) {
        return
      }
      attemptDownHandled = true
      handleStreamDown(reason)
    }
    void subscribeToRuntimeTerminalData(
      pinnedSettings,
      ptyId,
      clientId,
      (data) => {
        // Why: bytes prove the stream is healthy — reset the backoff here, not
        // on subscribe success, so an ack-then-end host can't tight-loop at 1s.
        resubscribeDelayMs = PARKED_RESUBSCRIBE_INITIAL_DELAY_MS
        for (const subscriber of subscribers) {
          subscriber(data)
        }
      },
      {
        // Why: skip the historical snapshot so watcher start can never re-fire
        // stale bells the outcome observers already consumed (§3.3).
        startAtLiveTail: true,
        onStreamEnd: noteStreamDown
      }
    )
      .then((dispose) => {
        subscribing = false
        if (disposed || exitReported || subscribers.size === 0 || attemptDownHandled) {
          dispose()
          return
        }
        closeStream = dispose
      })
      .catch((error) => {
        subscribing = false
        if (disposed || exitReported || attemptDownHandled) {
          return
        }
        attemptDownHandled = true
        if (isRemoteTerminalExitMessage(runtimeTerminalErrorMessage(error))) {
          reportExitConfirmed()
          return
        }
        scheduleResubscribe()
      })
  }

  return {
    runtimeEnvironmentId: environmentId,
    subscribeBytes: (cb) => {
      subscribers.add(cb)
      ensureSubscribed()
      return () => {
        subscribers.delete(cb)
        if (subscribers.size === 0 && closeStream !== null) {
          closeStream()
          closeStream = null
        }
      }
    },
    dispose: () => {
      disposed = true
      if (resubscribeTimer !== null) {
        clearTimeout(resubscribeTimer)
        resubscribeTimer = null
      }
      closeStream?.()
      closeStream = null
      subscribers.clear()
    }
  }
}
