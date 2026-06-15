// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

#![deny(unsafe_op_in_unsafe_fn)]
// F11-4 (#7941): production unwrap()/expect() forbidden; tests opt out
// per `#[allow(clippy::unwrap_used)]` at their module boundary.
#![deny(clippy::unwrap_used)]

//! VT100/ANSI escape sequence parser.
//!
//! ## Design
//!
//! Table-driven state machine based on the
//! [vt100.net DEC ANSI parser](https://vt100.net/emu/dec_ansi_parser).
//!
//! ## Verification
//!
//! - TLA+ spec: `tla/Parser.tla`
//! - Kani proofs: see `proofs.rs` + `proofs_utf8.rs` (27 harnesses), including:
//!   - Core invariants: `parser_never_panics`, `params_bounded`, `intermediates_bounded`,
//!     `state_always_valid`, `printable_slice_is_valid_utf8`.
//!   - Parameter safety: `param_accumulation_saturates`, `param_finalize_bounded`,
//!     `param_many_digits_safe`, `param_semicolon_safe`.
//!   - Transition/termination safety: `state_transitions_all_valid`, `state_transitions_sequential_valid`,
//!     `c1_controls_valid_transitions_when_enabled`, `c1_controls_no_state_change_when_disabled`,
//!     `escape_sequence_terminates`, `csi_sequence_terminates`, `osc_sequence_terminates`,
//!     `dcs_sequence_terminates`, `cancel_returns_to_ground`, `utf8_continuation_safe`,
//!     `utf8_malformed_sequences_preserve_decoder_invariants`, `transition_table_lookup_safe`.
//!   - SIMD safety: `simd_avx2_offset_no_overflow`, `simd_neon_offset_no_overflow`,
//!     `simd_pointer_within_bounds`, `simd_scalar_fallback_range_valid`,
//!     `simd_scalar_predicate_equivalence`, `simd_avx2_bias_correct`.
//! - Fuzz target: `fuzz/fuzz_targets/parser.rs`
//!
//! ## Performance
//!
//! Target: 400+ MB/s (vs Terminal's ~60 MB/s)
//!
//! Key techniques:
//! - Compile-time transition table
//! - Explicit SIMD intrinsics (AVX2/NEON) for escape scanning with scalar fallback
//! - Zero allocation during parse
//!
//! ## Complexity Proof (O(n))
//!
//! The parser processes input in O(n) time where n is the number of input bytes.
//! This is guaranteed by:
//!
//! 1. **Main loop**: `advance_fast`/`advance_batch` walk the input once.
//!    - Best case: SIMD `take_printable` skips runs of printable ASCII
//!    - Worst case: each byte goes through `process_byte_inner` (fast) or
//!      `process_byte_batch` (batch)
//!
//! 2. **Per-byte operations are O(1)**: `process_byte_inner`/`process_byte_batch` do:
//!    - Constant-time table lookup: `TRANSITIONS[state][byte]`
//!    - Bounded buffer operations:
//!      - `params`: ArrayVec<u16, 16> (MAX_PARAMS = 16)
//!      - `intermediates`: ArrayVec<u8, 4> (MAX_INTERMEDIATES = 4)
//!      - `osc_data`: capped at MAX_OSC_DATA = 65536, with O(1) push
//!
//! 3. **No nested loops**: Each byte triggers at most one state transition
//!    and one action dispatch. No action re-processes previous input.
//!
//! 4. **Bounds enforcement**: All buffer operations check capacity before
//!    insertion, preventing unbounded growth. See Kani proof `params_bounded`.
//!
//! The benchmarks (`cargo bench --package aterm-parser --bench parser`) verify
//! that throughput remains constant across input sizes (1KB, 64KB, 1MB).

#![deny(missing_docs, clippy::all, clippy::pedantic)]
#![cfg_attr(test, allow(clippy::all, clippy::pedantic))]
#![allow(
    unexpected_cfgs,
    reason = "required for #[cfg(kani)] formal verification"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "not all functions need #[must_use]"
)]
#![allow(
    clippy::module_name_repetitions,
    reason = "e.g. parser::Parser is idiomatic"
)]
#![allow(
    clippy::similar_names,
    reason = "fg/bg, row/col are domain-standard pairs"
)]
#![allow(clippy::wildcard_imports, reason = "use prelude::* is idiomatic")]
#![allow(
    clippy::match_same_arms,
    reason = "explicit match arms aid readability"
)]
#![allow(
    clippy::match_bool,
    reason = "match on bool can be clearer than if/else"
)]
#![allow(clippy::single_match_else, reason = "match for clarity")]
#![allow(
    clippy::missing_panics_doc,
    reason = "documentation coverage is incremental"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation coverage is incremental"
)]
#![allow(
    clippy::doc_markdown,
    reason = "technical terms like VT100 don't need backticks"
)]

