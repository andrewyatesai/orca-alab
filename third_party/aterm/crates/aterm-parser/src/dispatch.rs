// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
//
// Author: The aterm Authors

//! Parser dispatch engine: advance methods, state-machine byte processing, helpers.

use aterm_alloc::ArrayVec;
use aterm_provenance::pty_wrap_ref;

use super::table::{ActionType, TRANSITIONS};
use super::{ActionSink, BatchActionSink, MAX_OSC_DATA, MAX_OSC_PARAMS, Parser, State};

#[cfg(test)]
use super::count_parser_loop_iteration;

use super::MAX_INTERMEDIATES;

/// Shared byte-processing logic for both `ActionSink` and `BatchActionSink` paths.
///
/// Eliminates ~115 lines of duplicated state-machine dispatch code while
/// preserving full monomorphization for each sink type.
macro_rules! process_byte_impl {
    ($self:expr, $byte:expr, $sink:expr) => {{
        // Block C1 control bytes (0x80-0x9F) in non-Ground, non-string states
        // when C1 controls are disabled. The static transition table contains
        // "anywhere" entries that route 0x90→DCS, 0x9B→CSI, 0x9D→OSC, etc.
        // Without this guard, a malicious byte stream can inject escape sequences
        // via C1 introducers even when the parser is mid-sequence (#7556).
        //
        // Excluded states:
        //  - Ground: handled upstream (process_byte / process_ground_special_byte)
        //  - DcsPassthrough / OscString / SosPmApcString: their custom table
        //    overrides already map 0x80-0x9F to data actions (DcsPut / OscPut),
        //    so C1 bytes are harmless payload in those states.
        if !$self.c1_controls_enabled
            && (0x80..=0x9F).contains(&$byte)
            && !matches!(
                $self.state,
                State::Ground | State::DcsPassthrough | State::OscString | State::SosPmApcString
            )
        {
            // Silently drop — consistent with Ground state behavior.
        } else if $self.c1_controls_enabled && $byte == 0x9C && $self.state == State::OscString {
            // Runtime C1 ST override: when C1 controls are enabled and byte 0x9C
            // arrives in OscString state, treat it as ST (string terminator) instead
            // of data. The static table maps 0x9C -> OscPut for UTF-8 safety (0x9C
            // is a valid continuation byte in CJK like 本=E6 9C AC), but when C1
            // controls are explicitly enabled, 0x9C must terminate the OSC sequence
            // per the DEC spec.
            $self.dispatch_osc($sink, false);
            $self.state = State::Ground;
        } else {
            let transition = TRANSITIONS[$self.state as usize][$byte as usize];
            let prev_state = $self.state;

            // Handle DCS unhook when leaving DcsPassthrough
            if prev_state == State::DcsPassthrough && transition.next_state != State::DcsPassthrough
            {
                if $self.dcs_active {
                    $sink.dcs_unhook();
                    $self.dcs_active = false;
                }
            }

            // Handle OSC end when leaving OscString
            if prev_state == State::OscString
                && transition.next_state != State::OscString
                && transition.action != ActionType::OscEnd
            {
                $self.dispatch_osc($sink, false);
            }

            // Handle APC end when leaving SosPmApcString
            if prev_state == State::SosPmApcString
                && transition.next_state != State::SosPmApcString
                && transition.action != ActionType::ApcEnd
            {
                if $self.apc_active {
                    $sink.apc_end();
                    $self.apc_active = false;
                }
            }

            // Execute the action
            match transition.action {
                ActionType::None | ActionType::Ignore => {}
                ActionType::Print => {
                    $sink.print($byte as char);
                }
                ActionType::Execute => {
                    $sink.execute($byte);
                }
                ActionType::Clear => {
                    $self.clear();
                    $self.osc_data.clear();
                }
                ActionType::Collect => {
                    $self.collect($byte);
                }
                ActionType::Param => {
                    $self.add_param_digit($byte);
                }
                ActionType::EscDispatch => {
                    $sink.esc_dispatch(pty_wrap_ref($self.intermediates.as_slice()), $byte);
                }
                ActionType::CsiDispatch => {
                    if $self.param_started {
                        $self.finalize_param();
                    }
                    if $self.subparam_mask != 0 {
                        $sink.csi_dispatch_with_subparams(
                            pty_wrap_ref($self.params.as_slice()),
                            pty_wrap_ref($self.intermediates.as_slice()),
                            $byte,
                            $self.subparam_mask,
                        );
                    } else {
                        $sink.csi_dispatch(
                            pty_wrap_ref($self.params.as_slice()),
                            pty_wrap_ref($self.intermediates.as_slice()),
                            $byte,
                        );
                    }
                }
                ActionType::DcsHook => {
                    if $self.param_started {
                        $self.finalize_param();
                    }
                    $sink.dcs_hook(
                        pty_wrap_ref($self.params.as_slice()),
                        pty_wrap_ref($self.intermediates.as_slice()),
                        $byte,
                    );
                    $self.dcs_active = true;
                }
                ActionType::DcsPut => {
                    $sink.dcs_put($byte);
                }
                ActionType::OscStart => {
                    $self.osc_data.clear();
                }
                ActionType::OscPut => {
                    if $self.osc_data.len() < MAX_OSC_DATA {
                        $self.osc_data.push($byte);
                    }
                }
                ActionType::OscEnd => {
                    $self.dispatch_osc($sink, true);
                }
                ActionType::ApcStart => {
                    $sink.apc_start();
                    $self.apc_active = true;
                }
                ActionType::ApcPut => {
                    if $self.apc_active {
                        $sink.apc_put($byte);
                    }
                }
                ActionType::ApcEnd => {
                    if $self.apc_active {
                        $sink.apc_end();
                        $self.apc_active = false;
                    }
                }
            }

            $self.state = transition.next_state;
        } // end else (C1 ST override)
    }};
}

