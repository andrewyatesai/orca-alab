// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! CSI (Control Sequence Introducer) fast-path parsing.

use aterm_provenance::pty_wrap_ref;

use crate::action::ActionSink;
use crate::state::State;
use crate::{MAX_INTERMEDIATES, MAX_PARAMS, Parser};

impl Parser {
    /// Finalize the current parameter and push it to the params list.
    ///
    /// Called on semicolons, colons, and at end-of-params in CSI fast-path parsing.
    /// Sets `subparam_mask` bit if `is_subparam` is true.
    #[inline]
    pub(crate) fn push_current_param(&mut self, is_subparam: bool) {
        let param_index = self.params.len();
        if param_index < MAX_PARAMS {
            let clamped = self.current_param.min(u32::from(u16::MAX));
            let value = u16::try_from(clamped).unwrap_or(u16::MAX);
            self.params.push(value);
            if is_subparam && param_index < 16 {
                self.subparam_mask |= 1 << param_index;
            }
        }
        self.current_param = 0;
        self.param_started = false;
    }

    /// Try to parse a CSI sequence using the fast path.
    ///
    /// Returns the number of bytes consumed if successful, None if we should
    /// fall back to normal byte-by-byte parsing.
    ///
    /// The fast path handles simple CSI sequences of the form:
    /// - CSI \[private\] params \[intermediate\] final
    /// - Params are digits and semicolons
    /// - Final byte is 0x40-0x7E
    #[inline]
    pub(crate) fn try_parse_csi_fast<S: ActionSink>(
        &mut self,
        input: &[u8],
        sink: &mut S,
    ) -> Option<usize> {
        // Ultra-fast path: zero-param CSI (e.g., ESC[A, ESC[H).
        // The first byte after ESC[ is already a final byte — skip all parsing.
        // Covers CUU/CUD/CUF/CUB/CUP(home)/ED/EL/SGR(reset) with no params.
        if let Some(&first) = input.first()
            && (0x40..=0x7E).contains(&first)
        {
            self.params.clear();
            self.intermediates.clear();
            self.subparam_mask = 0;
            sink.csi_dispatch(
                pty_wrap_ref(self.params.as_slice()),
                pty_wrap_ref(self.intermediates.as_slice()),
                first,
            );
            self.state = State::Ground;
            return Some(1);
        }

        // Fast path: 1-2 digit param CSI (e.g., ESC[5A, ESC[0m, ESC[32m).
        // Covers ANSI colors (30-37,39,40-47,49), attribute resets (21-29),
        // and common single-digit cursor ops. Skips position scan + loop.
        if input.len() >= 2 && input[0].is_ascii_digit() {
            let b1 = input[1];
            if (0x40..=0x7E).contains(&b1) {
                // Single digit: ESC[Nm
                self.params.set_single(u16::from(input[0] - b'0'));
                self.intermediates.clear();
                self.subparam_mask = 0;
                sink.csi_dispatch(
                    pty_wrap_ref(self.params.as_slice()),
                    pty_wrap_ref(self.intermediates.as_slice()),
                    b1,
                );
                self.state = State::Ground;
                return Some(2);
            }
            if b1 == b';' {
                // D;... — 1-digit first param, multi-param sequence.
                // Common: ESC[1;31m (bold+red), ESC[5;20r (scroll region).
                let p1 = u16::from(input[0] - b'0');
                return self.parse_csi_after_first_param(input, sink, p1, 2);
            }
            if input.len() >= 3 && b1.is_ascii_digit() {
                let b2 = input[2];
                if (0x40..=0x7E).contains(&b2) {
                    // Two digits: ESC[NNx
                    self.params
                        .set_single(u16::from(input[0] - b'0') * 10 + u16::from(b1 - b'0'));
                    self.intermediates.clear();
                    self.subparam_mask = 0;
                    sink.csi_dispatch(
                        pty_wrap_ref(self.params.as_slice()),
                        pty_wrap_ref(self.intermediates.as_slice()),
                        b2,
                    );
                    self.state = State::Ground;
                    return Some(3);
                }
                if b2 == b';' {
                    // DD;... — 2-digit first param, multi-param sequence.
                    // Common: ESC[38;5;Nm (256-color), ESC[12;40H (CUP).
                    let p1 = u16::from(input[0] - b'0') * 10 + u16::from(b1 - b'0');
                    return self.parse_csi_after_first_param(input, sink, p1, 3);
                }
            }
        }

        self.parse_csi_general(input, sink)
    }

    /// Dispatch a CSI sequence, choosing subparam vs. normal dispatch.
    #[inline]
    fn csi_dispatch_final<S: ActionSink>(&self, sink: &mut S, final_byte: u8) {
        if self.subparam_mask != 0 {
            sink.csi_dispatch_with_subparams(
                pty_wrap_ref(self.params.as_slice()),
                pty_wrap_ref(self.intermediates.as_slice()),
                final_byte,
                self.subparam_mask,
            );
        } else {
            sink.csi_dispatch(
                pty_wrap_ref(self.params.as_slice()),
                pty_wrap_ref(self.intermediates.as_slice()),
                final_byte,
            );
        }
    }

