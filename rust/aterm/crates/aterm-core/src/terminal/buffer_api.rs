// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Buffer and scrollback API for [`Terminal`](super::Terminal).
//!
//! Scrollback buffer access, memory management, viewport scrolling,
//! response buffer operations, and paste formatting.
//! Extracted from `mod.rs` as part of #5524.

use super::Terminal;

/// Maximum paste size in bytes (16 MiB). Pastes exceeding this are truncated
/// at a char boundary to prevent unbounded memory allocation (#7379).
const MAX_PASTE_BYTES: usize = 16 * 1024 * 1024;

impl Terminal {
    /// Get a reference to the tiered scrollback storage, if attached.
    #[must_use]
    pub fn scrollback(&self) -> Option<&crate::scrollback::ScrollbackStorage> {
        self.grid.scrollback()
    }

    /// Get a mutable reference to the tiered scrollback storage, if attached.
    pub fn scrollback_mut(&mut self) -> Option<&mut crate::scrollback::ScrollbackStorage> {
        self.grid.scrollback_mut()
    }

    /// Drain deferred scrollback rows into attached tiered storage for all grids.
    ///
    /// The grid keeps recently scrolled rows in a lazy buffer for write-path
    /// efficiency. Diagnostics such as memory pressure need those rows promoted
    /// first so their byte accounting and watermarks reflect the full history.
    pub fn sync_scrollback_buffers(&mut self) {
        let _ = self.grid.scrollback_mut();
        if let Some(ref mut alt) = self.alt_grid {
            let _ = alt.scrollback_mut();
        }
    }

    /// Estimate total memory used by the terminal (grid + alt screen + scrollback).
    #[must_use]
    pub fn memory_used(&self) -> usize {
        let mut total = self.grid.memory_used();
        if let Some(ref alt) = self.alt_grid {
            total += alt.memory_used();
        }
        total
    }