impl Parser {
    /// Process input bytes, calling sink for each action.
    ///
    /// # Safety
    ///
    /// This function:
    /// - Never panics for any input
    /// - Never accesses out-of-bounds memory
    /// - Always terminates
    pub fn advance<S: ActionSink>(&mut self, input: &[u8], sink: &mut S) {
        for &byte in input {
            // Test instrumentation: count iterations for O(n) verification
            #[cfg(test)]
            count_parser_loop_iteration();

            self.process_byte(byte, sink);
        }
    }

    /// Process input with fast path for ground state.
    ///
    /// This is an optimization that uses SIMD scanning for printable text.
    /// On typical terminal output (mostly printable text), this is 5-10x
    /// faster than the basic `advance` method.
    ///
    /// Handles UTF-8 multi-byte sequences properly for non-ASCII characters.
    pub fn advance_fast<S: ActionSink>(&mut self, input: &[u8], sink: &mut S) {
        self.advance_simd_loop::<S, _, _, true, false, true>(
            input,
            sink,
            |sink, data| sink.print_ascii_bulk(pty_wrap_ref(data)),
            Self::process_byte_inner,
        );
    }

    /// Process a single byte through the state machine (inner implementation).
    #[inline]
    pub(crate) fn process_byte_inner<S: ActionSink>(&mut self, byte: u8, sink: &mut S) {
        process_byte_impl!(self, byte, sink);
    }

    /// Process input with batch printing optimization.
    ///
    /// Like `advance_fast`, but passes entire printable slices to a
    /// specialized `print_str` method for even better performance.
    pub fn advance_batch<S: BatchActionSink>(&mut self, input: &[u8], sink: &mut S) {
        self.advance_simd_loop::<S, _, _, false, true, false>(
            input,
            sink,
            |sink, printable| {
                // `take_printable` (via `find_non_printable`) returns only bytes
                // in 0x20-0x7E — all valid single-byte UTF-8 codepoints.
                // Kani proof `printable_slice_is_valid_utf8` verifies this.
                // SAFETY: take_printable returns only bytes 0x20-0x7E, all valid
                // single-byte UTF-8. Kani proof `printable_slice_is_valid_utf8`
                // verifies this invariant (#7866).
                let s = unsafe { std::str::from_utf8_unchecked(printable) };
                sink.print_str(pty_wrap_ref(s));
            },
            Self::process_byte_batch,
        );
    }

