/* eslint-disable max-lines -- Why: daemon handshake, RPC, stream events, and reconnect cleanup share one socket lifecycle. */
import { connect, type Socket } from 'node:net'
import { readFileSync } from 'node:fs'
import { randomUUID } from 'node:crypto'
import { StringDecoder } from 'node:string_decoder'
import { encodeNdjson, createNdjsonParser } from './ndjson'
import {
  CLEAN_DISCONNECT_PROTOCOL_VERSION,
  FORK_DAEMON_PROTOCOL_NAMESPACE_START,
  PROTOCOL_VERSION,
  BINARY_STREAM_PROTOCOL_VERSION,
  NOTIFY_PREFIX,
  DaemonProtocolError
} from './types'
import type {
  DaemonEndpointIdentity,
  HelloMessage,
  HelloResponse,
  RpcResponse,
  DaemonEvent
} from './types'
import { STREAM_FORMAT_BINARY, createBinaryStreamParser } from './daemon-binary-stream-protocol'
import { addNodePtyRecoveryHint } from './node-pty-error-hints'

const CONNECT_TIMEOUT_MS = 5000
const CONNECTION_ATTEMPT_WAIT_MS = CONNECT_TIMEOUT_MS * 4
const REQUEST_TIMEOUT_MS = 30000

// Why the EBUSY retry: the Windows named-pipe daemon pre-arms a spare pipe
// instance, but a dial can still land in the single-CreateNamedPipeW window (or
// a multi-client burst) where every instance is taken — libuv surfaces that
// ERROR_PIPE_BUSY as EBUSY. It clears as soon as the accept loop re-arms, so a
// short bounded backoff beats failing the connection. Unix socket connects never
// produce EBUSY, so this is inert off Windows.
const PIPE_BUSY_RETRY_DELAYS_MS = [25, 50]

export async function connectWithPipeBusyRetry(dial: () => Promise<Socket>): Promise<Socket> {
  let attempt = 0
  for (;;) {
    try {
      return await dial()
    } catch (err) {
      const code = (err as NodeJS.ErrnoException).code
      if (code !== 'EBUSY' || attempt >= PIPE_BUSY_RETRY_DELAYS_MS.length) {
        throw err
      }
      await new Promise((resolve) => setTimeout(resolve, PIPE_BUSY_RETRY_DELAYS_MS[attempt]))
      attempt += 1
    }
  }
}

// v1020 binary stream plane: request raw-byte frames on the stream socket by
// default. It's always safe — the daemon only switches formats when it echoes
// the grant, so a daemon that doesn't support it (or has it disabled) leaves
// both ends on NDJSON. Kill-switch: ORCA_DAEMON_STREAM_NDJSON=1 forces NDJSON.
const BINARY_STREAM_DEFAULT = process.env.ORCA_DAEMON_STREAM_NDJSON !== '1'

export type DaemonClientOptions = {
  socketPath: string
  tokenPath: string
  protocolVersion?: number
  // Opt out of the binary stream plane (defaults to BINARY_STREAM_DEFAULT).
  preferBinaryStream?: boolean
}

type PendingRequest = {
  resolve: (value: unknown) => void
  reject: (reason: Error) => void
  timer: ReturnType<typeof setTimeout>
}

export class DaemonClient {
  private socketPath: string
  private tokenPath: string
  private protocolVersion: number
  private preferBinaryStream: boolean
  private clientId = randomUUID()

  private controlSocket: Socket | null = null
  private streamSocket: Socket | null = null
  private connected = false
  private disconnectArmed = false
  // Why: after a disconnect + reconnect (daemon respawn), a stale 'close'
  // event from the old sockets can fire. Without a generation check, that
  // event would tear down the fresh connection. Each doConnect() increments
  // the generation; handleDisconnect ignores events from old generations.
  private connectionGeneration = 0
  // Why: multiple concurrent spawn() calls from simultaneous pane mounts
  // all call ensureConnected(). Without a lock, each starts a separate
  // connection attempt, overwriting sockets and triggering "Connection lost".
  private connectingPromise: Promise<void> | null = null
  private connectionAttemptGeneration = 0
  private daemonIdentity: DaemonEndpointIdentity | null = null
  private observedAuthenticatedDisconnect = false

  private pendingRequests = new Map<string, PendingRequest>()
  private eventListeners: ((event: unknown) => void)[] = []
  private disconnectedListeners: (() => void)[] = []
  private requestCounter = 0
  private cleanupSocketListeners: (() => void) | null = null

