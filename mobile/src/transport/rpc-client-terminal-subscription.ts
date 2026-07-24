import { buildNativeChatUnsubscribe } from '../../../src/shared/native-chat-stream-unsubscribe'

type TerminalStreamParams = {
  terminal?: unknown
}

type MutableStreamRequest = {
  method: string
  params: unknown
}

export function updateTerminalSubscriptionViewport(
  streams: Iterable<MutableStreamRequest>,
  terminal: string,
  viewport: { cols: number; rows: number }
): void {
  for (const stream of streams) {
    if (
      stream.method !== 'terminal.subscribe' ||
      !stream.params ||
      typeof stream.params !== 'object'
    ) {
      continue
    }
    const params = stream.params as TerminalStreamParams
    if (params.terminal !== terminal) {
      continue
    }
    stream.params = {
      ...stream.params,
      viewport
    }
  }
}

/** Build the unsubscribe RPC for a streaming method that needs the host told to
 *  tear down (session tabs, native chat), or null when none is required. Keeps
 *  the per-method echo logic out of the rpc-client teardown closure.
 *  `requestId` is the subscribe RPC id: the host keys each session.tabs
 *  subscriber by it, so it must be echoed as `subscriptionId` for a targeted
 *  teardown (a subscriptionId-less unsubscribe is a PREFIX wipe that evicts
 *  every sibling subscriber on the same worktree). */
export function buildStreamUnsubscribe(
  method: string | undefined,
  params: unknown,
  requestId?: string
): { method: string; params: Record<string, unknown> } | null {
  if (!params || typeof params !== 'object') {
    return null
  }
  if (method === 'session.tabs.subscribe') {
    const worktree = (params as { worktree?: unknown }).worktree
    if (typeof worktree !== 'string') {
      return null
    }
    return {
      method: 'session.tabs.unsubscribe',
      params: requestId ? { worktree, subscriptionId: requestId } : { worktree }
    }
  }
  if (method === 'nativeChat.subscribe') {
    const subscriptionId = (params as { subscriptionId?: unknown }).subscriptionId
    if (typeof subscriptionId === 'string') {
      return { method: 'nativeChat.unsubscribe', params: { subscriptionId } }
    }
    // Backward compatibility for callers that predate explicit cleanup tokens.
    const agent = (params as { agent?: unknown }).agent
    const sessionId = (params as { sessionId?: unknown }).sessionId
    return typeof agent === 'string' && typeof sessionId === 'string'
      ? buildNativeChatUnsubscribe(agent, sessionId)
      : null
  }
  return null
}

/** Unsubscribe method name for a subscriptionId-keyed server subscription
 *  (accounts, notifications, runtime.clientEvents, browser.screencast). Kept
 *  explicit because browser.screencast has no `.subscribe` suffix, so a naive
 *  `.replace(/\.subscribe$/, '.unsubscribe')` would re-emit `browser.screencast`
 *  (a re-subscribe) instead of tearing the stream down. */
export function serverSubscriptionUnsubscribeMethod(method: string): string {
  if (method === 'browser.screencast') {
    return 'browser.screencast.unsubscribe'
  }
  return method.replace(/\.subscribe$/, '.unsubscribe')
}

// Why: resolves a terminal handle to its live binary streamId from the rpc
// client's per-connection routing maps, so callers never track raw streamIds.
export function findRoutableTerminalStreamId(
  streams: ReadonlyMap<string, MutableStreamRequest>,
  terminalStreamIdsByRequest: ReadonlyMap<string, ReadonlySet<number>>,
  routableStreamIds: ReadonlyMap<number, unknown>,
  terminal: string
): number | null {
  for (const [requestId, streamIds] of terminalStreamIdsByRequest) {
    const stream = streams.get(requestId)
    if (
      !stream ||
      stream.method !== 'terminal.subscribe' ||
      !stream.params ||
      typeof stream.params !== 'object' ||
      (stream.params as TerminalStreamParams).terminal !== terminal
    ) {
      continue
    }
    // Why: a request can accumulate ids across host-side resubscribes; the
    // latest still-routable one is the live stream.
    let latest: number | null = null
    for (const streamId of streamIds) {
      if (routableStreamIds.has(streamId)) {
        latest = streamId
      }
    }
    if (latest !== null) {
      return latest
    }
  }
  return null
}

export function buildTerminalUnsubscribeParams(
  params: unknown
): { subscriptionId: string; client?: { id: string } } | null {
  if (!params || typeof params !== 'object') {
    return null
  }
  const subscribeParams = params as {
    terminal?: unknown
    client?: { id?: unknown }
  }
  if (typeof subscribeParams.terminal !== 'string') {
    return null
  }
  const clientId =
    typeof subscribeParams.client?.id === 'string' ? subscribeParams.client.id : undefined
  return {
    subscriptionId: clientId ? `${subscribeParams.terminal}:${clientId}` : subscribeParams.terminal,
    ...(clientId ? { client: { id: clientId } } : {})
  }
}
