// node:net transport leg for DaemonProtocolClient: connects a Unix socket (or
// Windows named pipe) to the daemon endpoint. Node-side callers only (Electron
// main, integration harnesses) — the coordinator renderer runs the same client
// over the preload byte tunnel instead.
import { createConnection, type Socket } from 'node:net'
import type { DaemonByteTransport, DaemonTransportConnection } from './daemon-protocol-client'

export function connectDaemonSocketTransport(
  socketPath: string,
  timeoutMs = 8000
): Promise<DaemonTransportConnection> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath)
    const timer = setTimeout(() => {
      socket.destroy()
      reject(new Error(`daemon socket connect timeout after ${timeoutMs}ms (${socketPath})`))
    }, timeoutMs)
    socket.once('connect', () => {
      clearTimeout(timer)
      resolve({ transport: socketByteTransport(socket) })
    })
    socket.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
  })
}

function socketByteTransport(socket: Socket): DaemonByteTransport {
  return {
    send: (data) => {
      socket.write(data)
    },
    onData: (listener) => {
      // Raw bytes, NOT utf8-decoded: the stream socket may carry v1020 binary
      // frames, which a decode would corrupt; the client decodes per format.
      // Chunk is always a Buffer — no setEncoding() call on this socket.
      socket.on('data', (chunk: Buffer) => listener(chunk))
    },
    onClose: (listener) => {
      socket.once('close', listener)
    },
    close: () => {
      socket.destroy()
    }
  }
}
