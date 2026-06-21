// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Hierarchical-session routing fabric — the per-edge authority + write seam.
//! (design `docs/design/HIERARCHICAL_SESSIONS.md` §§2, 4, 6.3, 7).
//!
//! This crate is the frontend/transport substrate that lets one terminal drive
//! another: stable [`SessionId`]s + per-launch [`LaunchNonce`]s ([`id`]); the
//! per-edge, op-scoped, fail-closed authority table ([`Op`]/[`Edge`]/[`EdgeToken`]/
//! [`EdgeTable`]/[`decide_edge`]); and the single byte sink that serializes every
//! writer to one PTY master with whole-frame atomicity ([`sink::SinkWriter`]).
//!
//! It performs NO filesystem or socket I/O itself — the GUI owns those (headless
//! invariant). It is the policy + serialization core that the control socket and
//! the file veneer call into.
//!
//! ## Status
//!
//! Design-proposed (Phase 0). The COARSE compile-time class gate lives in
//! `aterm_cap::effects::{ReadScreen, WriteInput, SignalEdge}`; the FINE per-edge
//! object identity (which `src → dst` for which op) is here. Per §7.7, cross-session
//! `WriteInput` by untrusted IN-PROCESS code stays compile-gated off until ROADMAP
//! §5.4 is GREEN; the same-uid, cross-process path rides the runtime [`EdgeToken`]
//! over the uid-checked control socket and is sound independent of §5.4.

#![forbid(unsafe_code)]

mod edge;
mod id;
pub mod sink;

pub use edge::{decide_edge, Edge, EdgeDecision, EdgeTable, EdgeToken, Op};
pub use id::{LaunchNonce, SessionId};

/// Lowercase-hex encode. No dependency — ids/tokens are short and fixed-length.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 0x0f) as usize] as char);
    }
    s
}

/// Decode lowercase/uppercase hex into `out`, requiring exactly `out.len() * 2`
/// hex chars. Returns `None` on any length or non-hex error.
pub(crate) fn from_hex(s: &str, out: &mut [u8]) -> Option<()> {
    if s.len() != out.len() * 2 {
        return None;
    }
    for (slot, chunk) in out.iter_mut().zip(s.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(chunk).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(())
}

/// Constant-time byte equality for secret / anti-spoof comparisons: no early-out on
/// the first differing byte. A length mismatch returns `false` immediately (the
/// values compared here are fixed-length, so length is not itself a secret).
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Fill `buf` with cryptographically-secure random bytes from the OS CSPRNG.
/// Panics only if the OS RNG is unavailable (a fatal startup condition).
pub(crate) fn fill_random(buf: &mut [u8]) {
    use rand_core::RngCore;
    rand_core::OsRng.fill_bytes(buf);
}
