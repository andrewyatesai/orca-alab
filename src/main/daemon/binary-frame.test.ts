import { describe, expect, it, vi } from 'vitest'
import { encodeFrame, createFrameParser, FrameType, FRAME_HEADER_SIZE } from './binary-frame'
import { FRAME_MAX_PAYLOAD } from './types'

/** Build a raw frame header with an arbitrary (possibly oversized) declared
 *  payload length, bypassing encodeFrame's encode-side bound. */
function rawHeader(type: FrameType, declaredLength: number): Buffer {
  const header = Buffer.alloc(FRAME_HEADER_SIZE)
  header[0] = type
  header.writeUInt32BE(declaredLength, 1)
  return header
}

describe('encodeFrame', () => {
  it('encodes a data frame with correct header', () => {
    const payload = Buffer.from('hello')
    const frame = encodeFrame(FrameType.Data, payload)

    expect(frame.length).toBe(FRAME_HEADER_SIZE + payload.length)
    expect(frame[0]).toBe(FrameType.Data)
    expect(frame.readUInt32BE(1)).toBe(payload.length)
    expect(frame.subarray(FRAME_HEADER_SIZE).toString()).toBe('hello')
  })

  it('encodes a resize frame', () => {
    const payload = Buffer.from(JSON.stringify({ cols: 120, rows: 40 }))
    const frame = encodeFrame(FrameType.Resize, payload)

    expect(frame[0]).toBe(FrameType.Resize)
    expect(frame.readUInt32BE(1)).toBe(payload.length)
  })

  it('encodes an exit frame with exit code', () => {
    const payload = Buffer.from(JSON.stringify({ code: 0 }))
    const frame = encodeFrame(FrameType.Exit, payload)

    expect(frame[0]).toBe(FrameType.Exit)
    const decoded = JSON.parse(frame.subarray(FRAME_HEADER_SIZE).toString())
    expect(decoded.code).toBe(0)
  })

  it('encodes empty payload', () => {
    const frame = encodeFrame(FrameType.Kill, Buffer.alloc(0))

    expect(frame.length).toBe(FRAME_HEADER_SIZE)
    expect(frame[0]).toBe(FrameType.Kill)
    expect(frame.readUInt32BE(1)).toBe(0)
  })

  it('throws on payload exceeding max size', () => {
    const oversized = Buffer.alloc(1024 * 1024 + 1)
    expect(() => encodeFrame(FrameType.Data, oversized)).toThrow()
  })
})

