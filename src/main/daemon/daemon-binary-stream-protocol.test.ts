import { describe, it, expect } from 'vitest'
import { createNdjsonParser } from './ndjson'
import {
  STREAM_FORMAT_BINARY,
  encodeDataFrame,
  encodeEventFrame,
  dataEventJson,
  decodeDataFramePayload,
  createBinaryStreamParser
} from './daemon-binary-stream-protocol'
import { FRAME_HEADER_SIZE } from './types'
import type { DaemonEvent } from './daemon-stream-events'

const exitEventJson = (sessionId: string, code: number): string =>
  JSON.stringify({ type: 'event', event: 'exit', sessionId, payload: { code } })

const collect = (): {
  events: DaemonEvent[]
  parser: ReturnType<typeof createBinaryStreamParser>
} => {
  const events: DaemonEvent[] = []
  const parser = createBinaryStreamParser((e) => events.push(e))
  return { events, parser }
}

describe('daemon-binary-stream-protocol', () => {
  it('the negotiation token matches the daemon constant', () => {
    expect(STREAM_FORMAT_BINARY).toBe('binary')
  })

  it('decodes a Data frame into a data event with raw control bytes intact', () => {
    const { events, parser } = collect()
    // ESC survives as one raw byte — the whole point vs NDJSON's 6-byte .
    parser.feed(encodeDataFrame('sess-1', 'a\x1b[32mgreen\x1b[0m'))
    expect(events).toEqual([
      {
        type: 'event',
        event: 'data',
        sessionId: 'sess-1',
        payload: { data: 'a\x1b[32mgreen\x1b[0m' }
      }
    ])
  })

  it('preserves multibyte UTF-8 (emoji / box-drawing) through a Data frame', () => {
    const { events, parser } = collect()
    const data = '┌─┐ 🚀 café ▛▜'
    parser.feed(encodeDataFrame('s', data))
    expect(events[0]).toMatchObject({ event: 'data', payload: { data } })
  })

  it('routes an Event frame through JSON as a normal stream event', () => {
    const { events, parser } = collect()
    parser.feed(encodeEventFrame(exitEventJson('s', 0)))
    expect(events).toEqual([{ type: 'event', event: 'exit', sessionId: 's', payload: { code: 0 } }])
  })

  it('reassembles a frame split across two feed() chunks', () => {
    const { events, parser } = collect()
    const frame = encodeDataFrame('sid', 'hello world')
    const cut = FRAME_HEADER_SIZE + 3
    parser.feed(frame.subarray(0, cut))
    expect(events).toHaveLength(0) // incomplete: nothing yet
    parser.feed(frame.subarray(cut))
    expect(events[0]).toMatchObject({ event: 'data', payload: { data: 'hello world' } })
  })

  it('drains multiple frames coalesced into one chunk in order', () => {
    const { events, parser } = collect()
    parser.feed(
      Buffer.concat([
        encodeDataFrame('s', 'one'),
        encodeDataFrame('s', 'two'),
        encodeEventFrame(exitEventJson('s', 3))
      ])
    )
    expect(
      events.map((e) =>
        e.event === 'data'
          ? e.payload.data
          : `exit:${(e as { payload: { code: number } }).payload.code}`
      )
    ).toEqual(['one', 'two', 'exit:3'])
  })

  it('tolerates an unknown frame type without throwing or emitting', () => {
    const { events, parser } = collect()
    const unknown = Buffer.from([0x7f, 0x00, 0x00, 0x00, 0x02, 0x41, 0x42]) // type 0x7f, len 2
    expect(() => parser.feed(unknown)).not.toThrow()
    expect(events).toHaveLength(0)
    // A valid frame after the tolerated one still parses.
    parser.feed(encodeDataFrame('s', 'ok'))
    expect(events).toHaveLength(1)
  })

  it('falls back to an Event-framed data event for a session id over 255 bytes', () => {
    const { events, parser } = collect()
    const sid = 's'.repeat(300)
    parser.feed(encodeDataFrame(sid, 'x'))
    // Still surfaces as an ordinary data event — the client can't tell the
    // encoder took the fallback path.
    expect(events).toEqual([
      { type: 'event', event: 'data', sessionId: sid, payload: { data: 'x' } }
    ])
  })

  it('decodeDataFramePayload round-trips id + data directly', () => {
    // Reconstruct the payload the way the parser hands it over (post-header).
    const frame = encodeDataFrame('abc', 'DATA\x00\x07')
    const payload = frame.subarray(FRAME_HEADER_SIZE)
    expect(decodeDataFramePayload(payload)).toEqual({ sessionId: 'abc', data: 'DATA\x00\x07' })
  })

  describe('wire parity: binary frames decode to the SAME events as NDJSON', () => {
    it('produces byte-identical event streams over a mixed corpus', () => {
      const corpus: { sessionId: string; data: string }[] = [
        { sessionId: 'a', data: 'plain shell line\r\n' },
        { sessionId: 'a', data: '\x1b[38;5;42m\x1b[1mbold color\x1b[0m' },
        { sessionId: 'b', data: '┌──────┐\r\n│ box  │\r\n└──────┘' },
        { sessionId: 'b', data: '🚀🔥 progress 100% ✓' },
        { sessionId: 'c', data: '\x00\x01\x02\x07\x08 control bytes' }
      ]

      const binaryEvents: DaemonEvent[] = []
      const binary = createBinaryStreamParser((e) => binaryEvents.push(e))
      for (const { sessionId, data } of corpus) {
        binary.feed(encodeDataFrame(sessionId, data))
      }

      const ndjsonEvents: DaemonEvent[] = []
      const ndjson = createNdjsonParser((m) => ndjsonEvents.push(m as DaemonEvent))
      for (const { sessionId, data } of corpus) {
        ndjson.feed(`${dataEventJson(sessionId, data)}\n`)
      }

      expect(binaryEvents).toEqual(ndjsonEvents)
    })
  })
})
