// Read-only subscriber role wire shapes (protocol v1019+, Rust daemon only).
// Why the role exists: createOrAttach REBINDS ownership, so a second client
// attaching silently steals the owner's stream. A subscriber mirrors a session
// instead: snapshot hydration on subscribe, then live data/exit fan-out, with
// write/resize DENIED — followers pin to the owner's grid (a follower resize
// would push new dims to the live PTY and SIGWINCH-bounce the owner's TUI).
// Re-exported via ./types (the line-capped wire-shape entry point), matching
// daemon-stream-events.ts / daemon-errors.ts.
import type { ShellReadyState, TerminalSnapshot } from './types'

// Gate senders on this: older daemons answer `subscribe` with an RPC error.
export const SUBSCRIBER_PROTOCOL_VERSION = 1019

// Typed error-code prefix on the RPC error rejecting a subscriber's write or
// resize. Must equal SUBSCRIBER_READ_ONLY_ERROR in
// rust/crates/orca-daemon/src/protocol.rs.
export const SUBSCRIBER_READ_ONLY_ERROR = 'subscriber-read-only'

export type SubscribeRequest = {
  id: string
  type: 'subscribe'
  payload: {
    sessionId: string
  }
}

// Idempotent (like detach); a full disconnect also drops all of the client's
// subscriptions daemon-side.
export type UnsubscribeRequest = {
  id: string
  type: 'unsubscribe'
  payload: {
    sessionId: string
  }
}

/** Subscriber hydration: the same engine serialize a reattach returns, minus
 *  the ownership rebind. */
export type SubscribeResult = {
  snapshot: TerminalSnapshot
  pid: number | null
  shellState: ShellReadyState
}

/** Both subscriber RPCs, as one member of the DaemonRequest union. */
export type SubscriberSessionRequest = SubscribeRequest | UnsubscribeRequest
