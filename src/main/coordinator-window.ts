// Coordinator v0 (docs/rust-migration/coordinator-v0-design.md): a separate
// BrowserWindow whose renderer is a thin client of the daemon socket. Main's
// entire job here is the ONE channel-pair byte tunnel — it resolves the live
// daemon endpoint (socket path + token) and relays bytes verbatim; every
// protocol decision (hello, RPCs, subscriber role) lives in the renderer.
import { join } from 'node:path'
import { readFileSync } from 'node:fs'
import { createConnection, type Socket } from 'node:net'
import { BrowserWindow, ipcMain, nativeTheme } from 'electron'
import { is } from '@electron-toolkit/utils'
import {
  COORDINATOR_TUNNEL_EVENT_CHANNEL,
  COORDINATOR_TUNNEL_REQUEST_CHANNEL,
  type CoordinatorTunnelEvent,
  type CoordinatorTunnelRequest
} from '../shared/coordinator-daemon-tunnel'
import { getDaemonEndpointPaths } from './daemon/daemon-init'
import { PROTOCOL_VERSION } from './daemon/types'

let coordinatorWindow: BrowserWindow | null = null
// One live tunnel-socket set per coordinator window; keyed by the renderer-
// assigned socketId (two per protocol client: control + stream).
const tunnelSockets = new Map<number, Socket>()
let tunnelHandlerInstalled = false

export function openCoordinatorWindow(): void {
  if (coordinatorWindow && !coordinatorWindow.isDestroyed()) {
    coordinatorWindow.show()
    coordinatorWindow.focus()
    return
  }
  installTunnelHandler()
  const window = new BrowserWindow({
    width: 1160,
    height: 780,
    minWidth: 640,
    minHeight: 480,
    title: 'Orca Coordinator',
    autoHideMenuBar: true,
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#0a0a0a' : '#ffffff',
    webPreferences: {
      preload: join(__dirname, '../preload/coordinator.js'),
      sandbox: true
    }
  })
  coordinatorWindow = window
  window.on('closed', () => {
    coordinatorWindow = null
    closeAllTunnelSockets()
  })
  if (is.dev && process.env.ELECTRON_RENDERER_URL) {
    void window.loadURL(`${process.env.ELECTRON_RENDERER_URL}/coordinator.html`)
  } else {
    void window.loadFile(join(__dirname, '../renderer/coordinator.html'))
  }
}

function installTunnelHandler(): void {
  if (tunnelHandlerInstalled) {
    return
  }
  tunnelHandlerInstalled = true
  ipcMain.on(COORDINATOR_TUNNEL_REQUEST_CHANNEL, (event, message: CoordinatorTunnelRequest) => {
    // Only the coordinator window's renderer may drive the tunnel — the token
    // relayed on open-ok grants full daemon access.
    if (!coordinatorWindow || event.sender.id !== coordinatorWindow.webContents.id) {
      return
    }
    handleTunnelRequest(message)
  })
}

function handleTunnelRequest(message: CoordinatorTunnelRequest): void {
  switch (message.op) {
    case 'open':
      openTunnelSocket(message.socketId)
      break
    case 'data':
      tunnelSockets.get(message.socketId)?.write(message.data)
      break
    case 'close':
      tunnelSockets.get(message.socketId)?.destroy()
      tunnelSockets.delete(message.socketId)
      break
  }
}

function openTunnelSocket(socketId: number): void {
  let endpoint: { socketPath: string; token: string }
  try {
    endpoint = resolveDaemonEndpoint()
  } catch (error) {
    sendTunnelEvent({
      op: 'open-error',
      socketId,
      error: `daemon endpoint unavailable: ${error instanceof Error ? error.message : String(error)}`
    })
    return
  }
  const socket = createConnection(endpoint.socketPath)
  tunnelSockets.set(socketId, socket)
  socket.once('connect', () => {
    sendTunnelEvent({
      op: 'open-ok',
      socketId,
      token: endpoint.token,
      protocolVersion: PROTOCOL_VERSION
    })
  })
  socket.on('data', (chunk: Buffer) => {
    // Relay RAW BYTES (structured-cloned across IPC), not a utf8 string: the
    // stream socket may carry v1020 binary frames a decode would corrupt.
    // Chunk is always a Buffer — no setEncoding() call on this socket.
    sendTunnelEvent({ op: 'data', socketId, data: chunk })
  })
  socket.once('error', (error) => {
    // Pre-connect failures ack as open-error; post-connect ones surface via
    // the 'close' that always follows an error.
    if (socket.connecting) {
      sendTunnelEvent({ op: 'open-error', socketId, error: error.message })
    }
  })
  socket.once('close', () => {
    if (tunnelSockets.get(socketId) === socket) {
      tunnelSockets.delete(socketId)
      sendTunnelEvent({ op: 'close', socketId })
    }
  })
}

function resolveDaemonEndpoint(): { socketPath: string; token: string } {
  const { socketPath, tokenPath } = getDaemonEndpointPaths()
  // The daemon publishes its generated token to this file on startup
  // (daemon-init launch contract); read fresh per open so a daemon restart
  // (new token) never strands the tunnel on stale auth.
  return { socketPath, token: readFileSync(tokenPath, 'utf8').trim() }
}

function sendTunnelEvent(event: CoordinatorTunnelEvent): void {
  if (coordinatorWindow && !coordinatorWindow.isDestroyed()) {
    coordinatorWindow.webContents.send(COORDINATOR_TUNNEL_EVENT_CHANNEL, event)
  }
}

function closeAllTunnelSockets(): void {
  for (const socket of tunnelSockets.values()) {
    socket.destroy()
  }
  tunnelSockets.clear()
}
