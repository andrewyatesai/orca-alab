// Browser-safe decoder for the v1020 binary stream plane, for the
// transport-agnostic DaemonProtocolClient (src/shared — no node imports, so it
// runs in the coordinator renderer as well as node harnesses). Uint8Array +
// DataView + TextDecoder only; the node-side twin is
// src/main/daemon/daemon-binary-stream-protocol.ts. Constants MUST stay in
// lockstep with src/main/daemon/daemon-frame-types.ts and the Rust encoder in
// rust/crates/orca-daemon/src/protocol.rs.

// The stream-hello field value that requests/echoes binary frames.
export const STREAM_FORMAT_BINARY = 'binary'
// The version at/after which the daemon speaks the binary stream plane.
export const BINARY_STREAM_PROTOCOL_VERSION = 1020

const FRAME_HEADER_SIZE = 5 // [type:u8][len:u32 BE]
const FRAME_TYPE_DATA = 0x01
const FRAME_TYPE_EVENT = 0x07
const SID_LEN_PREFIX_SIZE = 1

const utf8 = new TextDecoder('utf-8')

/** One decoded stream frame: a PTY data chunk (raw bytes → text) or a non-data
 *  event carried as its NDJSON-identical JSON text. */
export type DecodedStreamFrame =
  | { kind: 'data'; sessionId: string; data: string }
  | { kind: 'event'; json: string }

export type BinaryFrameReader = { feed: (chunk: Uint8Array) => void }

function concat(a: Uint8Array, b: Uint8Array): Uint8Array {
  if (a.length === 0) {
    return b
  }
  const out = new Uint8Array(a.length + b.length)
  out.set(a, 0)
  out.set(b, a.length)
  return out
}

/** Split a byte stream into v1020 stream frames. Data-frame payload is
 *  [sidLen:u8][sessionId][raw pty bytes]; the daemon pre-decodes each chunk to
 *  COMPLETE UTF-8 before framing, so per-frame TextDecode never strands a
 *  partial multibyte tail. Frames split across chunks reassemble; unknown frame
 *  types are tolerated (additive-forward). */
export function createBinaryFrameReader(
  onFrame: (frame: DecodedStreamFrame) => void
): BinaryFrameReader {
  let buffer = new Uint8Array(0)
  return {
    feed(chunk) {
      buffer = concat(buffer, chunk)
      while (buffer.length >= FRAME_HEADER_SIZE) {
        const view = new DataView(buffer.buffer, buffer.byteOffset, buffer.length)
        const len = view.getUint32(1, false)
        const total = FRAME_HEADER_SIZE + len
        if (buffer.length < total) {
          break
        }
        const type = buffer[0]
        const payload = buffer.subarray(FRAME_HEADER_SIZE, total)
        if (type === FRAME_TYPE_DATA) {
          const sidEnd = SID_LEN_PREFIX_SIZE + payload[0]
          onFrame({
            kind: 'data',
            sessionId: utf8.decode(payload.subarray(SID_LEN_PREFIX_SIZE, sidEnd)),
            data: utf8.decode(payload.subarray(sidEnd))
          })
        } else if (type === FRAME_TYPE_EVENT) {
          onFrame({ kind: 'event', json: utf8.decode(payload) })
        }
        buffer = buffer.subarray(total)
      }
    }
  }
}

/** Find the first newline (0x0A, never inside a UTF-8 multibyte sequence) — the
 *  hello reply is always an NDJSON line even on a binary stream, so callers
 *  decode the line before it and hand the RAW residual bytes after it to the
 *  binary reader. Returns null until a full line has arrived. */
export function splitFirstLine(bytes: Uint8Array): { line: string; rest: Uint8Array } | null {
  const nl = bytes.indexOf(0x0a)
  if (nl === -1) {
    return null
  }
  return { line: utf8.decode(bytes.subarray(0, nl)), rest: bytes.subarray(nl + 1) }
}