mod action;
mod csi;
mod dispatch;
mod invariants;
mod simd;
mod simd_csi;
mod state;
/// State transition table.
pub mod table;
mod utf8;

pub use action::{Action, ActionSink, BatchActionSink, NullSink};
pub use state::State;
pub use table::{ActionType, TRANSITIONS};

use aterm_alloc::ArrayVec;

// ----------------------------------------------------------------------------
// Test instrumentation: Deterministic operation counters
// ----------------------------------------------------------------------------
// These counters replace wall-clock timing for complexity assertions in tests.
// See #1572 for the rationale behind deterministic counters.
//
// NOTE: Use full path std::cell::Cell for consistency with grid module pattern.
// Parser doesn't have a Cell type conflict, but we maintain the pattern org-wide.

// Counter for main loop iterations in advance/advance_fast (O(n) verification).
#[cfg(test)]
thread_local! {
    static PARSER_LOOP_ITERATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Increment the parser loop iteration counter.
#[cfg(test)]
fn count_parser_loop_iteration() {
    PARSER_LOOP_ITERATIONS.with(|c| c.set(c.get() + 1));
}

/// Take (read and reset) the parser loop iteration count.
#[cfg(test)]
fn take_parser_loop_iterations() -> usize {
    PARSER_LOOP_ITERATIONS.with(|c| {
        let v = c.get();
        c.set(0);
        v
    })
}

/// Maximum number of CSI parameters
pub const MAX_PARAMS: usize = 16;

/// Maximum number of intermediate bytes
pub const MAX_INTERMEDIATES: usize = 4;

/// Maximum OSC data size (64KB)
pub(crate) const MAX_OSC_DATA: usize = 65536;

/// Maximum number of OSC parameters (semicolon-separated segments).
/// Matches CSI `MAX_PARAMS`. OSC sequences with semicolons in payload
/// bodies (OSC 66, OSC 777, OSC 1337) were silently truncated at 8 (#7268).
pub(crate) const MAX_OSC_PARAMS: usize = 16;

/// VT parser state machine.
///
/// ## Example
///
/// ```
/// use aterm_parser::{ActionSink, Parser};
/// use aterm_provenance::{Provenance, Pty};
///
/// struct PrintSink;
/// impl ActionSink for PrintSink {
///     fn print(&mut self, c: char) { print!("{}", c); }
///     fn execute(&mut self, _byte: u8) {}
///     fn csi_dispatch(
///         &mut self,
///         _params: &Provenance<[u16], Pty>,
///         _intermediates: &Provenance<[u8], Pty>,
///         _final_byte: u8,
///     ) {}
///     fn esc_dispatch(&mut self, _intermediates: &Provenance<[u8], Pty>, _final_byte: u8) {}
///     fn osc_dispatch(&mut self, _params: &Provenance<[&[u8]], Pty>) {}
///     fn dcs_hook(
///         &mut self,
///         _params: &Provenance<[u16], Pty>,
///         _intermediates: &Provenance<[u8], Pty>,
///         _final_byte: u8,
///     ) {}
///     fn dcs_put(&mut self, _byte: u8) {}
///     fn dcs_unhook(&mut self) {}
///     fn apc_start(&mut self) {}
///     fn apc_put(&mut self, _byte: u8) {}
///     fn apc_end(&mut self) {}
/// }
///
/// let mut parser = Parser::new();
/// let mut sink = PrintSink;
/// parser.advance(b"Hello, World!", &mut sink);
/// ```
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "parser state machine has many boolean flags"
)]
pub struct Parser {
    pub(crate) state: State,
    pub(crate) params: ArrayVec<u16, MAX_PARAMS>,
    pub(crate) intermediates: ArrayVec<u8, MAX_INTERMEDIATES>,
    pub(crate) osc_data: Vec<u8>,
    pub(crate) current_param: u32,
    pub(crate) param_started: bool,
    pub(crate) dcs_active: bool,
    /// Tracks whether we're in an APC sequence (vs SOS/PM)
    pub(crate) apc_active: bool,
    /// UTF-8 decoding buffer for multi-byte sequences
    pub(crate) utf8_buffer: [u8; 4],
    /// Number of bytes accumulated in utf8_buffer
    pub(crate) utf8_len: u8,
    /// Expected total bytes for current UTF-8 sequence
    pub(crate) utf8_expected: u8,
    /// Bitmask tracking which params were preceded by a colon (subparameter separator).
    /// Bit i is set if param\[i\] is a subparameter of param\[i-1\].
    /// Used for SGR 4:x underline style subparameters.
    pub(crate) subparam_mask: u16,
    /// Tracks if the last separator was a colon (for next param)
    pub(crate) last_was_colon: bool,
    /// Whether to interpret 8-bit C1 control codes (0x80-0x9F).
    ///
    /// When false (default), bytes 0x80-0x9F are treated as invalid UTF-8
    /// and replaced with the Unicode replacement character. This is the secure
    /// default for UTF-8 terminals, as C1 controls embedded in UTF-8 text
    /// can be used for escape sequence injection attacks.
    ///
    /// When true, bytes 0x80-0x9F are interpreted as 8-bit C1 control codes
    /// (the 8-bit equivalents of ESC+char sequences). Only enable this for
    /// legacy applications that require C1 support.
    ///
    /// See: dgl.cx/2023/09/ansi-terminal-security
    pub(crate) c1_controls_enabled: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    /// Create a new parser in the ground state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            params: ArrayVec::new_const(),
            intermediates: ArrayVec::new_const(),
            osc_data: Vec::with_capacity(128),
            current_param: 0,
            param_started: false,
            dcs_active: false,
            apc_active: false,
            utf8_buffer: [0; 4],
            utf8_len: 0,
            utf8_expected: 0,
            subparam_mask: 0,
            last_was_colon: false,
            c1_controls_enabled: false,
        }
    }

    /// Create a parser optimized for Kani verification.
    ///
    /// Avoids pre-allocation in `osc_data` to reduce CBMC state space.
    #[cfg(kani)]
    #[must_use]
    pub fn kani_stub() -> Self {
        Self {
            state: State::Ground,
            params: ArrayVec::new_const(),
            intermediates: ArrayVec::new_const(),
            osc_data: Vec::new(),
            current_param: 0,
            param_started: false,
            dcs_active: false,
            apc_active: false,
            utf8_buffer: [0; 4],
            utf8_len: 0,
            utf8_expected: 0,
            subparam_mask: 0,
            last_was_colon: false,
            c1_controls_enabled: false,
        }
    }

    /// Create a new parser with C1 control code interpretation enabled.
    ///
    /// This constructor enables 8-bit C1 control code interpretation (0x80-0x9F).
    /// Only use this for legacy applications that require C1 support. For modern
    /// UTF-8 terminals, use [`Parser::new()`] which disables C1 by default.
    #[must_use]
    pub fn with_c1_controls() -> Self {
        let mut parser = Self::new();
        parser.c1_controls_enabled = true;
        parser
    }

    /// Enable or disable 8-bit C1 control code interpretation.
    ///
    /// When disabled (default), bytes 0x80-0x9F are treated as invalid UTF-8.
    /// When enabled, they are interpreted as C1 control codes.
    pub fn set_c1_controls_enabled(&mut self, enabled: bool) {
        self.c1_controls_enabled = enabled;
    }

    /// Check if 8-bit C1 control code interpretation is enabled.
    #[must_use]
    pub fn c1_controls_enabled(&self) -> bool {
        self.c1_controls_enabled
    }

    /// Reset parser to ground state.
    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.params.clear();
        self.intermediates.clear();
        self.osc_data.clear();
        self.current_param = 0;
        self.param_started = false;
        self.dcs_active = false;
        self.apc_active = false;
        self.utf8_len = 0;
        self.utf8_expected = 0;
        self.subparam_mask = 0;
        self.last_was_colon = false;
    }

    /// Get current parser state.
    #[inline]
    pub fn state(&self) -> State {
        self.state
    }

    /// Get subparameter mask for the last CSI sequence.
    ///
    /// Bit `i` is set if `params[i]` was preceded by a colon (`:`) rather than
    /// a semicolon (`;`), indicating it's a subparameter.
    ///
    /// Example: `ESC[4:3m` → `params=[4,3]`, `subparam_mask=0b10` (bit 1 set)
    #[inline]
    pub fn subparam_mask(&self) -> u16 {
        self.subparam_mask
    }
}

// External test and proof modules
#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs;