    /// Set the scrollback memory budget (bytes) for the main and alt grids.
    ///
    /// Returns the first enforcement error encountered, if any.
    pub fn set_memory_budget(
        &mut self,
        budget: usize,
    ) -> Result<(), aterm_scrollback::ScrollbackError> {
        let mut first_err = None;
        if let Some(scrollback) = self.grid.scrollback_mut() {
            if let Err(e) = scrollback.set_memory_budget(budget) {
                first_err = Some(e);
            }
        }
        // Budget enforcement may evict scrollback lines; clamp display_offset
        // to maintain the invariant display_offset <= scrollback_lines() (#7233).
        self.grid.clamp_display_offset();
        if let Some(ref mut alt) = self.alt_grid {
            if let Some(scrollback) = alt.scrollback_mut() {
                if let Err(e) = scrollback.set_memory_budget(budget) {
                    first_err.get_or_insert(e);
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Set the retained tiered scrollback line limit.
    ///
    /// The fast ring buffer remains bounded by the grid's ring capacity; this
    /// limit applies to the older tiered storage behind that ring.
    pub fn set_scrollback_line_limit(&mut self, limit: Option<usize>) {
        if let Some(scrollback) = self.grid.scrollback_mut() {
            scrollback.set_line_limit(limit);
        }
        self.grid.clamp_display_offset();
        if let Some(ref mut alt) = self.alt_grid {
            if let Some(scrollback) = alt.scrollback_mut() {
                scrollback.set_line_limit(limit);
            }
            alt.clamp_display_offset();
        }
    }

    /// Highest scrollback watermark pressure across the main and alternate grids.
    #[must_use]
    pub fn scrollback_pressure_level(&self) -> crate::scrollback::WatermarkLevel {
        let mut level = self.grid.scrollback().map_or(
            crate::scrollback::WatermarkLevel::Green,
            aterm_scrollback::ScrollbackStorage::watermark_level,
        );
        if let Some(ref alt) = self.alt_grid {
            if let Some(scrollback) = alt.scrollback() {
                level = level.max(scrollback.watermark_level());
            }
        }
        level
    }

    /// Clear all scrollback history (main and alt grids).
    ///
    /// Resets both the ring buffer scrollback (`total_lines`, `ring_head`)
    /// and all tiers (hot, warm, cold) of the tiered scrollback.
    /// Preserves live visible rows. Clears any active text selection
    /// since scrollback-anchored selection coordinates become dangling.
    pub fn clear_scrollback(&mut self) {
        self.grid.erase_scrollback();
        if let Some(ref mut alt) = self.alt_grid {
            alt.erase_scrollback();
        }
        self.text_selection.clear();
        // Clear shell integration marks and marks state that contain absolute
        // row numbers — these become dangling references after scrollback is
        // erased (#7667).
        self.shell.command_marks.clear();
        self.shell.output_blocks.clear();
        self.shell.current_block = None;
        self.shell.current_mark = None;
        self.marks_state.marks.clear();
        self.marks_state.annotations.clear();
    }

    /// Scroll display by delta lines.
    pub fn scroll_display(&mut self, delta: i32) {
        self.grid.scroll_display(delta);
    }

    /// Scroll to top of scrollback.
    pub fn scroll_to_top(&mut self) {
        self.grid.scroll_to_top();
    }

    /// Scroll to bottom (live content).
    pub fn scroll_to_bottom(&mut self) {
        self.grid.scroll_to_bottom();
    }

    /// Take pending response data.
    ///
    /// Returns any data accumulated in the response buffer (from DSR/DA
    /// responses) and clears the buffer. The returned data should be
    /// written to the PTY.
    ///
    /// Uses `clone()+clear()` instead of `mem::take()` to preserve the
    /// internal buffer's heap allocation. Subsequent `process()` calls
    /// reuse the existing capacity instead of re-allocating (#4073).
    ///
    /// Returns `None` if the response buffer is empty.
    #[must_use]
    pub fn take_response(&mut self) -> Option<Vec<u8>> {
        if self.transient.response_buffer.is_empty() {
            None
        } else {
            let data = self.transient.response_buffer.clone();
            self.transient.response_buffer.clear();
            Some(data)
        }
    }

    /// Expose response buffer capacity for test assertions (#4544).
    #[cfg(test)]
    #[must_use]
    pub fn response_buffer_capacity(&self) -> usize {
        self.transient.response_buffer.capacity()
    }

    /// Check if there is pending response data.
    #[must_use]
    pub fn has_pending_response(&self) -> bool {
        !self.transient.response_buffer.is_empty()
    }

    /// Get the number of bytes in the response buffer.
    #[must_use]
    pub fn pending_response_len(&self) -> usize {
        self.transient.response_buffer.len()
    }

    /// Format text for pasting into the terminal.
    ///
    /// Strips terminal control bytes that can inject commands, converts line
    /// breaks to carriage returns for PTY input, and when bracketed paste mode
    /// is enabled wraps the body with the bracketed paste markers
    /// (`\x1b[200~` prefix and `\x1b[201~` suffix).
    ///
    /// This is useful for host applications that need to send paste data
    /// to the PTY in the correct format based on the terminal's current mode.
    ///
    /// # Example
    ///
    /// ```
    /// use aterm_core::terminal::Terminal;
    ///
    /// let mut term = Terminal::new(24, 80);
    ///
    /// // Without bracketed paste mode
    /// assert_eq!(term.format_paste("hello"), b"hello");
    ///
    /// // Enable bracketed paste mode
    /// term.process(b"\x1b[?2004h");
    /// assert_eq!(
    ///     term.format_paste("hello"),
    ///     b"\x1b[200~hello\x1b[201~"
    /// );
    /// ```
    #[must_use]
    pub fn format_paste(&self, text: &str) -> Vec<u8> {
        // Truncate at char boundary to prevent unbounded allocation (#7379).
        let text = if text.len() > MAX_PASTE_BYTES {
            let mut end = MAX_PASTE_BYTES;
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            &text[..end]
        } else {
            text
        };

        if self.modes.bracketed_paste {
            // Strip ESC (prevents injecting \x1b[201~ to terminate the bracket
            // region), C1 controls 0x80-0x9F (0x9B is C1 CSI which can also
            // terminate the bracket: \x9B201~), and ETX/Ctrl-C (some shells
            // incorrectly handle it inside bracketed paste).
            let sanitized: String = text
                .chars()
                .filter(|&c| c != '\x1b' && c != '\x03' && !('\u{0080}'..='\u{009F}').contains(&c))
                .collect();
            // Convert newlines to CR: terminals expect CR for line breaks in
            // pasted text; LF alone moves the cursor down without returning
            // to column 0 (#7773).
            let sanitized = sanitized.replace("\r\n", "\r").replace('\n', "\r");
            let mut result = Vec::with_capacity(sanitized.len() + 12);
            result.extend_from_slice(b"\x1b[200~");
            result.extend_from_slice(sanitized.as_bytes());
            result.extend_from_slice(b"\x1b[201~");
            result
        } else {
            // Strip ESC and C1 controls (0x80-0x9F) even outside bracketed paste.
            // C1 CSI (0x9B), C1 OSC (0x9D), C1 DCS (0x90) can inject terminal
            // commands when 8-bit controls are enabled. (#7411)
            let cleaned: String = text
                .chars()
                .filter(|&c| c != '\x1b' && !('\u{0080}'..='\u{009F}').contains(&c))
                .collect();
            // Convert newlines to CR (#7773).
            cleaned
                .replace("\r\n", "\r")
                .replace('\n', "\r")
                .into_bytes()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Terminal;

    /// The whole point of the bracket guard: a paste planted with ESC[201~
    /// must not terminate the region early and have its tail run as
    /// keystrokes. ESC is stripped, so the only ESC[201~ on the wire is the
    /// final guard and the planted "[201~" is inert text.
    #[test]
    fn bracketed_paste_blocks_embedded_escape_terminator() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b[?2004h");
        let out = term.format_paste("safe\x1b[201~rm -rf ~");
        assert_eq!(out, b"\x1b[200~safe[201~rm -rf ~\x1b[201~");
    }

    /// C1 CSI (0x9B) terminates the bracket region just like ESC[ when 8-bit
    /// controls are honored; it must be stripped too.
    #[test]
    fn bracketed_paste_blocks_c1_csi_terminator() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b[?2004h");
        let out = term.format_paste("a\u{009B}201~b");
        assert_eq!(out, b"\x1b[200~a201~b\x1b[201~");
    }

    /// Without bracketed paste there is no guard at all, so ESC and C1
    /// controls must still be stripped to keep pasted text inert.
    #[test]
    fn unbracketed_paste_strips_escape_and_c1() {
        let term = Terminal::new(24, 80);
        assert_eq!(term.format_paste("a\x1b[31mb\u{009D}c"), b"a[31mbc");
    }

    /// Line breaks become CR for PTY input, in both modes (#7773).
    #[test]
    fn paste_converts_line_breaks_to_cr() {
        let mut term = Terminal::new(24, 80);
        assert_eq!(term.format_paste("x\r\ny\nz"), b"x\ry\rz");
        term.process(b"\x1b[?2004h");
        assert_eq!(term.format_paste("x\r\ny\nz"), b"\x1b[200~x\ry\rz\x1b[201~");
    }
}