describe('createFrameParser', () => {
  it('parses a single complete frame', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const payload = Buffer.from('test data')
    const frame = encodeFrame(FrameType.Data, payload)
    parser.feed(frame)

    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][0]).toBe(FrameType.Data)
    expect(onFrame.mock.calls[0][1].toString()).toBe('test data')
  })

  it('parses multiple frames in one chunk', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const frame1 = encodeFrame(FrameType.Data, Buffer.from('one'))
    const frame2 = encodeFrame(FrameType.Data, Buffer.from('two'))
    parser.feed(Buffer.concat([frame1, frame2]))

    expect(onFrame).toHaveBeenCalledTimes(2)
    expect(onFrame.mock.calls[0][1].toString()).toBe('one')
    expect(onFrame.mock.calls[1][1].toString()).toBe('two')
  })

  it('handles frame split across header boundary', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const payload = Buffer.from('split test')
    const frame = encodeFrame(FrameType.Data, payload)

    // Split in the middle of the header (3 bytes, then the rest)
    parser.feed(frame.subarray(0, 3))
    expect(onFrame).not.toHaveBeenCalled()

    parser.feed(frame.subarray(3))
    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][1].toString()).toBe('split test')
  })

  it('handles frame split in the middle of payload', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const payload = Buffer.from('payload split')
    const frame = encodeFrame(FrameType.Data, payload)

    // Split after header + 3 bytes of payload
    const splitPoint = FRAME_HEADER_SIZE + 3
    parser.feed(frame.subarray(0, splitPoint))
    expect(onFrame).not.toHaveBeenCalled()

    parser.feed(frame.subarray(splitPoint))
    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][1].toString()).toBe('payload split')
  })

  it('handles byte-by-byte feeding', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const frame = encodeFrame(FrameType.Exit, Buffer.from('{"code":42}'))

    for (let i = 0; i < frame.length; i++) {
      parser.feed(frame.subarray(i, i + 1))
    }

    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][0]).toBe(FrameType.Exit)
    expect(JSON.parse(onFrame.mock.calls[0][1].toString())).toEqual({ code: 42 })
  })

  it('handles zero-length payload frame', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const frame = encodeFrame(FrameType.Kill, Buffer.alloc(0))
    parser.feed(frame)

    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][0]).toBe(FrameType.Kill)
    expect(onFrame.mock.calls[0][1].length).toBe(0)
  })

  it('parses different frame types correctly', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    const frames = [
      encodeFrame(FrameType.Data, Buffer.from('data')),
      encodeFrame(FrameType.Resize, Buffer.from('resize')),
      encodeFrame(FrameType.Signal, Buffer.from('signal'))
    ]

    parser.feed(Buffer.concat(frames))

    expect(onFrame).toHaveBeenCalledTimes(3)
    expect(onFrame.mock.calls[0][0]).toBe(FrameType.Data)
    expect(onFrame.mock.calls[1][0]).toBe(FrameType.Resize)
    expect(onFrame.mock.calls[2][0]).toBe(FrameType.Signal)
  })

  it('does not buffer toward an oversized declared payload length', () => {
    const onFrame = vi.fn()
    const onError = vi.fn()
    const parser = createFrameParser(onFrame, onError)

    // Header declares the max u32 (~4GiB) but only a trickle of bytes follows.
    parser.feed(rawHeader(FrameType.Data, 0xffffffff))
    for (let i = 0; i < 1000; i++) {
      parser.feed(Buffer.alloc(64))
    }

    // The oversized frame is discarded, not buffered toward 4GiB.
    expect(onError).toHaveBeenCalled()
    expect(onError.mock.calls[0][0].message).toMatch(/too large/i)
    expect(onFrame).not.toHaveBeenCalled()
  })

  it('stays synchronized after discarding an oversized frame', () => {
    const onFrame = vi.fn()
    const onError = vi.fn()
    const parser = createFrameParser(onFrame, onError)

    const oversizeLength = FRAME_MAX_PAYLOAD + 1
    // Oversized frame delivered across chunks, followed by a valid frame.
    parser.feed(rawHeader(FrameType.Data, oversizeLength))
    parser.feed(Buffer.alloc(oversizeLength - 100))
    const good = encodeFrame(FrameType.Exit, Buffer.from('{"code":0}'))
    parser.feed(Buffer.concat([Buffer.alloc(100), good]))

    expect(onError).toHaveBeenCalledTimes(1)
    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][0]).toBe(FrameType.Exit)
    expect(JSON.parse(onFrame.mock.calls[0][1].toString())).toEqual({ code: 0 })
  })

  it('accepts a frame at exactly FRAME_MAX_PAYLOAD', () => {
    const onFrame = vi.fn()
    const onError = vi.fn()
    const parser = createFrameParser(onFrame, onError)

    const payload = Buffer.alloc(FRAME_MAX_PAYLOAD, 0x61)
    parser.feed(Buffer.concat([rawHeader(FrameType.Data, FRAME_MAX_PAYLOAD), payload]))

    expect(onError).not.toHaveBeenCalled()
    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][1].length).toBe(FRAME_MAX_PAYLOAD)
  })

  it('resets buffer state', () => {
    const onFrame = vi.fn()
    const parser = createFrameParser(onFrame)

    // Feed partial frame then reset
    const frame = encodeFrame(FrameType.Data, Buffer.from('lost'))
    parser.feed(frame.subarray(0, 3))
    parser.reset()

    // Feed a new complete frame
    const frame2 = encodeFrame(FrameType.Data, Buffer.from('fresh'))
    parser.feed(frame2)

    expect(onFrame).toHaveBeenCalledOnce()
    expect(onFrame.mock.calls[0][1].toString()).toBe('fresh')
  })
})
