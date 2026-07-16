// The daemon NDJSON socket-protocol client, lifted from tools/daemon-parity's
// parity-proven DaemonSocketClient (docs/rust-migration/coordinator-v0-design.md):
// a `hello` handshake per socket, one clientId shared across a control (RPC) +
// stream (events) pair, id-correlated RpcResponses on control, and
// data/exit/terminalError events on stream (src/main/daemon/types.ts).
//
// Transport-agnostic on purpose: no node imports, so the coordinator renderer
// can run it over the preload byte tunnel while main-process/harness callers
// run it over node:net (daemon-socket-transport.ts).

import {
  STREAM_FORMAT_BINARY,
  BINARY_STREAM_PROTOCOL_VERSION,
  createBinaryFrameReader,
  splitFirstLine,
  type DecodedStreamFrame
} from './daemon-binary-frame'

export type DaemonSocketRole = 'control' | 'stream'

/** One duplex byte channel to the daemon socket. Sends are small JSON control
 *  lines (hello/RPC) so `send` stays string; receives are RAW BYTES so the
 *  stream socket can carry the v1020 binary frame plane (which a UTF-8 decode
 *  would corrupt) — the NDJSON path decodes them itself. The client attaches
 *  exactly one data listener and one close listener. */
export type DaemonByteTransport = {
  send: (data: string) => void
  onData: (listener: (chunk: Uint8Array) => void) => void
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

type HelloReplyLine = { type?: unknown; ok?: unknown; error?: unknown; streamFormat?: unknown }

function concatBytes(a: Uint8Array, b: Uint8Array): Uint8Array {
  if (a.length === 0) {
    return b
  }
  const out = new Uint8Array(a.length + b.length)
  out.set(a, 0)
  out.set(b, a.length)
  return out
}

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
  /** Request the v1020 binary stream plane on the stream socket (default true).
   *  Safe: the daemon only switches formats when it echoes the grant, so a
   *  daemon that doesn't support it leaves this client on NDJSON. Ignored when
   *  the negotiated version is below BINARY_STREAM_PROTOCOL_VERSION. */
  preferBinaryStream?: boolean
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
  readonly #preferBinaryStream: boolean
  #token: string | undefined
  #protocolVersion: number | undefined

  constructor(options: DaemonProtocolClientOptions) {
    this.#clientId = options.clientId
    this.#openTransport = options.openTransport
    this.#token = options.token
    this.#protocolVersion = options.protocolVersion
    this.#timeoutMs = options.timeoutMs ?? 8000
    this.#preferBinaryStream = options.preferBinaryStream ?? true
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
    // Only the stream socket negotiates binary frames, and only when the version
    // knows them. The daemon still replies hello_ok as an NDJSON line either
    // way, echoing streamFormat to grant.
    const requestBinary =
      role === 'stream' &&
      this.#preferBinaryStream &&
      this.#protocolVersion >= BINARY_STREAM_PROTOCOL_VERSION

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

    // The negotiated body reader (NDJSON lines or v1020 binary frames). A binary
    // Data frame is routed as the SAME data-event object shape the NDJSON path
    // yields, so downstream consumers are format-agnostic.
    const bodyDecoder = new TextDecoder('utf-8')
    const ndjsonBody = makeNdjsonLineReader((line) => route(line))
    const binaryBody = createBinaryFrameReader((frame: DecodedStreamFrame) => {
      if (frame.kind === 'data') {
        route({
          type: 'event',
          event: 'data',
          sessionId: frame.sessionId,
          payload: { data: frame.data }
        })
        return
      }
      try {
        route(JSON.parse(frame.json))
      } catch {
        // Resync: the daemon never emits a partial Event frame.
      }
    })

    // Bytes before the hello newline are the (ASCII JSON) hello line; bytes
    // after are the body — binary frames when granted, else NDJSON. Split at the
    // BYTE level so binary residual reaches the frame reader uncorrupted.
    let granted = false
    let greeted = false
    // Why the annotation: concatBytes() returns Uint8Array<ArrayBufferLike>, which
    // the narrower Uint8Array<ArrayBuffer> the constructor infers cannot re-hold.
    let helloBuffer: Uint8Array = new Uint8Array(0)
    const feedBody = (bytes: Uint8Array): void => {
      if (bytes.length === 0) {
        return
      }
      if (granted) {
        binaryBody.feed(bytes)
      } else {
        ndjsonBody(bodyDecoder.decode(bytes, { stream: true }))
      }
    }
    transport.onData((chunk) => {
      if (greeted) {
        feedBody(chunk)
        return
      }
      helloBuffer = concatBytes(helloBuffer, chunk)
      const split = splitFirstLine(helloBuffer)
      if (!split) {
        return
      }
      greeted = true
      let reply: HelloReplyLine
      try {
        reply = JSON.parse(split.line) as HelloReplyLine
      } catch {
        settleHello?.(new Error(`invalid hello (${role}) reply`))
        return
      }
      granted = requestBinary && reply.streamFormat === STREAM_FORMAT_BINARY
      if (reply.type === 'hello') {
        settleHello?.(
          reply.ok === true ? null : new Error(`hello rejected: ${reply.error ?? 'unknown'}`)
        )
      }
      feedBody(split.rest)
    })
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
        role,
        ...(requestBinary ? { streamFormat: STREAM_FORMAT_BINARY } : {})
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
