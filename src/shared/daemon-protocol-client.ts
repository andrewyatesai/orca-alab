// The daemon NDJSON socket-protocol client, lifted from tools/daemon-parity's
// parity-proven DaemonSocketClient (docs/rust-migration/coordinator-v0-design.md):
// a `hello` handshake per socket, one clientId shared across a control (RPC) +
// stream (events) pair, id-correlated RpcResponses on control, and
// data/exit/terminalError events on stream (src/main/daemon/types.ts).
//
// Transport-agnostic on purpose: no node imports, so the coordinator renderer
// can run it over the preload byte tunnel while main-process/harness callers
// run it over node:net (daemon-socket-transport.ts).

export type DaemonSocketRole = 'control' | 'stream'

/** One duplex byte channel to the daemon socket. The client attaches exactly
 *  one data listener and one close listener. */
export type DaemonByteTransport = {
  send: (data: string) => void
  onData: (listener: (chunk: string) => void) => void
  onClose: (listener: () => void) => void
  close: () => void
}

/** Credentials the transport layer learned while opening (the coordinator
 *  tunnel's open ack carries the token/version main resolved from the daemon
 *  runtime dir; a direct-socket caller already knows them and omits this). */
export type DaemonTransportAuth = {
  token: string
  protocolVersion: number
}

export type DaemonTransportConnection = {
  transport: DaemonByteTransport
  auth?: DaemonTransportAuth
}

export type DaemonTransportFactory = (role: DaemonSocketRole) => Promise<DaemonTransportConnection>

export type DaemonRpcResponse<T = unknown> =
  | { id: string; ok: true; payload: T }
  | { id: string; ok: false; error: string }

/** A stream-socket event line (daemon-stream-events.ts shapes, held loose so
 *  this client never lags a tolerated additive event). */
export type DaemonStreamEvent = {
  type: 'event'
  event: string
  sessionId: string
  payload?: Record<string, unknown>
}

type HelloReplyLine = { type?: unknown; ok?: unknown; error?: unknown }

/** Split a socket's byte stream into JSON objects on newline boundaries —
 *  mirrors the daemon's resync-on-newline framing (orca-net::ndjson /
 *  ndjson.ts). Malformed lines are skipped: a live client must resync on the
 *  next newline, not tear down the pair. */
export function makeNdjsonLineReader(onObject: (value: unknown) => void): (chunk: string) => void {
  let buffer = ''
  return (chunk) => {
    buffer += chunk
    let newline = buffer.indexOf('\n')
    while (newline !== -1) {
      const line = buffer.slice(0, newline)
      buffer = buffer.slice(newline + 1)
      newline = buffer.indexOf('\n')
      if (line.trim()) {
        try {
          onObject(JSON.parse(line))
        } catch {
          // Resync on the next newline (the daemon never emits partial lines;
          // this guards a torn buffer during teardown).
        }
      }
    }
  }
}

const encodeNdjsonLine = (message: unknown): string => `${JSON.stringify(message)}\n`

type PendingRpc = {
  settle: (response: DaemonRpcResponse) => void
  fail: (error: Error) => void
}

export type DaemonProtocolClientOptions = {
  clientId: string
  openTransport: DaemonTransportFactory
  /** Known upfront for direct-socket callers; omit to adopt the transport's
   *  auth (the coordinator tunnel resolves it main-side). */
  token?: string
  protocolVersion?: number
  timeoutMs?: number
}

export class DaemonProtocolClient {
  #control: DaemonByteTransport | null = null
  #stream: DaemonByteTransport | null = null
  #pending = new Map<string, PendingRpc>()
  #eventListeners = new Set<(event: DaemonStreamEvent) => void>()
  #closeListeners = new Set<() => void>()
  #seq = 0
  #closed = false
  readonly #clientId: string
  readonly #openTransport: DaemonTransportFactory
  readonly #timeoutMs: number
  #token: string | undefined
  #protocolVersion: number | undefined

  constructor(options: DaemonProtocolClientOptions) {
    this.#clientId = options.clientId
    this.#openTransport = options.openTransport
    this.#token = options.token
    this.#protocolVersion = options.protocolVersion
    this.#timeoutMs = options.timeoutMs ?? 8000
  }

  get clientId(): string {
    return this.#clientId
  }

  /** The negotiated protocol version (after connect, when transport-supplied). */
  get protocolVersion(): number | undefined {
    return this.#protocolVersion
  }

