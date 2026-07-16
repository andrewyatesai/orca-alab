// The coordinator window's ENTIRE preload surface: one channel pair tunneling
// daemon socket bytes (coordinator-v0-design.md). Deliberately not index.ts —
// this window must never inherit the legacy per-feature IPC surface.
import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron'
import {
  COORDINATOR_TUNNEL_EVENT_CHANNEL,
  COORDINATOR_TUNNEL_REQUEST_CHANNEL,
  type CoordinatorDaemonTunnelBridge,
  type CoordinatorTunnelEvent,
  type CoordinatorTunnelRequest
} from '../shared/coordinator-daemon-tunnel'

const coordinatorDaemonTunnel: CoordinatorDaemonTunnelBridge = {
  send: (message: CoordinatorTunnelRequest): void => {
    ipcRenderer.send(COORDINATOR_TUNNEL_REQUEST_CHANNEL, message)
  },
  onMessage: (listener: (message: CoordinatorTunnelEvent) => void): (() => void) => {
    const wrapped = (_event: IpcRendererEvent, message: CoordinatorTunnelEvent): void => {
      listener(message)
    }
    ipcRenderer.on(COORDINATOR_TUNNEL_EVENT_CHANNEL, wrapped)
    return () => {
      ipcRenderer.removeListener(COORDINATOR_TUNNEL_EVENT_CHANNEL, wrapped)
    }
  }
}

contextBridge.exposeInMainWorld('coordinatorDaemonTunnel', coordinatorDaemonTunnel)