    #[inline]
    fn process_ground_special_byte<S, ProcessByte>(
        &mut self,
        byte: u8,
        sink: &mut S,
        process_byte: &mut ProcessByte,
    ) where
        S: ActionSink,
        ProcessByte: FnMut(&mut Self, u8, &mut S),
    {
        if (0xC0..=0xF7).contains(&byte) {
            self.start_utf8(byte);
            return;
        }

        if (0x80..=0x9F).contains(&byte) {
            if self.c1_controls_enabled {
                process_byte(self, byte, sink);
            } else {
                sink.print(char::REPLACEMENT_CHARACTER);
            }
            return;
        }

        if (0xA0..=0xBF).contains(&byte) || byte >= 0xF8 {
            sink.print(char::REPLACEMENT_CHARACTER);
            return;
        }

        process_byte(self, byte, sink);
    }

    #[allow(clippy::too_many_lines)]
    fn advance_simd_loop<
        S,
        EmitPrintable,
        ProcessByte,
        const COUNT_LOOPS: bool,
        const REPLAY_ESCAPE_BRACKET_ON_FAIL: bool,
        const SET_GROUND_AFTER_ESCAPE_FAST_PATH: bool,
    >(
        &mut self,
        input: &[u8],
        sink: &mut S,
        mut emit_printable: EmitPrintable,
        mut process_byte: ProcessByte,
    ) where
        S: ActionSink,
        EmitPrintable: FnMut(&mut S, &[u8]),
        ProcessByte: FnMut(&mut Self, u8, &mut S),
    {
        let mut remaining = input;

        while !remaining.is_empty() {
            if COUNT_LOOPS {
                // Test instrumentation: count loop iterations for O(n) verification
                #[cfg(test)]
                count_parser_loop_iteration();
            }

            if self.utf8_len > 0 && self.state == State::Ground {
                let byte = remaining[0];
                remaining = &remaining[1..];
                self.process_utf8_byte(byte, sink);
                continue;
            }

            if self.state == State::Ground {
                let (printable, rest) = super::simd::take_printable(remaining);
                if !printable.is_empty() {
                    emit_printable(sink, printable);
                }

                remaining = rest;
                if remaining.is_empty() {
                    break;
                }

                let byte = remaining[0];
                remaining = &remaining[1..];

                if byte == 0x1B && remaining.first() == Some(&b'[') {
                    remaining = &remaining[1..];
                    if let Some(consumed) = self.try_parse_csi_fast(remaining, sink) {
                        remaining = &remaining[consumed..];
                        continue;
                    }
                    self.state = State::Escape;
                    self.clear();
                    process_byte(self, b'[', sink);
                    continue;
                }

                // C0 control fast path: LF/CR/BS/ESC are the most common
                // non-printable bytes. Route them directly to the state machine
                // without the 3 redundant range checks in process_ground_special_byte.
                if byte < 0x20 {
                    process_byte(self, byte, sink);
                    continue;
                }

                // UTF-8 fast path: decode multi-byte sequences when all
                // continuation bytes are available, batching consecutive
                // non-ASCII characters for amortized dispatch overhead.
                // The decode logic is in a separate function to keep the
                // hot ASCII dispatch loop compact for L1i cache.
                if (0xC0..=0xF7).contains(&byte) {
                    let consumed = self.decode_multibyte_run(byte, remaining, sink);
                    remaining = &remaining[consumed..];
                    continue;
                }

                self.process_ground_special_byte(byte, sink, &mut process_byte);
                continue;
            }

            if self.state == State::Escape && remaining.first() == Some(&b'[') {
                let rest = &remaining[1..];
                if let Some(consumed) = self.try_parse_csi_fast(rest, sink) {
                    remaining = &rest[consumed..];
                    if SET_GROUND_AFTER_ESCAPE_FAST_PATH {
                        self.state = State::Ground;
                    }
                    continue;
                }

                if REPLAY_ESCAPE_BRACKET_ON_FAIL {
                    remaining = rest;
                    self.state = State::Escape;
                    self.clear();
                    process_byte(self, b'[', sink);
                    continue;
                }
            }

            // OSC bulk fast path (#7864): bytes 0x20-0xFF are all OscPut
            // in the OscString state. Scan for the first C0 control byte
            // and bulk-append everything before it.
            if self.state == State::OscString {
                let n = if self.c1_controls_enabled {
                    remaining
                        .iter()
                        .position(|&b| b < 0x20 || b == 0x9C)
                        .unwrap_or(remaining.len())
                } else {
                    super::simd::find_c0_control(remaining).unwrap_or(remaining.len())
                };
                if n > 0 {
                    let capacity_left = MAX_OSC_DATA.saturating_sub(self.osc_data.len());
                    let copy_len = n.min(capacity_left);
                    self.osc_data.extend_from_slice(&remaining[..copy_len]);
                    remaining = &remaining[n..];
                    if remaining.is_empty() {
                        break;
                    }
                }
                // Fall through to process_byte for the C0 control byte.
            }

            // DCS bulk fast path (#7864): bytes 0x00-0x17 (minus 0x18/0x1A/0x1B),
            // 0x19, 0x1C-0x7E, and 0x80-0x9B, 0x9D-0xFF are all DcsPut.
            // Only 0x18 (CAN), 0x1A (SUB), 0x1B (ESC), and 0x9C (ST)
            // exit DcsPassthrough. Scan for these terminators and bulk-dispatch.
            if self.state == State::DcsPassthrough && self.dcs_active {
                let n = Self::find_dcs_terminator(remaining);
                if n > 0 {
                    sink.dcs_put_bulk(pty_wrap_ref(&remaining[..n]));
                    remaining = &remaining[n..];
                    if remaining.is_empty() {
                        break;
                    }
                }
                // Fall through to process_byte for the terminator.
            }

            let byte = remaining[0];
            remaining = &remaining[1..];
            process_byte(self, byte, sink);
        }
    }