    /// Multi-param CSI fast path: first param already parsed, continue from `pos`.
    ///
    /// Called when `try_parse_csi_fast` detects `D;` or `DD;` at the start of a
    /// CSI sequence. Avoids `parse_csi_general` overhead (clear, private-marker
    /// check, re-parsing first param digits) for common multi-param patterns:
    /// `38;5;Nm` (256-color), `R;CH` (CUP), `1;31m` (bold+red SGR).
    ///
    /// Separated from the fast path (`#[inline(never)]`) to keep the L1i-hot
    /// zero/single/two-digit dispatch compact.
    #[inline(never)]
    fn parse_csi_after_first_param<S: ActionSink>(
        &mut self,
        input: &[u8],
        sink: &mut S,
        first_param: u16,
        mut pos: usize,
    ) -> Option<usize> {
        self.params.clear();
        self.params.push(first_param);
        self.intermediates.clear();
        self.subparam_mask = 0;
        self.current_param = 0;
        self.param_started = false;

        let limit = input.len().min(65);

        // Parse remaining params (no private marker — first byte was a digit)
        while pos < limit {
            let b = input[pos];
            if b.is_ascii_digit() {
                self.current_param = self
                    .current_param
                    .saturating_mul(10)
                    .saturating_add(u32::from(b - b'0'));
                self.param_started = true;
                pos += 1;
            } else if b == b';' {
                self.push_current_param(false);
                pos += 1;
            } else if b == b':' {
                self.push_current_param(false);
                // Parse the full colon-separated subparam group. Each `:` introduces
                // a subparam value that must be flagged in subparam_mask. We loop
                // here instead of handling only one value, so `58:5:196` in the
                // middle of a mixed sequence like `1;58:5:196m` is fully consumed
                // without falling through to parse_csi_general_from mid-group.
                loop {
                    let param_index = self.params.len();
                    self.current_param = 0;
                    self.param_started = false;
                    pos += 1; // consume the ':'
                    // Parse the subparam value digits
                    while pos < limit && input[pos].is_ascii_digit() {
                        self.current_param = self
                            .current_param
                            .saturating_mul(10)
                            .saturating_add(u32::from(input[pos] - b'0'));
                        self.param_started = true;
                        pos += 1;
                    }
                    // Push with subparam flag
                    if self.param_started {
                        self.push_current_param(true);
                    } else if param_index < MAX_PARAMS {
                        self.params.push(0);
                        if param_index < 16 {
                            self.subparam_mask |= 1 << param_index;
                        }
                    }
                    // If the next byte is another ':', continue the colon group.
                    if pos < limit && input[pos] == b':' {
                        continue;
                    }
                    break;
                }
                // Colon-subparam sequences are rare; fall through to general
                // for any remaining complexity.
                // If the next byte is `;`, consume it here so parse_csi_general_from
                // doesn't push a phantom zero param. The subparam value was already
                // pushed above, so the `;` is just a group separator (#7648).
                if pos < limit && input[pos] == b';' {
                    pos += 1;
                }
                return self.parse_csi_general_from(input, sink, pos);
            } else if (0x40..=0x7E).contains(&b) {
                if self.param_started {
                    self.push_current_param(false);
                }
                self.csi_dispatch_final(sink, b);
                self.state = State::Ground;
                return Some(pos + 1);
            } else if (0x20..=0x2F).contains(&b) {
                if self.param_started {
                    self.push_current_param(false);
                }
                return self.parse_csi_intermediates(input, sink, pos, limit);
            } else {
                return None;
            }
        }

        None
    }

    /// Continue general CSI parsing from a given position.
    ///
    /// Used when `parse_csi_after_first_param` encounters a colon subparam
    /// and needs to fall back to the general-purpose loop. Params already
    /// parsed up to `pos` are preserved.
    #[inline(never)]
    fn parse_csi_general_from<S: ActionSink>(
        &mut self,
        input: &[u8],
        sink: &mut S,
        mut pos: usize,
    ) -> Option<usize> {
        let limit = input.len().min(65);
        let mut next_is_subparam = false;

        while pos < limit {
            let b = input[pos];
            if b.is_ascii_digit() {
                self.current_param = self
                    .current_param
                    .saturating_mul(10)
                    .saturating_add(u32::from(b - b'0'));
                self.param_started = true;
                pos += 1;
            } else if b == b';' {
                self.push_current_param(next_is_subparam);
                next_is_subparam = false;
                pos += 1;
            } else if b == b':' {
                self.push_current_param(next_is_subparam);
                next_is_subparam = true;
                pos += 1;
            } else if (0x40..=0x7E).contains(&b) {
                if self.param_started {
                    self.push_current_param(next_is_subparam);
                }
                self.csi_dispatch_final(sink, b);
                self.state = State::Ground;
                return Some(pos + 1);
            } else if (0x20..=0x2F).contains(&b) {
                if self.param_started {
                    self.push_current_param(next_is_subparam);
                }
                return self.parse_csi_intermediates(input, sink, pos, limit);
            } else {
                return None;
            }
        }

        None
    }

