import { describe, it, expect } from 'vitest'
import { DaemonProtocolClient, type DaemonStreamEvent } from './daemon-protocol-client'
import { encodeDataFrame, encodeEventFrame } from '../main/daemon/daemon-binary-stream-protocol'

// A scripted byte transport: it auto-answers the client's hello (via `replyFor`,
// which sees the parsed hello so it can grant/deny binary and coalesce a first
// frame), records what the client sent, and lets the test push more bytes after
// connect. Mirrors the real daemon closely enough to exercise the client's
// byte-level hello split + format switch without a daemon or Electron.
function scriptedTransport(replyFor: (hello: Record<string, unknown>) => Uint8Array) {
  let onData: ((chunk: Uint8Array) => void) | null = null
  let onClose: (() => void) | null = null
  const sent: string[] = []
  return {
    sent,
    push: (bytes: Uint8Array) => onData?.(bytes),
    triggerClose: () => onClose?.(),
    transport: {
      send: (data: string) => {
        sent.push(data)
        const msg = JSON.parse(data) as Record<string, unknown>
        if (msg.type === 'hello') {
          const reply = replyFor(msg)
          queueMicrotask(() => onData?.(reply))
        }
      },
      onData: (l: (chunk: Uint8Array) => void) => {
        onData = l
      },
      onClose: (l: () => void) => {
        onClose = l
      },
      close: () => {}
    }
  }
}

const ndjsonHello = (extra = ''): Uint8Array => Buffer.from(`{"type":"hello","ok":true${extra}}\n`)
const grantHello = (): Uint8Array => ndjsonHello(`,"streamFormat":"binary"`)

function buildClient(
  ctrlReply: (h: Record<string, unknown>) => Uint8Array,
  streamReply: (h: Record<string, unknown>) => Uint8Array,
  preferBinaryStream = true
) {
  const ctrl = scriptedTransport(ctrlReply)
  const stream = scriptedTransport(streamReply)
  const client = new DaemonProtocolClient({
    clientId: 'c1',
    token: 't',
    protocolVersion: 1020,
    preferBinaryStream,
    openTransport: (role) =>
      Promise.resolve({ transport: role === 'control' ? ctrl.transport : stream.transport })
  })
  const events: DaemonStreamEvent[] = []
  client.onEvent((e) => events.push(e))
  return { client, ctrl, stream, events }
}

describe('DaemonProtocolClient — v1020 binary stream negotiation', () => {
  it('requests binary on the stream hello (not control) at version >= 1020', async () => {
    const { client, ctrl, stream } = buildClient(
      () => ndjsonHello(),
      () => grantHello()
    )
    await client.connect()
    expect(JSON.parse(ctrl.sent[0]).streamFormat).toBeUndefined()
    expect(JSON.parse(stream.sent[0])).toMatchObject({ role: 'stream', streamFormat: 'binary' })
    client.close()
  })

  it('decodes binary Data + Event frames into the same event shape as NDJSON', async () => {
    const { client, stream, events } = buildClient(
      () => ndjsonHello(),
      () => grantHello()
    )
    await client.connect()
    stream.push(encodeDataFrame('s1', 'out\x1b[32mgreen\x1b[0m'))
    stream.push(
      encodeEventFrame('{"type":"event","event":"exit","sessionId":"s1","payload":{"code":0}}')
    )
    expect(events).toEqual([
      {
        type: 'event',
        event: 'data',
        sessionId: 's1',
        payload: { data: 'out\x1b[32mgreen\x1b[0m' }
      },
      { type: 'event', event: 'exit', sessionId: 's1', payload: { code: 0 } }
    ])
    client.close()
  })

  it('handles the hello line + first binary frame coalesced in one chunk', async () => {
    const { client, events } = buildClient(
      () => ndjsonHello(),
      // Grant + a first data frame in the SAME reply chunk (the busy-reconnect case).
      () => Buffer.concat([grantHello(), encodeDataFrame('s', 'coalesced')])
    )
    await client.connect()
    // The coalesced frame was decoded from the post-hello residual, no push.
    expect(events).toEqual([
      { type: 'event', event: 'data', sessionId: 's', payload: { data: 'coalesced' } }
    ])
    client.close()
  })

  it('reassembles a binary frame split across two transport chunks', async () => {
    const { client, stream, events } = buildClient(
      () => ndjsonHello(),
      () => grantHello()
    )
    await client.connect()
    const frame = encodeDataFrame('s', 'split-across-chunks')
    stream.push(frame.subarray(0, 8))
    expect(events).toHaveLength(0)
    stream.push(frame.subarray(8))
    expect(events[0]).toEqual({
      type: 'event',
      event: 'data',
      sessionId: 's',
      payload: { data: 'split-across-chunks' }
    })
    client.close()
  })

  it('stays on NDJSON when the daemon does not echo the grant', async () => {
    const { client, stream, events } = buildClient(
      () => ndjsonHello(),
      () => ndjsonHello() // ok, but no streamFormat -> NDJSON
    )
    await client.connect()
    stream.push(
      Buffer.from('{"type":"event","event":"data","sessionId":"s","payload":{"data":"ndjson"}}\n')
    )
    expect(events).toEqual([
      { type: 'event', event: 'data', sessionId: 's', payload: { data: 'ndjson' } }
    ])
    client.close()
  })

  it('does not request binary when preferBinaryStream is false', async () => {
    const { client, stream } = buildClient(
      () => ndjsonHello(),
      () => ndjsonHello(),
      false
    )
    await client.connect()
    expect(JSON.parse(stream.sent[0]).streamFormat).toBeUndefined()
    client.close()
  })
})