  constructor(opts: DaemonClientOptions) {
    this.socketPath = opts.socketPath
    this.tokenPath = opts.tokenPath
    this.protocolVersion = opts.protocolVersion ?? PROTOCOL_VERSION
    this.preferBinaryStream = opts.preferBinaryStream ?? BINARY_STREAM_DEFAULT
  }

  isConnected(): boolean {
    return this.connected
  }

  getDaemonIdentity(): DaemonEndpointIdentity | null {
    return this.daemonIdentity ? { ...this.daemonIdentity } : null
  }

  hasObservedAuthenticatedDisconnect(): boolean {
    return this.observedAuthenticatedDisconnect
  }

  async ensureConnected(): Promise<void> {
    return this.ensureConnectedWithTimeout(CONNECT_TIMEOUT_MS, false)
  }

  async ensureConnectedWithin(timeoutMs: number): Promise<void> {
    return this.ensureConnectedWithTimeout(timeoutMs, true)
  }

  private async ensureConnectedWithTimeout(
    timeoutMs: number,
    sharedBudget: boolean
  ): Promise<void> {
    if (this.connected) {
      return
    }
    if (this.connectingPromise) {
      // Why: a normal connection may legitimately consume one timeout for each
      // socket and hello; bounded teardown calls instead keep their one shared budget.
      const waiterTimeoutMs = sharedBudget ? timeoutMs : CONNECTION_ATTEMPT_WAIT_MS
      return this.waitForConnectionAttempt(this.connectingPromise, waiterTimeoutMs)
    }

    const attemptGeneration = this.connectionAttemptGeneration
    this.connectingPromise = this.doConnect(timeoutMs, attemptGeneration, sharedBudget)
    try {
      await this.connectingPromise
    } finally {
      this.connectingPromise = null
    }
  }

  private async doConnect(
    timeoutMs: number,
    attemptGeneration: number,
    sharedBudget: boolean
  ): Promise<void> {
    const token = readFileSync(this.tokenPath, 'utf-8').trim()
    const deadlineMs = Date.now() + timeoutMs
    const remainingMs = (): number =>
      sharedBudget ? Math.max(1, deadlineMs - Date.now()) : timeoutMs
    const pendingListenerCleanups: (() => void)[] = []
    const cleanupPendingListeners = (): void => {
      for (const cleanup of pendingListenerCleanups.splice(0)) {
        cleanup()
      }
    }

    try {
      // Sequential: control first, then stream. Feed each hello's residual bytes into
      // the matching parser so events/responses coalesced into the hello packet survive.
      const pendingControlSocket = await this.connectSocket(remainingMs())
      this.assertConnectionAttemptCurrent(attemptGeneration, pendingControlSocket)
      this.controlSocket = pendingControlSocket
      const control = await this.sendHello(this.controlSocket, token, 'control', remainingMs())
      this.assertConnectionAttemptCurrent(attemptGeneration, this.controlSocket)
      pendingListenerCleanups.push(this.setupControlParser(this.controlSocket, control.residual))

      const pendingStreamSocket = await this.connectSocket(remainingMs())
      this.assertConnectionAttemptCurrent(attemptGeneration, pendingStreamSocket)
      this.streamSocket = pendingStreamSocket
      // Only request binary frames when the negotiated version knows them; a
      // hello asking a pre-1020 daemon for streamFormat would just be ignored,
      // but gating keeps the handshake honest about what it expects.
      const requestBinary =
        this.preferBinaryStream && this.protocolVersion >= BINARY_STREAM_PROTOCOL_VERSION
      const stream = await this.sendHello(
        this.streamSocket,
        token,
        'stream',
        remainingMs(),
        requestBinary
      )
      this.assertConnectionAttemptCurrent(attemptGeneration, this.streamSocket)
      if (!sameDaemonIdentity(control.identity, stream.identity)) {
        throw new DaemonProtocolError('Daemon identity changed during connection')
      }
      pendingListenerCleanups.push(
        this.setupStreamParser(this.streamSocket, stream.grantedBinaryStream, stream.residual)
      )

      this.assertConnectionAttemptCurrent(attemptGeneration)
      this.connected = true
      this.observedAuthenticatedDisconnect = false
      this.daemonIdentity = control.identity
      this.disconnectArmed = true
      this.connectionGeneration++

      const gen = this.connectionGeneration
      const handleClose = () => this.handleDisconnect(gen)
      const controlSocket = this.controlSocket
      const streamSocket = this.streamSocket
      controlSocket.on('close', handleClose)
      controlSocket.on('error', handleClose)
      streamSocket.on('close', handleClose)
      streamSocket.on('error', handleClose)
      pendingListenerCleanups.push(() => {
        controlSocket.off('close', handleClose)
        controlSocket.off('error', handleClose)
        streamSocket.off('close', handleClose)
        streamSocket.off('error', handleClose)
      })
      this.cleanupSocketListeners = cleanupPendingListeners
    } catch (error) {
      cleanupPendingListeners()
      this.controlSocket?.destroy()
      this.streamSocket?.destroy()
      this.controlSocket = null
      this.streamSocket = null
      this.connected = false
      this.daemonIdentity = null
      this.disconnectArmed = false
      throw error
    }
  }

