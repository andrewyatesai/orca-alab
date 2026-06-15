// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! DCS dispatch helpers for terminal escape sequence handling.
//!
//! This module contains DCS routing and state machine helpers used by
//! `ActionSink::{dcs_hook, dcs_put, dcs_unhook}`.

use super::super::{DcsType, MAX_DCS_CALLBACK_BYTES, MAX_DCS_GLOBAL_BUDGET};
use super::TerminalHandler;

impl TerminalHandler<'_> {
    /// Begin processing a DCS (Device Control String) sequence.
    ///
    /// DCS sequences have the format: `ESC P <params> <intermediates> <final_byte> <data> ST`
    ///
    /// # Supported DCS Types
    ///
    /// - **DECRQSS** (`DCS $ q Pt ST`): Request terminal settings
    /// - **Sixel** (`DCS Ps q <data> ST`): Sixel graphics data
    /// - **XTGETTCAP** (`DCS + q Pt ST`): xterm termcap/terminfo query
    ///
    /// DECDLD (`{`), tmux control mode (`DCS 1000 p`), tmux passthrough
    /// (`DCS tmux;`), and SSH conductor (`DCS 2000 p`) are recognized shapes
    /// whose integrations are permanently compiled out — they are consumed
    /// and ignored as `Unknown`.
    ///
    /// This function identifies the DCS type and prepares state for `dcs_put` calls.
    /// The sequence is finalized by `dcs_unhook`.
    ///
    /// See `docs/ESCAPE_SEQUENCE_MATRIX.md` for complete DCS coverage.
    pub(super) fn dcs_hook_inner(&mut self, params: &[u16], intermediates: &[u8], final_byte: u8) {
        // Release budget from any abandoned prior sequence before starting
        // a new one. If dcs_hook is called without a preceding dcs_unhook
        // (parser reset mid-sequence), the old bytes would leak permanently.
        self.dcs.total_bytes = self.dcs.total_bytes.saturating_sub(self.dcs.sequence_bytes);
        self.dcs.data.clear();
        self.dcs.sequence_bytes = 0;
        self.dcs.final_byte = Some(final_byte);

        // Deactivate an abandoned Sixel decoder. If the prior DCS was a Sixel
        // sequence interrupted by a parser reset (no dcs_unhook), the decoder's
        // `active` flag is still true. Clear it to prevent stale image output
        // if `unhook()` is ever called out-of-band.
        #[cfg(feature = "sixel")]
        if matches!(self.dcs.dcs_type, DcsType::Sixel) {
            // Abort the partial image — frees pixel buffer without allocating
            // a copy (unhook() would allocate up to 64MB just to drop). (#7453)
            self.sixel.decoder.abort();
        }

        // DECRQSS: DCS $ q <Pt> ST
        // intermediates = [$], final_byte = q
        if intermediates == [b'$'] && final_byte == b'q' {
            self.dcs.dcs_type = DcsType::Decrqss;
        } else if intermediates.is_empty() && final_byte == b'q' {
            // Sixel graphics: DCS Ps1 ; Ps2 ; Ps3 q <sixel-data> ST
            // No intermediates, final byte is 'q'
            #[cfg(feature = "sixel")]
            {
                self.dcs.dcs_type = DcsType::Sixel;
                let cursor = self.grid.cursor();
                self.sixel.decoder.hook(params, cursor.row, cursor.col);
            }
            #[cfg(not(feature = "sixel"))]
            {
                self.dcs.dcs_type = DcsType::Unknown;
            }
        } else if intermediates.is_empty() && final_byte == b'{' {
            // DECDLD: DCS Pfn;Pcn;Pe;Pcmw;Pss;Pt;Pcmh;Pcss { <data> ST
            // Downloadable Character Set (soft fonts). The DRCS integration
            // is permanently compiled out; consume and ignore.
            self.dcs.dcs_type = DcsType::Unknown;
        } else if intermediates.is_empty() && final_byte == b'p' && params.first() == Some(&1000) {
            // tmux control mode: DCS 1000 p <64-hex-nonce> ST. The tmux -CC
            // integration is permanently compiled out; consume and ignore.
            self.dcs.dcs_type = DcsType::Unknown;
        } else if intermediates.is_empty() && final_byte == b'p' && params.first() == Some(&2000) {
            // SSH conductor mode: DCS 2000 p <64-hex-nonce> ST. The SSH
            // conductor integration is permanently compiled out; consume
            // and ignore.
            self.dcs.dcs_type = DcsType::Unknown;
        } else if intermediates == [b'+'] && final_byte == b'q' {
            // XTGETTCAP: DCS + q Pt ST
            // xterm termcap/terminfo query
            self.dcs.dcs_type = DcsType::Xtgettcap;
        } else if intermediates.is_empty() && final_byte == b't' && params.is_empty() {
            // tmux DCS passthrough: ESC P tmux; <escaped-content> ESC \
            // The tmux integration is permanently compiled out; inner escape
            // sequences still parse via natural parser breakout.
            self.dcs.dcs_type = DcsType::Unknown;
        } else {
            self.dcs.dcs_type = DcsType::Unknown;
        }
    }

    /// Accumulate data bytes for the current DCS sequence.
    ///
    /// Called for each byte between `dcs_hook` and `dcs_unhook`. Routes data
    /// to the appropriate handler based on the DCS type identified in `dcs_hook`:
    ///
    /// - **DECRQSS**: Accumulates parameter string (max 256 bytes)
    /// - **Sixel**: Feeds data to the Sixel decoder
    /// - **XTGETTCAP**: Accumulates query parameters
    ///
    /// Global memory budget (`MAX_DCS_GLOBAL_BUDGET`) prevents DoS via large sequences.
    pub(super) fn dcs_put_inner(&mut self, byte: u8) {
        // Accumulate data bytes for the current DCS sequence
        // Check global budget first to prevent unbounded memory growth
        if self.dcs.total_bytes >= MAX_DCS_GLOBAL_BUDGET {
            return; // Global budget exceeded, drop data
        }

        match self.dcs.dcs_type {
            DcsType::Decrqss => {
                // Always count bytes against the budget, even when the data
                // vec is capped. Otherwise flooding past the cap goes
                // untracked by the budget system.
                self.dcs.total_bytes += 1;
                self.dcs.sequence_bytes += 1;
                // Accumulate the parameter string (Pt) up to 256 bytes.
                if self.dcs.data.len() < 256 {
                    self.dcs.data.push(byte);
                }
            }
            #[cfg(feature = "sixel")]
            DcsType::Sixel => {
                // Always count Sixel bytes against the global DCS budget (#5948).
                self.dcs.total_bytes += 1;
                self.dcs.sequence_bytes += 1;
                // Track pixel buffer allocation against DCS budget (#7405).
                let alloc_before = self.sixel.decoder.pixel_alloc_bytes();
                self.sixel.decoder.put(byte);
                let alloc_after = self.sixel.decoder.pixel_alloc_bytes();
                if alloc_after > alloc_before {
                    let delta = alloc_after - alloc_before;
                    self.dcs.total_bytes += delta;
                    self.dcs.sequence_bytes += delta;
                    // Check if pixel allocation pushed us over budget.
                    if self.dcs.total_bytes > MAX_DCS_GLOBAL_BUDGET {
                        self.sixel.decoder.abort();
                        // Release pixel bytes from budget (stream bytes stay charged).
                        self.dcs.total_bytes -= alloc_after;
                        self.dcs.sequence_bytes -= alloc_after;
                    }
                }
                if self.dcs.callback.is_some() && self.dcs.data.len() < MAX_DCS_CALLBACK_BYTES {
                    self.dcs.data.push(byte);
                }
            }
            DcsType::Xtgettcap => {
                // Always count bytes against the budget, even when the data
                // vec is capped, so flooding past the cap is visible.
                self.dcs.total_bytes += 1;
                self.dcs.sequence_bytes += 1;
                // Accumulate hex-encoded capability names (Pt) up to 1024 bytes.
                if self.dcs.data.len() < 1024 {
                    self.dcs.data.push(byte);
                }
            }
            DcsType::Unknown | DcsType::None => {
                // Always count bytes against the budget, even when no callback
                // is registered. Otherwise Unknown DCS sequences bypass the
                // global budget entirely (#7367).
                self.dcs.total_bytes += 1;
                self.dcs.sequence_bytes += 1;
                if self.dcs.callback.is_some() && self.dcs.data.len() < MAX_DCS_CALLBACK_BYTES {
                    self.dcs.data.push(byte);
                }
            }
        }
    }

    /// Finalize a DCS sequence after receiving the String Terminator (ST).
    ///
    /// Processes accumulated data based on DCS type:
    /// - **DECRQSS**: Generates response for the queried setting
    /// - **Sixel**: Finalizes image and stores for retrieval
    /// - **XTGETTCAP**: Generates termcap capability responses
    ///
    /// Triggers DCS callback if registered, then resets DCS state.
    pub(super) fn dcs_unhook_inner(
        &mut self,
        cap: &super::super::response_capability::ResponseCapability,
    ) {
        // Process the complete DCS sequence
        match self.dcs.dcs_type {
            DcsType::Decrqss => {
                self.handle_decrqss(cap);
            }
            #[cfg(feature = "sixel")]
            DcsType::Sixel => {
                if let Some(image) = self.sixel.decoder.unhook() {
                    self.sixel.pending_image = Some(image);
                    self.sixel.next_id += 1;
                }
            }
            DcsType::Xtgettcap => {
                self.handle_xtgettcap(cap);
            }
            DcsType::Unknown | DcsType::None => {
                // Nothing to do
            }
        }

        // #8009 CF-013: structural gate on raw DCS callback delivery.
        // The payload in `self.dcs.data` is PTY-origin (accumulated by
        // the parser from PTY bytes). `invoke_dcs_callback` wraps it in
        // `Provenance<&[u8], Pty>` at the emission site before erasing
        // provenance at the FFI boundary. The capability token proves
        // the host has authorized raw-bytes callback delivery; a
        // revoked `DcsAuth` drops the payload silently.
        if let (Some(callback), Some(final_byte), Some(token)) = (
            self.dcs.callback.as_mut(),
            self.dcs.final_byte,
            self.dcs_auth.try_mint_capability(),
        ) {
            super::super::dcs_auth::invoke_dcs_callback(
                callback,
                token,
                self.dcs.data.as_slice(),
                final_byte,
            );
        }

        // Reset DCS state and release global budget.
        // Use sequence_bytes (not data.len()) because Sixel feeds bytes to the
        // decoder without accumulating in data — data.len() would under-release (#5948).
        self.dcs.dcs_type = DcsType::None;
        self.dcs.total_bytes = self.dcs.total_bytes.saturating_sub(self.dcs.sequence_bytes);
        self.dcs.sequence_bytes = 0;
        self.dcs.data.clear();
        // Shrink retained capacity after large sequences (mirrors osc_data shrink_to #7272).
        if self.dcs.data.capacity() > 4096 {
            self.dcs.data.shrink_to(128);
        }
        self.dcs.final_byte = None;
    }
}
