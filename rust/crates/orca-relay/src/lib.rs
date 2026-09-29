//! `orca-relay` — remote/mobile transport for Orca (replaces the `ws`-based relay).
//!
//! Starts with the terminal binary-stream framing that multiplexes terminal
//! output/input/resize/snapshot traffic over a single connection. JSON payloads
//! ride the vendored `serde_json`; the frame header is hand-rolled bytes.

// Trust contracts: `trustc` sets `cfg(trust_verify)` itself whenever verification
// runs (`targo trust`); the cfg is off under `targo --unverified` and under the
// stock wasm32 lane (orca-git-wasm), where this gating keeps the crate building. terminal_stream.rs carries a `trust::ensures` that was a hard
// E0433 the moment verification turned on — the contract landed without this
// registration, invisible while nothing set the cfg.
#![cfg_attr(trust_verify, feature(register_tool))]
#![cfg_attr(trust_verify, register_tool(trust))]

mod base64;
pub mod canonical_https_origin;
// The encrypted-channel reducer is the only crypto consumer; behind the default
// `e2ee` feature so pure dependents (aggregate dispatch → relay/renderer wasm)
// opt out and keep the NaCl-box stack out of a crypto-free artifact.
#[cfg(feature = "e2ee")]
pub mod e2ee_channel;
pub mod pairing;
pub mod pairing_offer_schema;
pub mod terminal_stream;

#[cfg(feature = "e2ee")]
pub use e2ee_channel::{E2eeChannel, E2eeEffect, RawMessage, HANDSHAKE_TIMEOUT_MS, MAX_BINARY_BUFFERED_AMOUNT};
pub use pairing::{
    decode_pairing_offer, encode_pairing_offer, parse_pairing_code, PairingOffer, PairingRelay,
    PairingScope,
};
pub use pairing_offer_schema::{
    is_canonical_base64_key, pairing_offer_to_json, validate_pairing_offer,
    PAIRING_CODE_MAX_CHARACTERS, PAIRING_DEVICE_TOKEN_MAX_CHARACTERS,
    PAIRING_ENDPOINT_MAX_CHARACTERS, PAIRING_INPUT_MAX_CHARACTERS, PAIRING_OFFER_VERSION,
    PAIRING_PUBLIC_KEY_MAX_CHARACTERS,
};
pub use terminal_stream::{
    decode_terminal_stream_frame, decode_terminal_stream_json, decode_terminal_stream_text,
    encode_terminal_stream_frame, encode_terminal_stream_json, encode_terminal_stream_text,
    TerminalStreamFrame, TerminalStreamOpcode,
};


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod browser_screencast_protocol;

pub use browser_screencast_protocol::{decode_browser_screencast_frame, encode_browser_screencast_frame, BrowserScreencastFormat, BrowserScreencastFrame, BrowserScreencastOpcode};
