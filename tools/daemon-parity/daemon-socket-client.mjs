// A minimal NDJSON client for the terminal daemon's Unix-socket protocol
// (src/main/daemon/types.ts). Speaks the real wire: a `hello` handshake per
// socket, one shared `clientId` across a control (RPC) + stream (events) pair,
// id-correlated RpcResponses on control, and `data`/`exit`/`terminalError`
// events on stream. Used by the daemon parity gate to drive BOTH the Rust
// (orca-daemon) and Node daemons over the exact same transport.

import net from 'node:net'

const encodeNdjson = (msg) => `${JSON.stringify(msg)}\n`

// Split a socket's byte stream into JSON objects on newline boundaries. Mirrors
// the daemon's resync-on-newline framing (orca-net::ndjson / ndjson.ts).
function makeLineReader(onObject) {
  let buffer = ''
  return (chunk) => {
    buffer += chunk
    let nl = buffer.indexOf('\n')
    while (nl !== -1) {
      const line = buffer.slice(0, nl)
      buffer = buffer.slice(nl + 1)
      nl = buffer.indexOf('\n')
      if (line.trim()) {
        onObject(JSON.parse(line))
      }
    }
  }
}

function connectSocket(socketPath, timeoutMs) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath)
    const timer = setTimeout(() => {
      socket.destroy()
      reject(new Error(`connect timeout after ${timeoutMs}ms (${socketPath})`))
    }, timeoutMs)
    socket.once('connect', () => {
      clearTimeout(timer)
      resolve(socket)
    })
    socket.once('error', (err) => {
      clearTimeout(timer)
      reject(err)
    })
  })
}

// Send the first-line `hello` on a socket and await the `{type:'hello',ok}` reply.
function helloHandshake(socket, hello, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('hello timeout')), timeoutMs)
    const read = makeLineReader((obj) => {
      if (obj.type === 'hello') {
        clearTimeout(timer)
        socket.removeListener('data', onData)
        if (obj.ok) {
          resolve()
        } else {
          reject(new Error(`hello rejected: ${obj.error ?? 'unknown'}`))
        }
      }
    })
    const onData = (chunk) => read(chunk.toString('utf8'))
    socket.on('data', onData)
    socket.write(encodeNdjson(hello))
  })
}

export class DaemonSocketClient {
  #control = null
  #stream = null
  #pending = new Map() // id -> {resolve, reject, timer}
  #events = []
  #seq = 0
  #clientId
  #protocolVersion
  #token
  #timeoutMs

  constructor({ token, protocolVersion, clientId, timeoutMs = 8000 }) {
    this.#token = token
    this.#protocolVersion = protocolVersion
    this.#clientId = clientId
    this.#timeoutMs = timeoutMs
  }

  get clientId() {
    return this.#clientId
  }

  async connect(socketPath) {
    // Control socket FIRST, then stream — matching the real client (client.ts).
    // The Node daemon creates the per-clientId entry on the control hello and
    // destroys any stream hello that arrives with no entry yet
    // (daemon-server.ts). Both sockets connect before any createOrAttach, so
    // session output streams live to the stream socket.
    this.#control = await connectSocket(socketPath, this.#timeoutMs)
    await this.#hello(this.#control, 'control')
    const controlReader = makeLineReader((obj) => {
      const entry = obj.id ? this.#pending.get(obj.id) : undefined
      if (entry) {
        this.#pending.delete(obj.id)
        entry.resolve(obj)
      }
    })
    this.#control.on('data', (chunk) => controlReader(chunk.toString('utf8')))

    this.#stream = await connectSocket(socketPath, this.#timeoutMs)
    await this.#hello(this.#stream, 'stream')
    const streamReader = makeLineReader((obj) => {
      if (obj.type === 'event') {
        this.#events.push(obj)
      }
    })
    this.#stream.on('data', (chunk) => streamReader(chunk.toString('utf8')))
  }

  #hello(socket, role) {
    return helloHandshake(
      socket,
      {
        type: 'hello',
        version: this.#protocolVersion,
        token: this.#token,
        clientId: this.#clientId,
        role
      },
      this.#timeoutMs
    )
  }

  // Send an RPC request on the control socket; resolve with its id-correlated
  // RpcResponse ({id, ok, payload?|error?}).
  rpc(type, payload) {
    const id = `r${++this.#seq}`
    const req = payload === undefined ? { id, type } : { id, type, payload }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id)
        reject(new Error(`rpc ${type} timed out`))
      }, this.#timeoutMs)
      this.#pending.set(id, {
        resolve: (v) => {
          clearTimeout(timer)
          resolve(v)
        },
        reject
      })
      this.#control.write(encodeNdjson(req))
    })
  }

  // Snapshot the events seen so far (data/exit/terminalError), newest last.
  events() {
    return [...this.#events]
  }

  // Concatenated `data` event payloads for a session — the live output stream.
  streamData(sessionId) {
    return this.#events
      .filter((e) => e.event === 'data' && e.sessionId === sessionId)
      .map((e) => e.payload?.data ?? '')
      .join('')
  }

  close() {
    this.#control?.destroy()
    this.#stream?.destroy()
  }
}