    /// Process a single byte for BatchActionSink.
    fn process_byte_batch<S: BatchActionSink>(&mut self, byte: u8, sink: &mut S) {
        process_byte_impl!(self, byte, sink);
    }

    /// Find the first DCS terminator byte in input.
    ///
    /// DCS terminators: 0x18 (CAN), 0x1A (SUB), 0x1B (ESC), 0x9C (ST).
    ///
    /// Unlike OSC, DCS treats 0x9C as ST even when C1 controls are
    /// otherwise disabled: the transition table leaves 0x9C as a DCS
    /// terminator, so the fast path must scan for it too.
    #[inline]
    fn find_dcs_terminator(input: &[u8]) -> usize {
        input
            .iter()
            .position(|&b| b == 0x18 || b == 0x1A || b == 0x1B || b == 0x9C)
            .unwrap_or(input.len())
    }

    /// Clear parameters and intermediates (on entry to escape sequences).
    #[inline]
    fn clear(&mut self) {
        self.params.clear();
        self.intermediates.clear();
        self.current_param = 0;
        self.param_started = false;
        self.subparam_mask = 0;
        self.last_was_colon = false;
    }

    /// Add a digit to the current parameter, or handle separator (`;` or `:`).
    #[inline]
    pub(crate) fn add_param_digit(&mut self, byte: u8) {
        if byte.is_ascii_digit() {
            self.current_param = self
                .current_param
                .saturating_mul(10)
                .saturating_add(u32::from(byte - b'0'));
            self.param_started = true;
        } else if byte == b';' {
            // Semicolon: finalize current param and start new one
            self.finalize_param();
            self.last_was_colon = false;
        } else if byte == b':' {
            // Colon: finalize current param, mark next as subparameter
            self.finalize_param();
            self.last_was_colon = true;
        }
    }

    /// Finalize the current parameter.
    ///
    /// Delegates to `push_current_param` using `last_was_colon` as the
    /// subparam flag (byte-by-byte path tracks colon state in a field,
    /// while the CSI fast-path passes it explicitly).
    #[inline]
    pub(crate) fn finalize_param(&mut self) {
        self.push_current_param(self.last_was_colon);
    }

    /// Collect an intermediate byte.
    #[inline]
    fn collect(&mut self, byte: u8) {
        if self.intermediates.len() < MAX_INTERMEDIATES {
            self.intermediates.push(byte);
        }
    }

