// ─── Binary Frame Protocol ──────────────────────────────────────────
// Re-exported via ./types (the line-capped wire-shape entry point) so importers
// keep one entry point. Two producers frame with this envelope: the daemon↔PTY
// subprocess channel, and the v1020 daemon→client binary stream plane (see
// daemon-binary-stream-protocol.ts). Must stay in lockstep with the frame
// constants in rust/crates/orca-daemon/src/protocol.rs.
//
// 5-byte header: [type:1][length:4 big-endian], followed by `length` payload bytes.

export const enum FrameType {
  Data = 0x01,
  Resize = 0x02,
  Exit = 0x03,
  Error = 0x04,
  Kill = 0x05,
  Signal = 0x06,
  // v1020 binary stream plane: a non-data stream event (exit, etc.) carried as
  // its NDJSON-identical JSON text inside a frame, so the binary stream needs
  // exactly one parser. Must equal FRAME_TYPE_EVENT in protocol.rs.
  Event = 0x07
}

export const FRAME_HEADER_SIZE = 5
export const FRAME_MAX_PAYLOAD = 1024 * 1024 // 1MB