    /// General-path CSI parser: single-pass scan that parses params and finds
    /// the final byte simultaneously. Handles private markers, subparams,
    /// and intermediate bytes.
    ///
    /// For non-private-marker sequences, attempts SIMD-accelerated parameter
    /// parsing first (see `simd::simd_parse_csi_params`), falling back to
    /// byte-by-byte when subparams or private markers are present.
    #[inline]
    fn parse_csi_general<S: ActionSink>(&mut self, input: &[u8], sink: &mut S) -> Option<usize> {
        self.params.clear();
        self.subparam_mask = 0;
        self.current_param = 0;
        self.param_started = false;
        self.intermediates.clear();

        let mut pos = 0;
        let limit = input.len().min(65); // Cap scan to 65 bytes (64 params + final)

        // Check for private marker (? > < = etc.)
        if pos < limit && (0x3C..=0x3F).contains(&input[pos]) {
            self.intermediates.push(input[pos]);
            pos += 1;
        }

        // SIMD fast path: when no private marker was consumed (pos == 0),
        // try bulk parameter parsing. This accelerates the common case of
        // multi-param CSI sequences like `38;2;255;128;0m`.
        if pos == 0
            && let Some(result) = crate::simd_csi::simd_parse_csi_params(input)
            && !result.has_subparams
        {
            // No subparams — we can use the SIMD result directly.
            let consumed = result.bytes_consumed;
            if consumed < limit {
                let b = input[consumed];
                if (0x40..=0x7E).contains(&b) {
                    // Final byte — dispatch with SIMD-parsed params
                    for i in 0..result.count.min(MAX_PARAMS) {
                        self.params.push(result.params[i]);
                    }
                    sink.csi_dispatch(
                        pty_wrap_ref(self.params.as_slice()),
                        pty_wrap_ref(self.intermediates.as_slice()),
                        b,
                    );
                    self.state = State::Ground;
                    return Some(consumed + 1);
                }
                if (0x20..=0x2F).contains(&b) {
                    // Intermediate byte — load params and delegate
                    for i in 0..result.count.min(MAX_PARAMS) {
                        self.params.push(result.params[i]);
                    }
                    return self.parse_csi_intermediates(input, sink, consumed, limit);
                }
                // Invalid byte (< 0x20 or 0x7F) — reject
                return None;
            }
            // Consumed up to the limit with no final byte — reject
            return None;
        }
        // Note: when has_subparams is true, or no SIMD result, or private
        // marker present (pos != 0), falls through to byte-by-byte below.

        // Byte-by-byte fallback (private markers, subparams, or SIMD unavailable)
        let mut next_is_subparam = false;

        // Single pass: parse params and find final byte simultaneously
        while pos < limit {
            let b = input[pos];
            if b.is_ascii_digit() {
                self.current_param = self
                    .current_param
                    .saturating_mul(10)
                    .saturating_add(u32::from(b - b'0'));
                self.param_started = true;
                pos += 1;
            } else if b == b';' {
                self.push_current_param(next_is_subparam);
                next_is_subparam = false;
                pos += 1;
            } else if b == b':' {
                self.push_current_param(next_is_subparam);
                next_is_subparam = true;
                pos += 1;
            } else if (0x40..=0x7E).contains(&b) {
                if self.param_started {
                    self.push_current_param(next_is_subparam);
                }
                self.csi_dispatch_final(sink, b);
                self.state = State::Ground;
                return Some(pos + 1);
            } else if (0x20..=0x2F).contains(&b) {
                if self.param_started {
                    self.push_current_param(next_is_subparam);
                }
                return self.parse_csi_intermediates(input, sink, pos, limit);
            } else {
                return None;
            }
        }

        None
    }

    /// Parse intermediate bytes (0x20-0x2F) and the final byte that follows.
    #[inline]
    fn parse_csi_intermediates<S: ActionSink>(
        &mut self,
        input: &[u8],
        sink: &mut S,
        mut pos: usize,
        limit: usize,
    ) -> Option<usize> {
        while pos < limit {
            let ib = input[pos];
            if (0x20..=0x2F).contains(&ib) {
                if self.intermediates.len() < MAX_INTERMEDIATES {
                    self.intermediates.push(ib);
                }
                pos += 1;
            } else if (0x40..=0x7E).contains(&ib) {
                self.csi_dispatch_final(sink, ib);
                self.state = State::Ground;
                return Some(pos + 1);
            } else {
                return None;
            }
        }
        None
    }
}