    /// Process a single byte through the state machine (basic method).
    ///
    /// Note: This is the simple byte-by-byte method. For better UTF-8 support,
    /// use `advance_fast` instead which properly handles multi-byte sequences.
    #[inline]
    fn process_byte<S: ActionSink>(&mut self, byte: u8, sink: &mut S) {
        if byte >= 0x80 && self.state == State::Ground {
            // C1 control codes (0x80-0x9F) security check
            // When c1_controls_enabled is false (default), treat C1 bytes as invalid
            // UTF-8 and emit replacement character instead of processing as controls.
            // This prevents escape sequence injection attacks in UTF-8 terminals.
            if (0x80..=0x9F).contains(&byte) {
                if self.c1_controls_enabled {
                    self.process_byte_inner(byte, sink);
                } else {
                    sink.print(char::REPLACEMENT_CHARACTER);
                }
                return;
            }
            // Latin-1 range (0xA0-0xFF): These bytes are valid printable Latin-1
            // characters. The transition table has no entries for them in Ground state
            // (they'd be silently dropped). Print them as their Unicode equivalents
            // (Latin-1 maps 1:1 to Unicode codepoints U+00A0-U+00FF).
            // SAFETY: 0xA0-0xFF are valid Unicode scalar values.
            sink.print(byte as char);
            return;
        }
        self.process_byte_inner(byte, sink);
    }

    /// Parse and dispatch OSC data.
    ///
    /// `bel_terminated` indicates whether the OSC was terminated by BEL (0x07)
    /// vs ST (ESC \\ or C1 0x9C). Passed through to
    /// [`ActionSink::osc_dispatch_with_terminator`] so response-generating
    /// handlers (e.g., OSC 52 clipboard query) can echo the same terminator.
    ///
    /// Performance: fast path for common 2-param OSC sequences (title set,
    /// CWD, shell integration marks) avoids the ArrayVec construction and
    /// full-buffer `;` scan. The format is `<cmd>;<payload>` with no further
    /// semicolons — covers OSC 0/1/2/7/9 which are the highest-frequency
    /// sequences in typical shell output. All other sequences fall through
    /// to the general ArrayVec split path. (#7355)
    fn dispatch_osc<S: ActionSink>(&mut self, sink: &mut S, bel_terminated: bool) {
        // Scoped so params (borrowing osc_data) is dropped before clear.
        {
            let data = &self.osc_data;

            // Fast path: single-digit command + ';' + payload with no
            // further semicolons. This is the overwhelmingly common case
            // for OSC 0/1/2/7/9 (titles, CWD, notifications). Avoids
            // ArrayVec init (256 bytes zeroed) and full-buffer split scan.
            if data.len() >= 2 && data[0].is_ascii_digit() && data[1] == b';' {
                // Check for a second semicolon only in the payload region.
                if data[2..].contains(&b';') {
                    self.dispatch_osc_general(sink, bel_terminated);
                } else {
                    let params: [&[u8]; 2] = [&data[..1], &data[2..]];
                    sink.osc_dispatch_with_terminator(pty_wrap_ref(&params[..]), bel_terminated);
                }
            } else {
                self.dispatch_osc_general(sink, bel_terminated);
            }
        }
        self.osc_data.clear();
        // Shrink the buffer if a large OSC payload inflated it beyond 4 KiB.
        // Without this, a single OSC 1337 image permanently holds up to 64 KiB
        // per parser instance for the session lifetime (#7272).
        if self.osc_data.capacity() > 4096 {
            self.osc_data.shrink_to(128);
        }
    }

    /// General OSC dispatch using ArrayVec split. Called for OSC sequences
    /// that don't match the 2-param fast path (multi-digit commands, multiple
    /// semicolons, etc.).
    #[inline(never)]
    fn dispatch_osc_general<S: ActionSink>(&mut self, sink: &mut S, bel_terminated: bool) {
        let mut params: ArrayVec<&[u8], MAX_OSC_PARAMS> = ArrayVec::new();
        for segment in self.osc_data.split(|&b| b == b';') {
            if params.is_full() {
                break;
            }
            params.push(segment);
        }
        sink.osc_dispatch_with_terminator(pty_wrap_ref(params.as_slice()), bel_terminated);
    }