  async connect(): Promise<void> {
    // Control socket FIRST, then stream — the daemon creates the per-clientId
    // entry on the control hello and destroys a stream hello that arrives with
    // no entry yet (daemon-server.ts / connection.rs).
    this.#control = await this.#openAndGreet('control', (line) => this.#routeControlLine(line))
    this.#stream = await this.#openAndGreet('stream', (line) => this.#routeStreamLine(line))
  }

  /** Send an RPC on the control socket; resolves with the id-correlated
   *  RpcResponse ({id, ok, payload?|error?}) — an ok:false response RESOLVES
   *  (it is a daemon answer); only transport loss/timeouts reject. */
  rpc<T = unknown>(type: string, payload?: Record<string, unknown>): Promise<DaemonRpcResponse<T>> {
    const control = this.#control
    if (!control || this.#closed) {
      return Promise.reject(new Error(`rpc ${type} on a disconnected daemon client`))
    }
    const id = `r${++this.#seq}`
    const request = payload === undefined ? { id, type } : { id, type, payload }
    return new Promise<DaemonRpcResponse<T>>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id)
        reject(new Error(`rpc ${type} timed out after ${this.#timeoutMs}ms`))
      }, this.#timeoutMs)
      this.#pending.set(id, {
        settle: (response) => {
          clearTimeout(timer)
          resolve(response as DaemonRpcResponse<T>)
        },
        fail: (error) => {
          clearTimeout(timer)
          reject(error)
        }
      })
      control.send(encodeNdjsonLine(request))
    })
  }

  /** Subscribe to stream-socket events (data/exit/…). Returns unsubscribe. */
  onEvent(listener: (event: DaemonStreamEvent) => void): () => void {
    this.#eventListeners.add(listener)
    return () => this.#eventListeners.delete(listener)
  }

  /** Fires once when either underlying transport closes (or on close()). */
  onClose(listener: () => void): () => void {
    this.#closeListeners.add(listener)
    return () => this.#closeListeners.delete(listener)
  }

  close(): void {
    this.#teardown(new Error('daemon client closed'))
  }

  async #openAndGreet(
    role: DaemonSocketRole,
    route: (line: unknown) => void
  ): Promise<DaemonByteTransport> {
    const { transport, auth } = await this.#openTransport(role)
    this.#token ??= auth?.token
    this.#protocolVersion ??= auth?.protocolVersion
    if (this.#token === undefined || this.#protocolVersion === undefined) {
      transport.close()
      throw new Error(
        'daemon auth unresolved: pass token/protocolVersion or a transport that supplies them'
      )
    }
    let settleHello: ((error: Error | null) => void) | null = null
    const helloReply = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`hello (${role}) timed out after ${this.#timeoutMs}ms`)),
        this.#timeoutMs
      )
      settleHello = (error) => {
        clearTimeout(timer)
        settleHello = null
        if (error) {
          reject(error)
        } else {
          resolve()
        }
      }
    })
    // One reader per socket: it consumes the hello reply first, then hands
    // every later line to the role's router.
    const reader = makeNdjsonLineReader((line) => {
      if (settleHello) {
        const reply = line as HelloReplyLine
        if (reply.type === 'hello') {
          settleHello(
            reply.ok === true ? null : new Error(`hello rejected: ${reply.error ?? 'unknown'}`)
          )
        }
        return
      }
      route(line)
    })
    transport.onData(reader)
    transport.onClose(() => {
      settleHello?.(new Error(`daemon ${role} socket closed during hello`))
      this.#teardown(new Error(`daemon ${role} socket closed`))
    })
    transport.send(
      encodeNdjsonLine({
        type: 'hello',
        version: this.#protocolVersion,
        token: this.#token,
        clientId: this.#clientId,
        role
      })
    )
    try {
      await helloReply
    } catch (error) {
      transport.close()
      throw error
    }
    return transport
  }

  #routeControlLine(line: unknown): void {
    const response = line as DaemonRpcResponse
    if (typeof response?.id !== 'string') {
      return
    }
    const entry = this.#pending.get(response.id)
    if (entry) {
      this.#pending.delete(response.id)
      entry.settle(response)
    }
  }

  #routeStreamLine(line: unknown): void {
    const event = line as DaemonStreamEvent
    if (event?.type !== 'event') {
      return
    }
    for (const listener of this.#eventListeners) {
      listener(event)
    }
  }

  #teardown(cause: Error): void {
    if (this.#closed) {
      return
    }
    this.#closed = true
    for (const entry of this.#pending.values()) {
      entry.fail(cause)
    }
    this.#pending.clear()
    this.#control?.close()
    this.#stream?.close()
    for (const listener of this.#closeListeners) {
      listener()
    }
    this.#closeListeners.clear()
    this.#eventListeners.clear()
  }
}
