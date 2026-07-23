// v1020 binary stream plane — the opt-in wire format on the daemon→client STREAM
// socket. PTY output rides as RAW bytes (no JSON \uXXXX escape expansion, no
// per-chunk stringify/parse); non-data events ride as their NDJSON-identical
// JSON text inside an Event frame, so the client keeps exactly one parser.
//
// The frame envelope is binary-frame.ts: [type:u8][len:u32 BE][payload].
// This module must stay byte-identical to the daemon encoder in
// rust/crates/orca-daemon/src/protocol.rs (data_frame / event_frame).
import { FrameType, FRAME_HEADER_SIZE } from './types'
import { encodeFrame, createFrameParser } from './binary-frame'
import type { DaemonEvent } from './daemon-stream-events'

// The stream-hello field value that requests/echoes binary frames. Must equal
// STREAM_FORMAT_BINARY in rust/crates/orca-daemon/src/protocol.rs.
export const STREAM_FORMAT_BINARY = 'binary'

// Data-frame payload layout: [sessionIdLen:u8][sessionId utf8][raw pty bytes].
// A session id that would overflow the u8 length prefix (>255 bytes) is
// delivered as a JSON data event in an Event frame instead, so the decoder
// never sees an oversized id.
const SID_LEN_PREFIX_SIZE = 1
const MAX_SID_LEN = 0xff

// ─── Encoders — mirror the daemon side in protocol.rs ───────────────────────
// Production framing lives in the Rust daemon; these exist so the in-tree bench
// and parity tests exercise the exact wire the client decodes. Any change here
// must move in lockstep with data_frame/event_frame in protocol.rs.

/** The NDJSON-identical `data` event JSON — the oversized-sid fallback payload
 *  and what the legacy NDJSON stream sends. Must equal `data_event` in
 *  protocol.rs. */
export function dataEventJson(sessionId: string, data: string): string {
  return JSON.stringify({ type: 'event', event: 'data', sessionId, payload: { data } })
}

/** A PTY-output Data frame: `[sidLen:u8][sessionId][raw bytes]`. A session id
 *  too long for the u8 prefix falls back to a JSON data event in an Event frame
 *  — which the parser already routes through its normal event path. */
export function encodeDataFrame(sessionId: string, data: string): Buffer {
  const sid = Buffer.from(sessionId, 'utf8')
  if (sid.length > MAX_SID_LEN) {
    return encodeEventFrame(dataEventJson(sessionId, data))
  }
  const body = Buffer.from(data, 'utf8')
  const payload = Buffer.allocUnsafe(SID_LEN_PREFIX_SIZE + sid.length + body.length)
  payload[0] = sid.length
  sid.copy(payload, SID_LEN_PREFIX_SIZE)
  body.copy(payload, SID_LEN_PREFIX_SIZE + sid.length)
  return encodeFrame(FrameType.Data, payload)
}

/** A non-data stream event carried as its NDJSON-identical JSON text inside an
 *  Event frame, so the binary stream needs exactly one parser. */
export function encodeEventFrame(eventJson: string): Buffer {
  return encodeFrame(FrameType.Event, Buffer.from(eventJson, 'utf8'))
}

export { FRAME_HEADER_SIZE }

/** Decode one Data-frame payload into its session id + raw output text. The
 *  daemon runs every chunk through its Utf8StreamDecoder before framing, so a
 *  payload's pty bytes are always a COMPLETE UTF-8 run — safe to decode
 *  per-frame without carrying a partial multibyte tail across frames. */
export function decodeDataFramePayload(payload: Buffer): { sessionId: string; data: string } {
  const sidEnd = SID_LEN_PREFIX_SIZE + payload[0]
  return {
    sessionId: payload.toString('utf8', SID_LEN_PREFIX_SIZE, sidEnd),
    data: payload.toString('utf8', sidEnd)
  }
}

export type BinaryStreamParser = {
  feed(chunk: Buffer): void
  reset(): void
}

/** A stream parser for the v1020 binary plane. Feed it RAW socket bytes (never
 *  through a StringDecoder — frames are binary) and it emits fully-formed
 *  DaemonEvents identical to what the NDJSON parser would emit, so the client's
 *  event path is unchanged. Data frames reconstruct a data event from raw
 *  bytes; Event frames carry the NDJSON JSON text, parsed as-is. Unknown frame
 *  types are tolerated (additive-forward), mirroring the NDJSON path's
 *  unknown-event tolerance. */
export function createBinaryStreamParser(
  onEvent: (event: DaemonEvent) => void,
  onError?: (err: Error) => void
): BinaryStreamParser {
  const parser = createFrameParser(
    (type, payload) => {
      try {
        if (type === FrameType.Data) {
          const { sessionId, data } = decodeDataFramePayload(payload)
          onEvent({ type: 'event', event: 'data', sessionId, payload: { data } })
          return
        }
        if (type === FrameType.Event) {
          const msg = JSON.parse(payload.toString('utf8')) as DaemonEvent
          if (msg.type === 'event') {
            onEvent(msg)
          }
        }
      } catch (err) {
        onError?.(err instanceof Error ? err : new Error(String(err)))
      }
    },
    // Surface oversized-frame rejections so a corrupted/hostile length prefix
    // can't silently wedge the stream parser.
    (err) => onError?.(err)
  )
  return {
    feed: (chunk: Buffer) => parser.feed(chunk),
    reset: () => parser.reset()
  }
}
