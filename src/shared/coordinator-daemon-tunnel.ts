// Wire shapes for the coordinator window's SINGLE preload channel pair — the
// only IPC the coordinator surface uses (coordinator-v0-design.md: main relays
// daemon socket bytes verbatim; no per-feature channels, ever). One renderer→
// main channel carries open/data/close for multiplexed tunnel sockets; one
// main→renderer channel carries their acks, bytes, and closes back.

export const COORDINATOR_TUNNEL_REQUEST_CHANNEL = 'coordinator:daemon-tunnel-request'
export const COORDINATOR_TUNNEL_EVENT_CHANNEL = 'coordinator:daemon-tunnel-event'

/** Renderer → main. `socketId` is renderer-assigned and scopes one daemon
 *  socket connection (a protocol client opens two: control + stream). */
export type CoordinatorTunnelRequest =
  | { op: 'open'; socketId: number }
  | { op: 'data'; socketId: number; data: string }
  | { op: 'close'; socketId: number }

/** Main → renderer. `open-ok` carries the daemon auth main resolved (token
 *  file + current protocol) so the renderer can send the hello itself. */
export type CoordinatorTunnelEvent =
  | { op: 'open-ok'; socketId: number; token: string; protocolVersion: number }
  | { op: 'open-error'; socketId: number; error: string }
  | { op: 'data'; socketId: number; data: string }
  | { op: 'close'; socketId: number }

/** The preload-exposed bridge (window.coordinatorDaemonTunnel). */
export type CoordinatorDaemonTunnelBridge = {
  send: (message: CoordinatorTunnelRequest) => void
  /** Returns unsubscribe. */
  onMessage: (listener: (message: CoordinatorTunnelEvent) => void) => () => void
}