  async request<T = unknown>(
    type: string,
    payload: unknown,
    timeoutMs = REQUEST_TIMEOUT_MS
  ): Promise<T> {
    if (!this.connected || !this.controlSocket) {
      throw new DaemonProtocolError('Not connected')
    }

    const id = `req-${++this.requestCounter}`
    const msg = { id, type, ...(payload !== undefined ? { payload } : {}) }

    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingRequests.delete(id)
        reject(new DaemonProtocolError(`Request ${type} timed out after ${timeoutMs}ms`))
      }, timeoutMs)

      this.pendingRequests.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timer
      })

      this.controlSocket!.write(encodeNdjson(msg))
    })
  }

  notify(type: string, payload: unknown): void {
    if (!this.connected || !this.controlSocket) {
      return
    }

    const id = `${NOTIFY_PREFIX}${++this.requestCounter}`
    const msg = { id, type, ...(payload !== undefined ? { payload } : {}) }
    this.controlSocket.write(encodeNdjson(msg))
  }

  onEvent(listener: (event: unknown) => void): () => void {
    this.eventListeners.push(listener)
    return () => {
      const idx = this.eventListeners.indexOf(listener)
      if (idx !== -1) {
        this.eventListeners.splice(idx, 1)
      }
    }
  }

  onDisconnected(listener: () => void): () => void {
    this.disconnectedListeners.push(listener)
    return () => {
      const idx = this.disconnectedListeners.indexOf(listener)
      if (idx !== -1) {
        this.disconnectedListeners.splice(idx, 1)
      }
    }
  }

  disconnect(): void {
    this.connectionAttemptGeneration++
    this.connected = false
    this.daemonIdentity = null
    this.disconnectArmed = false
    this.cleanupActiveSocketListeners()

    for (const [id, pending] of this.pendingRequests) {
      clearTimeout(pending.timer)
      pending.reject(new DaemonProtocolError('Disconnected'))
      this.pendingRequests.delete(id)
    }

    this.controlSocket?.destroy()
    this.streamSocket?.destroy()
    this.controlSocket = null
    this.streamSocket = null
  }

  private connectSocket(timeoutMs: number): Promise<Socket> {
    return connectWithPipeBusyRetry(() => this.connectSocketOnce(timeoutMs))
  }

  private connectSocketOnce(timeoutMs: number): Promise<Socket> {
    return new Promise((resolve, reject) => {
      const socket = connect(this.socketPath)
      const cleanup = (): void => {
        clearTimeout(timer)
        socket.removeListener('connect', onConnect)
        socket.removeListener('error', onError)
      }
      const onConnect = (): void => {
        cleanup()
        resolve(socket)
      }
      const onError = (err: Error): void => {
        cleanup()
        reject(err)
      }
      const timer = setTimeout(() => {
        cleanup()
        socket.destroy()
        reject(new DaemonProtocolError('Connection timed out'))
      }, timeoutMs)

      socket.on('connect', onConnect)
      socket.on('error', onError)
    })
  }

  private waitForConnectionAttempt(attempt: Promise<void>, timeoutMs: number): Promise<void> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new DaemonProtocolError('Connection attempt wait timed out'))
      }, timeoutMs)
      attempt.then(
        () => {
          clearTimeout(timer)
          resolve()
        },
        (error) => {
          clearTimeout(timer)
          reject(error)
        }
      )
    })
  }

  private assertConnectionAttemptCurrent(attemptGeneration: number, socket?: Socket): void {
    if (attemptGeneration === this.connectionAttemptGeneration) {
      return
    }
    socket?.destroy()
    throw new DaemonProtocolError('Disconnected')
  }

  // Resolves with any bytes that arrived AFTER the hello line in the same packet —
  // the daemon can write hello_ok and the first stream data/exit events in one write,
  // so on a busy stream socket they coalesce into one read. The caller must feed this
  // residual into the parser or that terminal output is lost.
  private sendHello(
    socket: Socket,
    token: string,
    role: 'control' | 'stream',
    timeoutMs: number,
    requestBinaryStream = false
  ): Promise<{
    identity: DaemonEndpointIdentity | null
    residual: Buffer
    grantedBinaryStream: boolean
  }> {
    return new Promise((resolve, reject) => {
      const hello: HelloMessage = {
        type: 'hello',
        version: this.protocolVersion,
        token,
        clientId: this.clientId,
        role,
        // Only stream-role hellos carry this; the daemon echoes it to grant.
        ...(requestBinaryStream ? { streamFormat: STREAM_FORMAT_BINARY } : {})
      }

      let buffer: Buffer = Buffer.alloc(0)
      let settled = false
      let timer: ReturnType<typeof setTimeout> | null = null
      const cleanup = (): void => {
        if (timer) {
          clearTimeout(timer)
          timer = null
        }
        socket.removeListener('data', onData)
        socket.removeListener('error', onError)
        socket.removeListener('close', onClose)
      }
      const finish = (
        error?: Error,
        result?: {
          identity: DaemonEndpointIdentity | null
          residual: Buffer
          grantedBinaryStream: boolean
        }
      ): void => {
        if (settled) {
          return
        }
        settled = true
        cleanup()
        if (error) {
          reject(error)
          return
        }
        resolve(result ?? { identity: null, residual: Buffer.alloc(0), grantedBinaryStream: false })
      }
      // Why: buffer raw BYTES and split on the '\n' byte (0x0A never occurs inside a
      // multibyte UTF-8 sequence), so the hello line decodes cleanly AND the residual
      // stays byte-exact for the parser's own decoder — decoding here with a throwaway
      // StringDecoder would strand a partial multibyte tail on handoff.
      const onData = (chunk: Buffer): void => {
        buffer = buffer.length === 0 ? chunk : Buffer.concat([buffer, chunk])
        const newlineIdx = buffer.indexOf(0x0a)
        if (newlineIdx === -1) {
          return
        }

        const line = buffer.subarray(0, newlineIdx).toString('utf8')
        // Copy the post-hello bytes out of the shared buffer so the caller can feed
        // them to the stream/control parser instead of losing them.
        const residual = Buffer.from(buffer.subarray(newlineIdx + 1))
        try {
          const response = JSON.parse(line) as HelloResponse
          if (response.ok) {
            const identity = parseDaemonEndpointIdentity(response.daemonIdentity)
            // Identity is REQUIRED from public daemons at v24+ (upstream #9277
            // clean-disconnect contract), but the fork's Rust daemon (1000+
            // namespace) doesn't publish an endpoint identity yet, so
            // fork-namespace versions stay exempt. A malformed identity is
            // always rejected.
            const identityRequired =
              this.protocolVersion >= CLEAN_DISCONNECT_PROTOCOL_VERSION &&
              this.protocolVersion < FORK_DAEMON_PROTOCOL_NAMESPACE_START
            if (
              (identityRequired && identity === null) ||
              (response.daemonIdentity !== undefined && identity === null)
            ) {
              finish(new DaemonProtocolError('Invalid daemon identity'))
              return
            }
            // The daemon grants binary frames ONLY by echoing streamFormat; its
            // absence (any older daemon, or one that declined) keeps NDJSON.
            finish(undefined, {
              identity,
              residual,
              grantedBinaryStream: response.streamFormat === STREAM_FORMAT_BINARY
            })
          } else {
            finish(
              new DaemonProtocolError(addNodePtyRecoveryHint(response.error ?? 'Hello rejected'))
            )
          }
        } catch {
          finish(new DaemonProtocolError('Invalid hello response'))
        }
      }
      const onError = (error: Error): void => finish(error)
      const onClose = (): void =>
        finish(new DaemonProtocolError('Connection closed before hello response'))

      timer = setTimeout(() => {
        // Why: a stale daemon can accept the socket but never answer hello;
        // without a handshake timeout, startup waits forever on ensureConnected().
        finish(new DaemonProtocolError('Hello response timed out'))
        socket.destroy()
      }, timeoutMs)
      socket.on('data', onData)
      socket.on('error', onError)
      socket.on('close', onClose)
      socket.write(encodeNdjson(hello))
    })
  }

  // `initial` is the residual returned by sendHello — bytes that coalesced in after
  // the hello line — and MUST be fed through this parser's own decoder first so a
  // partial multibyte tail carries into the next chunk.
  private setupControlParser(socket: Socket, initial?: Buffer): () => void {
    // Why: control responses may contain terminal/startup data with multibyte
    // text; keep incomplete UTF-8 bytes until the next socket chunk.
    const decoder = new StringDecoder('utf8')
    const parser = createNdjsonParser(
      (msg) => {
        const response = msg as RpcResponse
        if (response.id) {
          const pending = this.pendingRequests.get(response.id)
          if (pending) {
            this.pendingRequests.delete(response.id)
            clearTimeout(pending.timer)
            if (response.ok) {
              pending.resolve(response.payload)
            } else {
              pending.reject(new DaemonProtocolError(addNodePtyRecoveryHint(response.error)))
            }
          }
        }
      },
      () => {} // Ignore parse errors on control socket
    )

    const onData = (chunk: Buffer) => parser.feed(decoder.write(chunk))
    if (initial && initial.length > 0) {
      parser.feed(decoder.write(initial))
    }
    socket.on('data', onData)
    return () => socket.off('data', onData)
  }

  private setupStreamParser(socket: Socket, binary: boolean, initial?: Buffer): () => void {
    const dispatch = (event: DaemonEvent): void => {
      if (event.type === 'event') {
        for (const listener of this.eventListeners) {
          listener(event)
        }
      }
    }

    if (binary) {
      // Binary frames are raw bytes: feed the socket Buffer straight in — a
      // StringDecoder would corrupt frame bytes. The daemon pre-decodes each
      // chunk to complete UTF-8 before framing, so per-frame decode never
      // strands a partial multibyte tail; the frame parser handles frames split
      // across socket packets.
      const parser = createBinaryStreamParser(dispatch, () => {})
      const onData = (chunk: Buffer): void => parser.feed(chunk)
      if (initial && initial.length > 0) {
        parser.feed(initial)
      }
      socket.on('data', onData)
      return () => socket.off('data', onData)
    }

    // Why: PTY output streams include emoji/box-drawing tables; socket chunks
    // can split those UTF-8 sequences across packets.
    const decoder = new StringDecoder('utf8')
    const parser = createNdjsonParser(
      (msg) => dispatch(msg as DaemonEvent),
      () => {} // Ignore parse errors on stream socket
    )

    const onData = (chunk: Buffer): void => parser.feed(decoder.write(chunk))
    // Drain the post-hello residual BEFORE attaching the live listener so the first
    // coalesced data/exit event isn't lost on a busy-session reconnect.
    if (initial && initial.length > 0) {
      parser.feed(decoder.write(initial))
    }
    socket.on('data', onData)
    return () => socket.off('data', onData)
  }

  private handleDisconnect(generation: number): void {
    if (!this.disconnectArmed || generation !== this.connectionGeneration) {
      return
    }
    this.disconnectArmed = false
    this.connectionAttemptGeneration++
    if (this.daemonIdentity) {
      this.observedAuthenticatedDisconnect = true
    }
    this.connected = false
    this.daemonIdentity = null
    this.cleanupActiveSocketListeners()

    for (const [id, pending] of this.pendingRequests) {
      clearTimeout(pending.timer)
      pending.reject(new DaemonProtocolError('Connection lost'))
      this.pendingRequests.delete(id)
    }

    this.controlSocket?.destroy()
    this.streamSocket?.destroy()
    this.controlSocket = null
    this.streamSocket = null

    for (const listener of this.disconnectedListeners) {
      listener()
    }
  }

  private cleanupActiveSocketListeners(): void {
    const cleanup = this.cleanupSocketListeners
    this.cleanupSocketListeners = null
    cleanup?.()
  }
}

function parseDaemonEndpointIdentity(value: unknown): DaemonEndpointIdentity | null {
  if (!value || typeof value !== 'object') {
    return null
  }
  const identity = value as { pid?: unknown; startedAtMs?: unknown; launchNonce?: unknown }
  if (
    !Number.isSafeInteger(identity.pid) ||
    (identity.pid as number) <= 0 ||
    typeof identity.startedAtMs !== 'number' ||
    !Number.isFinite(identity.startedAtMs) ||
    identity.startedAtMs <= 0 ||
    typeof identity.launchNonce !== 'string' ||
    identity.launchNonce.length === 0
  ) {
    return null
  }
  return {
    pid: identity.pid as number,
    startedAtMs: identity.startedAtMs,
    launchNonce: identity.launchNonce
  }
}

function sameDaemonIdentity(
  left: DaemonEndpointIdentity | null,
  right: DaemonEndpointIdentity | null
): boolean {
  return (
    (left === null && right === null) ||
    (left !== null &&
      right !== null &&
      left.pid === right.pid &&
      left.startedAtMs === right.startedAtMs &&
      left.launchNonce === right.launchNonce)
  )
}