    /// Decode and dispatch a run of multi-byte UTF-8 characters.
    ///
    /// Called when a UTF-8 lead byte (0xC0..=0xF7) is encountered in the
    /// ground-state fast path. Decodes consecutive multi-byte characters
    /// into a buffer and dispatches via `print`/`print_unicode_bulk`.
    ///
    /// Separated from the main dispatch loop (`#[inline(never)]`) to keep
    /// the hot ASCII path compact for L1 instruction cache. The function
    /// call overhead (~2 cycles) is negligible compared to the per-character
    /// decode cost, and UTF-8 multi-byte workloads keep this function warm
    /// in L1i anyway.
    ///
    /// Returns the number of bytes consumed from `remaining` (the slice
    /// after the lead byte).
    #[inline(never)]
    fn decode_multibyte_run<S: ActionSink>(
        &mut self,
        first_lead: u8,
        remaining: &[u8],
        sink: &mut S,
    ) -> usize {
        let mut char_buf = ['\0'; 256];
        let mut count: usize = 0;
        let mut lead = first_lead;
        let orig_len = remaining.len();
        let mut rem = remaining;

        loop {
            // Each branch produces Option<(char, bytes_consumed_from_rem)>.
            let decoded: Option<(char, usize)> = if lead >= 0xF0 {
                // 4-byte: SMP characters (emoji, math symbols)
                if rem.len() >= 3
                    && (rem[0] & 0xC0) == 0x80
                    && (rem[1] & 0xC0) == 0x80
                    && (rem[2] & 0xC0) == 0x80
                {
                    let cp = (u32::from(lead & 0x07) << 18)
                        | (u32::from(rem[0] & 0x3F) << 12)
                        | (u32::from(rem[1] & 0x3F) << 6)
                        | u32::from(rem[2] & 0x3F);
                    // cp must be in 0x10000..=0x10FFFF to be a valid Unicode
                    // scalar value. The lower bound excludes surrogates and
                    // overlongs. The upper bound rejects codepoints above the
                    // Unicode maximum — e.g., 0xF4 0x90 0x80 0x80 decodes to
                    // U+110000 which is not a valid char. (#7159)
                    if (0x10000..=0x0010_FFFF).contains(&cp) {
                        // SAFETY: cp is in 0x10000..=0x10FFFF — a valid
                        // Unicode scalar value (no surrogates, no overlongs,
                        // within Unicode range).
                        Some((unsafe { char::from_u32_unchecked(cp) }, 3))
                    } else {
                        None // Overlong encoding
                    }
                } else {
                    None
                }
            } else if lead >= 0xE0 {
                // 3-byte: BMP non-ASCII (CJK, Hangul, Greek, Cyrillic, etc.)
                if rem.len() >= 2 && (rem[0] & 0xC0) == 0x80 && (rem[1] & 0xC0) == 0x80 {
                    let cp = (u32::from(lead & 0x0F) << 12)
                        | (u32::from(rem[0] & 0x3F) << 6)
                        | u32::from(rem[1] & 0x3F);
                    if cp >= 0x800 && !(0xD800..=0xDFFF).contains(&cp) {
                        // SAFETY: cp is a valid Unicode scalar value:
                        // >= 0x800 (not overlong), not a surrogate, <= 0xFFFF
                        // (lead < 0xF0 means max cp = 0xFFFF).
                        Some((unsafe { char::from_u32_unchecked(cp) }, 2))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                // 2-byte: Latin extensions, IPA, etc. (U+0080-U+07FF)
                if !rem.is_empty() && (rem[0] & 0xC0) == 0x80 {
                    let cp = (u32::from(lead & 0x1F) << 6) | u32::from(rem[0] & 0x3F);
                    if cp >= 0x80 {
                        // SAFETY: 0x80..=0x7FF are all valid Unicode scalar values.
                        Some((unsafe { char::from_u32_unchecked(cp) }, 1))
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            if let Some((c, consumed)) = decoded {
                char_buf[count] = c;
                count += 1;
                rem = &rem[consumed..];
                if count >= 256 {
                    break;
                }
                match rem.first() {
                    Some(&next) if (0xC0..=0xF7).contains(&next) => {
                        lead = next;
                        rem = &rem[1..];
                        continue;
                    }
                    _ => break,
                }
            }
            // Invalid, overlong, or incomplete — byte-by-byte fallback
            self.start_utf8(lead);
            break;
        }

        // Dispatch decoded characters
        if count == 1 {
            sink.print(char_buf[0]);
        } else if count > 1 {
            sink.print_unicode_bulk(pty_wrap_ref(&char_buf[..count]));
        }

        orig_len - rem.len()
    }
}
