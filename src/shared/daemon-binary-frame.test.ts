import { describe, it, expect } from 'vitest'
import {
  createBinaryFrameReader,
  splitFirstLine,
  STREAM_FORMAT_BINARY,
  BINARY_STREAM_PROTOCOL_VERSION,
  type DecodedStreamFrame
} from './daemon-binary-frame'
// The node/Rust-mirroring encoders: proving the browser-safe reader decodes what
// they emit is a cross-implementation parity check against the daemon wire.
import { encodeDataFrame, encodeEventFrame } from '../main/daemon/daemon-binary-stream-protocol'

const collect = (): {
  frames: DecodedStreamFrame[]
  reader: ReturnType<typeof createBinaryFrameReader>
} => {
  const frames: DecodedStreamFrame[] = []
  return { frames, reader: createBinaryFrameReader((f) => frames.push(f)) }
}

describe('daemon-binary-frame (browser-safe reader)', () => {
  it('constants match the daemon', () => {
    expect(STREAM_FORMAT_BINARY).toBe('binary')
    expect(BINARY_STREAM_PROTOCOL_VERSION).toBe(1020)
  })

  it('decodes a node-encoded Data frame (cross-impl parity) with raw control bytes', () => {
    const { frames, reader } = collect()
    reader.feed(encodeDataFrame('sess-1', 'a\x1b[32mgreen\x1b[0m'))
    expect(frames).toEqual([{ kind: 'data', sessionId: 'sess-1', data: 'a\x1b[32mgreen\x1b[0m' }])
  })

  it('preserves multibyte UTF-8 through a Data frame', () => {
    const { frames, reader } = collect()
    reader.feed(encodeDataFrame('s', '┌─┐ 🚀 café ▛▜'))
    expect(frames[0]).toEqual({ kind: 'data', sessionId: 's', data: '┌─┐ 🚀 café ▛▜' })
  })

  it('decodes an Event frame as its JSON text', () => {
    const { frames, reader } = collect()
    reader.feed(
      encodeEventFrame('{"type":"event","event":"exit","sessionId":"s","payload":{"code":0}}')
    )
    expect(frames).toEqual([
      {
        kind: 'event',
        json: '{"type":"event","event":"exit","sessionId":"s","payload":{"code":0}}'
      }
    ])
  })

  it('reassembles a frame split across two feeds', () => {
    const { frames, reader } = collect()
    const frame = encodeDataFrame('sid', 'hello world')
    reader.feed(frame.subarray(0, 7))
    expect(frames).toHaveLength(0)
    reader.feed(frame.subarray(7))
    expect(frames[0]).toEqual({ kind: 'data', sessionId: 'sid', data: 'hello world' })
  })

  it('drains multiple coalesced frames in order', () => {
    const { frames, reader } = collect()
    reader.feed(
      Buffer.concat([
        encodeDataFrame('s', 'one'),
        encodeDataFrame('s', 'two'),
        encodeEventFrame('{"type":"event","event":"exit","sessionId":"s","payload":{"code":1}}')
      ])
    )
    expect(frames.map((f) => (f.kind === 'data' ? f.data : 'exit'))).toEqual(['one', 'two', 'exit'])
  })

  it('tolerates an unknown frame type', () => {
    const { frames, reader } = collect()
    reader.feed(Uint8Array.from([0x7f, 0, 0, 0, 2, 0x41, 0x42]))
    expect(frames).toHaveLength(0)
    reader.feed(encodeDataFrame('s', 'ok'))
    expect(frames).toHaveLength(1)
  })

  describe('splitFirstLine', () => {
    it('returns null until a newline arrives, then splits line/residual at the byte level', () => {
      expect(splitFirstLine(Uint8Array.from(Buffer.from('{"ok":true}')))).toBeNull()
      const withResidual = Buffer.concat([
        Buffer.from('{"type":"hello","ok":true}\n'),
        encodeDataFrame('s', 'x')
      ])
      const split = splitFirstLine(withResidual)
      expect(split?.line).toBe('{"type":"hello","ok":true}')
      // The residual is the raw binary frame, uncorrupted.
      const { frames, reader } = collect()
      reader.feed(split!.rest)
      expect(frames).toEqual([{ kind: 'data', sessionId: 's', data: 'x' }])
    })
  })
})
