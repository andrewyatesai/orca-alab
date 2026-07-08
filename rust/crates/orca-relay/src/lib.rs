//! `orca-relay` — remote/mobile transport for Orca (replaces the `ws`-based relay).
//!
//! Starts with the terminal binary-stream framing that multiplexes terminal
//! output/input/resize/snapshot traffic over a single connection. JSON payloads
//! ride the vendored `serde_json`; the frame header is hand-rolled bytes.

mod base64;
// The encrypted-channel reducer is the only crypto consumer; behind the default
// `e2ee` feature so pure dependents (aggregate dispatch → relay/renderer wasm)
// opt out and keep the NaCl-box stack out of a crypto-free artifact.
#[cfg(feature = "e2ee")]
pub mod e2ee_channel;
pub mod pairing;
pub mod terminal_stream;

#[cfg(feature = "e2ee")]
pub use e2ee_channel::{E2eeChannel, E2eeEffect, RawMessage, HANDSHAKE_TIMEOUT_MS, MAX_BINARY_BUFFERED_AMOUNT};
pub use pairing::{
    decode_pairing_offer, encode_pairing_offer, parse_pairing_code, PairingOffer, PairingScope,
    PAIRING_OFFER_VERSION,
};
pub use terminal_stream::{
    decode_terminal_stream_frame, decode_terminal_stream_json, decode_terminal_stream_text,
    encode_terminal_stream_frame, encode_terminal_stream_json, encode_terminal_stream_text,
    TerminalStreamFrame, TerminalStreamOpcode,
};


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod browser_screencast_protocol;

pub use browser_screencast_protocol::{decode_browser_screencast_frame, encode_browser_screencast_frame, BrowserScreencastFormat, BrowserScreencastFrame, BrowserScreencastOpcode};
