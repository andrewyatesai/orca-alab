export { FrameType } from './types'
import type { FrameType } from './types'
import { FRAME_HEADER_SIZE, FRAME_MAX_PAYLOAD } from './types'

export { FRAME_HEADER_SIZE }

export function encodeFrame(type: FrameType, payload: Buffer): Buffer {
  if (payload.length > FRAME_MAX_PAYLOAD) {
    throw new Error(`Frame payload ${payload.length} exceeds max ${FRAME_MAX_PAYLOAD}`)
  }

  const frame = Buffer.allocUnsafe(FRAME_HEADER_SIZE + payload.length)
  frame[0] = type
  frame.writeUInt32BE(payload.length, 1)
  payload.copy(frame, FRAME_HEADER_SIZE)
  return frame
}

export type FrameParser = {
  feed(chunk: Buffer): void
  reset(): void
}

export function createFrameParser(
  onFrame: (type: FrameType, payload: Buffer) => void,
  onError?: (err: Error) => void
): FrameParser {
  let buffer: Buffer = Buffer.alloc(0)
  // Bytes of an oversized (discarded) frame's payload still owed by the peer.
  let discardBytesRemaining = 0

  function parse(): void {
    for (;;) {
      // Drain the tail of a frame we already rejected before touching headers,
      // so trickled payload bytes are never re-interpreted as a new header.
      if (discardBytesRemaining > 0) {
        const drop = Math.min(discardBytesRemaining, buffer.length)
        buffer = buffer.subarray(drop)
        discardBytesRemaining -= drop
        if (discardBytesRemaining > 0) {
          return
        }
      }

      if (buffer.length < FRAME_HEADER_SIZE) {
        return
      }

      const payloadLength = buffer.readUInt32BE(1)

      // Why: the u32 length is attacker/corruption-controlled (0..4GiB). Without
      // this bound feed() would Buffer.concat toward `totalLength` on every
      // socket 'data' event — unbounded memory + O(n²) re-copy (slow-loris/OOM).
      // Discard the oversized frame's bytes as they arrive (like the SSH
      // FrameDecoder) so the stream stays synchronized rather than throwing
      // mid-buffer and corrupting every future frame.
      if (payloadLength > FRAME_MAX_PAYLOAD) {
        const available = buffer.length - FRAME_HEADER_SIZE
        const drop = Math.min(available, payloadLength)
        buffer = buffer.subarray(FRAME_HEADER_SIZE + drop)
        discardBytesRemaining = payloadLength - drop
        onError?.(new Error(`Frame payload too large: ${payloadLength} bytes — discarded`))
        continue
      }

      const totalLength = FRAME_HEADER_SIZE + payloadLength
      if (buffer.length < totalLength) {
        return
      }

      const type = buffer[0] as FrameType
      const payload = buffer.subarray(FRAME_HEADER_SIZE, totalLength)
      buffer = buffer.subarray(totalLength)

      onFrame(type, payload)
    }
  }

  return {
    feed(chunk: Buffer): void {
      buffer = buffer.length === 0 ? chunk : Buffer.concat([buffer, chunk])
      parse()
    },

    reset(): void {
      buffer = Buffer.alloc(0)
      discardBytesRemaining = 0
    }
  }
}
