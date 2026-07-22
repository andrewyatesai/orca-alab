// Why: renderer mirror of the runtime's #9156 terminal query-reply election.
// Keyed by ptyId. Host renderers receive verdicts over runtime IPC
// (onTerminalQueryReplyAuthorityChanged); remote viewers receive them in the
// multiplex subscribe ack and query-reply-authority-changed stream events.
// pty-connection's canSendDesktopQueryReply consults this before forwarding
// any engine-drained or capability-handler query reply.

import type { RuntimeTerminalQueryReplyAuthority } from '../../../../shared/runtime-types'

const authorityByPtyId = new Map<string, RuntimeTerminalQueryReplyAuthority>()

export function setQueryReplyAuthorityForPty(
  ptyId: string,
  authority: RuntimeTerminalQueryReplyAuthority
): void {
  authorityByPtyId.set(ptyId, authority)
}

/** Called when a remote stream ends: the next attach re-learns its verdict
 *  from the subscribe ack, and an unknown verdict fails open (see below). */
export function clearQueryReplyAuthorityForPty(ptyId: string): void {
  authorityByPtyId.delete(ptyId)
}

export function getQueryReplyAuthorityForPty(
  ptyId: string
): RuntimeTerminalQueryReplyAuthority | null {
  return authorityByPtyId.get(ptyId) ?? null
}

/**
 * Is THIS view the elected reply answerer for the PTY?
 * `viewerClientId` is null for the host renderer's own panes and the remote
 * subscribe clientId for remote-viewer panes.
 */
export function isQueryReplyAuthorityForThisView(
  ptyId: string,
  viewerClientId: string | null
): boolean {
  const authority = authorityByPtyId.get(ptyId)
  if (!authority) {
    // Why fail open: never leave zero answerers. Pre-verdict panes and views on
    // pre-#9156 hosts (which never send a verdict) keep today's answer-always behavior.
    return true
  }
  switch (authority.kind) {
    case 'host-renderer':
      return viewerClientId === null
    case 'remote-viewer':
      return viewerClientId === authority.clientId
    case 'mobile':
    case 'model':
      return false
  }
}

/** Test seam: reset module state between tests. */
export function _resetQueryReplyAuthorityStateForTest(): void {
  authorityByPtyId.clear()
}
