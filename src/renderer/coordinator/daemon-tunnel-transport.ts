// The renderer half of the coordinator byte tunnel: adapts the preload bridge
// (one channel pair, socketId-multiplexed) into the DaemonByteTransport the
// shared protocol client consumes. Auth (token + protocol version) arrives on
// the open ack — main resolves it from the daemon runtime dir — so the client
// adopts it instead of being configured upfront.
import type {
  CoordinatorDaemonTunnelBridge,
  CoordinatorTunnelEvent
} from '../../shared/coordinator-daemon-tunnel'
import type {
  DaemonByteTransport,
  DaemonSocketRole,
  DaemonTransportConnection
} from '../../shared/daemon-protocol-client'

declare global {
  // oxlint-disable-next-line typescript-eslint/consistent-type-definitions -- declaration merging requires interface
  interface Window {
    coordinatorDaemonTunnel: CoordinatorDaemonTunnelBridge
  }
}

let nextSocketId = 1

export function openCoordinatorDaemonTransport(
  _role: DaemonSocketRole
): Promise<DaemonTransportConnection> {
  // The tunnel is role-agnostic — the protocol client sends the hello line
  // (which carries the role) itself; main just relays bytes.
  const bridge = window.coordinatorDaemonTunnel
  const socketId = nextSocketId++
  return new Promise((resolve, reject) => {
    let dataListener: ((chunk: Uint8Array) => void) | null = null
    let closeListener: (() => void) | null = null
    let opened = false
    const unsubscribe = bridge.onMessage((message: CoordinatorTunnelEvent) => {
      if (message.socketId !== socketId) {
        return
      }
      switch (message.op) {
        case 'open-ok':
          opened = true
          resolve({
            transport,
            auth: { token: message.token, protocolVersion: message.protocolVersion }
          })
          break
        case 'open-error':
          unsubscribe()
          reject(new Error(message.error))
          break
        case 'data':
          dataListener?.(message.data)
          break
        case 'close':
          unsubscribe()
          if (opened) {
            closeListener?.()
          } else {
            reject(new Error('daemon tunnel closed before open'))
          }
          break
      }
    })
    const transport: DaemonByteTransport = {
      send: (data) => {
        bridge.send({ op: 'data', socketId, data })
      },
      onData: (listener) => {
        dataListener = listener
      },
      onClose: (listener) => {
        closeListener = listener
      },
      close: () => {
        bridge.send({ op: 'close', socketId })
        unsubscribe()
      }
    }
    bridge.send({ op: 'open', socketId })
  })
}
