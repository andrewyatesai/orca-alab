// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Character write-path helpers for terminal rendering.
//!
//! This module contains the hot path for writing translated characters,
//! REP writes, and combining-character/ZWJ continuation behavior.

use std::sync::Arc;

use crate::grid::{Cell, CellFlags, row_u16};

use super::TerminalHandler;

/// Fast character width lookup with multi-tier fast-path.
///
/// Tier 1: ASCII (0x20-0x7E) — always width 1, covers ~90% of terminal content.
/// Tier 2: Latin-1 Supplement through Spacing Modifiers (0xA0-0x02FF) — all width 1.
/// Tier 3: CJK main blocks (U+3000-U+9FFF) and Hangul Syllables (U+AC00-U+D7A3) —
///         always width 2, except two combining marks (U+3099-U+309A).
/// Tier 4: SMP Emoji (U+1F300-U+1F6FF) — O(1) bitmap lookup.
///         BMP Emoji (U+2600-U+27BF) — O(1) bitmap lookup.
///         CJK Extension B-H + Compat Ideographs Supp (U+20000-U+2FA1F, U+30000-U+323AF) — width 2.
/// Tier 5: Full aterm_grapheme::char_width lookup for everything else.
///         Uses `width_cjk()` when `cjk` is true for East Asian Ambiguous chars.
#[inline]
fn char_width(c: char, cjk: bool) -> usize {
    let cp = c as u32;
    // Tier 2 fast path only safe when NOT in CJK mode — the 0xA0-0x02FF range
    // contains many EA Width "Ambiguous" characters (°, §, ±, ×, ÷, etc.)
    // that should be width 2 in CJK mode.
    if (0x20..0x7F).contains(&cp) || (!cjk && (0xA0..0x0300).contains(&cp)) {
        1
    } else if (0x3000..0xA000).contains(&cp) {
        // CJK Symbols, Hiragana, Katakana, Bopomofo, CJK Extensions, CJK Ideographs.
        // All width 2 except:
        // - U+302A-U+302F: CJK tone marks (Mn/Mc category, zero-width)
        // - U+3099-U+309A: combining Katakana voicing marks
        // - U+4DC0-U+4DFF: Yijing Hexagram Symbols (East Asian Width "N", width 1)
        if (0x302A..=0x302F).contains(&cp) || cp == 0x3099 || cp == 0x309A {
            0
        } else if (0x4DC0..=0x4DFF).contains(&cp) {
            1
        } else {
            2
        }
    } else if (0xAC00..0xD7A4).contains(&cp) {
        2 // Hangul Syllables — always width 2
    } else if (0x1F300..0x1F700).contains(&cp) {
        // SMP Emoji blocks (Misc Symbols, Emoticons, Ornamental Dingbats, Transport).
        smp_emoji_width(cp)
    } else if (0x2600..0x27C0).contains(&cp) {
        // BMP Misc Symbols + Dingbats (✨ U+2728, ⚡ U+26A1, zodiac, etc.).
        bmp_emoji_width(cp)
    } else if (0x20000..0x2FA20).contains(&cp) || (0x30000..0x323B0).contains(&cp) {
        // CJK Extension B through Compat Ideographs Supp (U+20000-U+2FA1F): all width 2.
        // CJK Extensions G (U+30000-U+3134F) and H (U+31350-U+323AF): all width 2.
        2
    } else if cjk {
        aterm_grapheme::char_width_cjk(c)
    } else {
        aterm_grapheme::char_width(c)
    }
}

/// Bitmap of width-2 codepoints in U+1F300-U+1F6FF (128 bytes, L1-cache friendly).
///
/// Generated from Unicode East Asian Width tables. Each bit = one codepoint:
/// set = width 2, clear = width 1/0. Covers Miscellaneous Symbols and
/// Pictographs, Emoticons, Ornamental Dingbats, and Transport/Map Symbols.
#[rustfmt::skip]
static SMP_EMOJI_WIDTH2: [u8; 128] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0xE0, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xDF,
    0xFF, 0xFF, 0x0F, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x87, 0x0F, 0x00, 0xFF, 0xFF, 0x11, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x9F,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x3F, 0x00, 0x78, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x04,
    0x00, 0x00, 0x60, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x3F, 0x10, 0xE7, 0xF0, 0x00, 0x18, 0xF0, 0x1F,
];

/// O(1) bitmap lookup for SMP emoji width (U+1F300-U+1F6FF).
#[inline]
fn smp_emoji_width(cp: u32) -> usize {
    let idx = (cp - 0x1F300) as usize;
    if SMP_EMOJI_WIDTH2[idx / 8] & (1 << (idx % 8)) != 0 {
        2
    } else {
        // Rare text-presentation symbols: fall through for correctness.
        // SAFETY: cp is guaranteed to be a valid Unicode codepoint (U+1F300-U+1F6FF).
        aterm_grapheme::char_width(unsafe { char::from_u32_unchecked(cp) }).max(1)
    }
}

/// Bitmap of width-2 codepoints in U+2600-U+27BF (56 bytes).
///
/// Covers Miscellaneous Symbols (U+2600-U+26FF) and Dingbats (U+2700-U+27BF).
/// Only 10% of codepoints are width 2 (zodiac signs, ✨, ⚡, misc emoji).
#[rustfmt::skip]
static BMP_EMOJI_WIDTH2: [u8; 56] = [
    0x00, 0x00, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x80,
    0x00, 0x00, 0x08, 0x00, 0x02, 0x0C, 0x00, 0x60, 0x30, 0x40, 0x10, 0x00, 0x00, 0x04, 0x2C, 0x24,
    0x20, 0x0C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x50, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0xE0, 0x00, 0x00, 0x00, 0x01, 0x80,
];

/// O(1) bitmap lookup for BMP emoji width (U+2600-U+27BF).
#[inline]
fn bmp_emoji_width(cp: u32) -> usize {
    let idx = (cp - 0x2600) as usize;
    if BMP_EMOJI_WIDTH2[idx / 8] & (1 << (idx % 8)) != 0 {
        2
    } else {
        // Fallback to char_width for bitmap misses, matching smp_emoji_width().
        // SAFETY: cp is guaranteed to be a valid Unicode codepoint (U+2600-U+27BF).
        aterm_grapheme::char_width(unsafe { char::from_u32_unchecked(cp) }).max(1)
    }
}

impl TerminalHandler<'_> {
    /// Write a character to the grid with current style.
    pub(super) fn write_char(&mut self, c: char) {
        // Translate character through the active character set
        let translated = self.charset.translate(c);

        // Capture for CopyToClipboard mode
        if let Some(state) = self.clipboard.copy_state.as_mut() {
            state.push(translated);
        }

        let width = char_width(translated, self.modes.ambiguous_width_double);

        if width == 0 {
            // Track whether this combining character is a ZWJ for fast-path skipping.
            self.transient.last_combining_was_zwj = translated == '\u{200D}';
            self.add_combining_to_previous_cell(translated);
            // VS16 (U+FE0F): emoji presentation selector widens eligible base
            // characters from 1 cell to 2 cells, matching kitty/WezTerm/foot.
            if translated == '\u{FE0F}' {
                self.widen_previous_cell_for_vs16();
            }
            // VS15 (U+FE0E): text presentation selector narrows emoji from
            // 2 cells to 1 cell, the inverse of VS16.
            if translated == '\u{FE0E}' {
                self.narrow_previous_cell_for_vs15();
            }
            return;
        }

        // Store for REP (CSI b): only track width > 0 graphic characters.
        // Combining marks and ZWJ are not "preceding graphic characters" per
        // ECMA-48 §8.3.103 and should not be repeated by REP.
        // Track the RAW received char, not the translated glyph: xterm
        // CASE_REP re-translates `lastchar` through the CURRENT GL charset
        // (dotext(xw, screen->gsets[curgl], ...)), so a charset designation
        // between the print and the REP changes the repeated glyph.
        self.transient.last_graphic_char = Some(c);

        // Emoji skin tone modifiers (U+1F3FB-U+1F3FF) should combine with the
        // preceding emoji base rather than rendering as separate 2-cell characters.
        if is_emoji_skin_tone_modifier(translated)
            && self.try_combine_skin_tone_modifier(translated)
        {
            return;
        }

        // Regional-indicator pairs (U+1F1E6-U+1F1FF) form ONE flag glyph: combine
        // the 2nd RI of a pair into the 1st RI's cell so `🇺🇸` is a single 2-cell
        // grapheme. The colour font has a bitmap for the PAIR, not single RIs.
        if is_regional_indicator(translated)
            && self.try_combine_regional_indicator(translated)
        {
            return;
        }

        // ZWJ sequence continuation: combine with previous cell for emoji sequences.
        // Fast-path: skip the expensive grid lookup unless the last combining char was ZWJ.
        if self.transient.last_combining_was_zwj && self.should_combine_with_previous_zwj() {
            self.add_combining_to_previous_cell(translated);
            return;
        }
        self.transient.last_combining_was_zwj = false;

        self.write_char_core(translated, width);
    }

    /// Bulk write path for runs of non-ASCII characters.
    ///
    /// Called by the parser when 2+ consecutive multi-byte UTF-8 sequences are
    /// decoded. Checks preconditions once and dispatches to a tight inner loop,
    /// skipping per-character charset translate, clipboard capture, char_width,
    /// ZWJ tracking, and style/extras computation.
    ///
    /// Falls back to per-character `write_char` when preconditions aren't met
    /// (VT52, insert mode, no autowrap, active clipboard, pending ZWJ, extras).
    #[allow(
        clippy::too_many_lines,
        reason = "hot-path character dispatch with many optimized branches"
    )]
    pub(super) fn write_unicode_bulk(&mut self, chars: &[char]) {
        // Precondition: must NOT be in VT52 cursor addressing, insert mode,
        // no-autowrap, or clipboard capture mode, and must not have style extras.
        if self.transient.vt52_cursor_state != super::Vt52CursorState::None
            || self.modes.insert_mode
            || !self.modes.auto_wrap
            || self.clipboard.copy_state.is_some()
            || self.transient.has_transient_extras
            || self.style.has_style_extras()
            || self.transient.last_combining_was_zwj
        {
            for &c in chars {
                self.write_char(c);
            }
            return;
        }

        // Non-ASCII chars bypass charset translation for the GL range (>= 0x100).
        // However, characters in U+00A0-U+00FF may need GR-mapped translation
        // when a non-ASCII charset is designated on the GR-mapped G-set (#7546).
        // Fall back to per-char processing when GR translation is active.
        if !self.charset.gr_is_passthrough() {
            for &c in chars {
                self.write_char(c);
            }
            return;
        }
        // Clear single_shift once for the entire batch.
        self.charset.clear_single_shift();

        // Cache ambiguous-width mode for the entire bulk run.
        let cjk = self.modes.ambiguous_width_double;

        // Pre-compute style state once for the entire run.
        let colors = self.style.cached_colors();
        let flags = if self.style.protected {
            self.style.flags.union(CellFlags::PROTECTED)
        } else {
            self.style.flags
        };
        // Pre-compute complex flags for non-BMP emoji (hoisted from inner loop).
        let complex_flags = flags.union(CellFlags::COMPLEX);

        // Track last graphic char across the bulk run for REP (CSI b).
        // Only updated for width > 0 characters.
        let mut last_graphic: Option<char> = None;

        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let cp = c as u32;

            // Width classification: inline the hot tiers of char_width.
            // CJK main blocks (U+3000-U+9FFF) and Hangul (U+AC00-U+D7A3) are width 2.
            // Latin Supplement through Spacing Modifiers (U+00A0-U+02FF) are width 1.
            // Everything else goes through the per-character slow path.
            if (0x3000..0xA000).contains(&cp) {
                if (0x302A..=0x302F).contains(&cp) || cp == 0x3099 || cp == 0x309A {
                    // CJK combining marks (U+302A-302F tone marks, U+3099-309A voicing) — width 0
                    self.transient.last_combining_was_zwj = false;
                    self.add_combining_to_previous_cell(c);
                    i += 1;
                    continue;
                }
                // Yijing Hexagram Symbols (U+4DC0-U+4DFF) are width 1, not CJK width 2.
                if (0x4DC0..=0x4DFF).contains(&cp) {
                    self.grid.write_narrow_autowrap_fast(c, colors, flags);
                    last_graphic = Some(c);
                    self.transient.last_combining_was_zwj = false;
                    i += 1;
                    continue;
                }
                // BMP CJK width-2: find run of consecutive CJK/Hangul chars
                let run_start = i;
                i += 1;
                while i < chars.len() {
                    let cp2 = chars[i] as u32;
                    if ((0x3000..0xA000).contains(&cp2)
                        && !(0x302A..=0x302F).contains(&cp2)
                        && cp2 != 0x3099
                        && cp2 != 0x309A
                        && !(0x4DC0..=0x4DFF).contains(&cp2))
                        || (0xAC00..0xD7A4).contains(&cp2)
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                // Batch write the entire CJK/Hangul run
                self.grid
                    .write_wide_run_autowrap(&chars[run_start..i], colors, flags);
                last_graphic = Some(chars[i - 1]);
                self.transient.last_combining_was_zwj = false;
                continue;
            } else if (0xAC00..0xD7A4).contains(&cp) {
                // Hangul Syllables — always width 2
                self.grid.write_wide_autowrap_fast(c, colors, flags);
            } else if cp > 0xFFFF {
                // Non-BMP (emoji, math symbols, etc.)
                let width = char_width(c, cjk);
                if width == 0 {
                    self.transient.last_combining_was_zwj = c == '\u{200D}';
                    self.add_combining_to_previous_cell(c);
                    i += 1;
                    continue;
                }
                if width == 2 {
                    // ZWJ continuation: combine with previous cell for emoji sequences.
                    if self.transient.last_combining_was_zwj
                        && self.should_combine_with_previous_zwj()
                    {
                        self.add_combining_to_previous_cell(c);
                        self.transient.last_combining_was_zwj = false;
                        i += 1;
                        continue;
                    }
                    // Skin tone modifiers combine with previous emoji base.
                    if is_emoji_skin_tone_modifier(c) && self.try_combine_skin_tone_modifier(c) {
                        self.transient.last_combining_was_zwj = false;
                        i += 1;
                        continue;
                    }
                    // Regional-indicator pairs form one flag glyph (see write_char).
                    if is_regional_indicator(c)
                        && self.try_combine_regional_indicator(c)
                    {
                        self.transient.last_combining_was_zwj = false;
                        i += 1;
                        continue;
                    }
                    self.transient.last_combining_was_zwj = false;
                    // Find run of consecutive width-2 chars (both BMP and non-BMP)
                    // for batching. Extending runs to include BMP emoji like
                    // ✨ U+2728 and ⚡ U+26A1 avoids per-char dispatch overhead
                    // when they appear adjacent to SMP emoji.
                    let run_start = i;
                    i += 1;
                    while i < chars.len() {
                        let c2 = chars[i];
                        let cp2 = c2 as u32;
                        // Break before a skin-tone modifier or a regional
                        // indicator so each is processed individually and can
                        // combine into the preceding cell next iteration.
                        if is_emoji_skin_tone_modifier(c2) || is_regional_indicator(c2) {
                            break;
                        }
                        if (0x1F300..0x1FB00).contains(&cp2)
                            || (0x20000..0x2FA20).contains(&cp2)
                            || (0x30000..0x323B0).contains(&cp2)
                            || char_width(c2, cjk) == 2
                        {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    // Batch write mixed wide run — handles BMP and non-BMP
                    self.grid.write_mixed_wide_run_autowrap(
                        &chars[run_start..i],
                        colors,
                        flags,
                        complex_flags,
                    );
                    last_graphic = Some(chars[i - 1]);
                    continue;
                }
                // Rare: width-1 non-BMP (some math symbols)
                self.write_char_core(c, width);
            } else if !cjk && (0xA0..0x300).contains(&cp) {
                // Latin Supplement through Spacing Modifiers — width 1 when not CJK.
                // Most common non-ASCII in European text. Skip char_width().
                // In CJK mode, many chars here are ambiguous-width → fall through.
                self.grid.write_narrow_autowrap_fast(c, colors, flags);
            } else {
                // BMP non-CJK: compute width and use standard path
                let width = char_width(c, cjk);
                if width == 0 {
                    self.transient.last_combining_was_zwj = c == '\u{200D}';
                    self.add_combining_to_previous_cell(c);
                    if c == '\u{FE0F}' {
                        self.widen_previous_cell_for_vs16();
                    }
                    if c == '\u{FE0E}' {
                        self.narrow_previous_cell_for_vs15();
                    }
                    i += 1;
                    continue;
                }
                // Width-1 BMP chars (Greek, Cyrillic, etc.)
                // or width-2 BMP chars outside CJK main blocks
                if width == 2 {
                    self.grid.write_wide_autowrap_fast(c, colors, flags);
                } else {
                    self.grid.write_narrow_autowrap_fast(c, colors, flags);
                }
            }
            last_graphic = Some(c);
            self.transient.last_combining_was_zwj = false;
            i += 1;
        }

        // Update last_graphic_char for REP (CSI b): only width > 0 chars.
        if let Some(c) = last_graphic {
            self.transient.last_graphic_char = Some(c);
        }
    }

    /// Shared write path: insert, write at cursor, apply extras, advance.
    ///
    /// Uses split write/advance primitives so that extras (hyperlinks,
    /// non-BMP overflow, underline colors, RGB) are applied at the correct
    /// cursor position BEFORE the cursor advances and potentially triggers
    /// an autowrap scroll that shifts the written row.
    fn write_char_core(&mut self, c: char, width: usize) {
        // Read colors/flags directly from CurrentStyle — avoids StyleTable lookup.
        // CurrentStyle is already kept in sync by update_style_id() on SGR changes.
        let mut flags = if self.style.protected {
            self.style.flags.union(CellFlags::PROTECTED)
        } else {
            self.style.flags
        };

        // Non-BMP characters (emoji, math symbols, etc.) need overflow storage.
        // Pre-set the COMPLEX flag so readers know to look up the actual codepoint.
        let is_non_bmp = (c as u32) > 0xFFFF;
        if is_non_bmp {
            flags = flags.union(CellFlags::COMPLEX);
        }

        // Pre-compute whether this character needs CellExtras HashMap overflow.
        // Non-BMP complex chars use the dense ring buffer (O(1) flat array)
        // instead of the HashMap, so they don't need the HAS_EXTRAS flag.
        // Uses cached booleans to avoid 5 per-character Option/bitfield checks.
        // Both flags are updated at mutation time (SGR, OSC 8) — not per character.
        let needs_style_extras =
            self.transient.has_transient_extras || self.style.has_style_extras();

        // Use cached packed colors — computed once per SGR change, not per character.
        // Only set HAS_EXTRAS for style extras (HashMap). Non-BMP uses the ring
        // buffer and doesn't need the flag — readers check COMPLEX independently.
        let colors = if needs_style_extras {
            self.style.cached_colors().with_extras_flag()
        } else {
            self.style.cached_colors()
        };

        // FAST PATH: Wide char with autowrap, no insert mode, no style extras.
        // Combined pre-wrap + write + damage + advance in a single Grid call,
        // eliminating 4-6 separate method calls and redundant bounds checks.
        if width == 2 && self.modes.auto_wrap && !self.modes.insert_mode && !needs_style_extras {
            if is_non_bmp {
                // Non-BMP emoji: combined write + ring-buffer codepoint store (no Arc)
                self.grid.write_emoji_autowrap_fast(c, colors, flags);
            } else {
                // BMP CJK: combined write (no ring buffer needed)
                self.grid.write_wide_autowrap_fast(c, colors, flags);
            }
            return;
        }

        // Resolve deferred wrap before writing the next character.
        // This matches xterm behavior: the wrap only happens when the next
        // printable character arrives, not when the last column is filled.
        // xterm consumes do_wrap at print time UNCONDITIONALLY and wraps
        // only when WRAPAROUND is set (charproc.c dotext: `do_wrap = False;
        // if (flags & WRAPAROUND) WrapLine;`) — with autowrap off the flag
        // is discarded and the write overstrikes the margin column (it is
        // re-armed by the no-wrap advance below when it fills the margin).
        if self.modes.auto_wrap {
            self.grid.resolve_pending_wrap();
        } else {
            self.grid.set_pending_wrap(false);
        }

        // Phase 1: Write character at cursor (no cursor advance yet).
        // Wide chars use _ecols variants to compute effective_cols once instead
        // of 3 separate ring-buffer lookups (pre-wrap, write, advance).
        let (did_write, ecols) = if width == 2 {
            let mut ecols = self.grid.effective_cols_for_current_row();
            if self.modes.auto_wrap {
                ecols = self.grid.pre_wrap_wide_ecols(ecols);
            }
            // In insert mode without auto-wrap, check that the wide char fits
            // before inserting blanks. Otherwise insert_chars shifts content
            // right but the write fails, leaving a spurious blank (#7483).
            if self.modes.insert_mode {
                if !self.modes.auto_wrap && self.grid.cursor_col().saturating_add(1) >= ecols {
                    // Wide char doesn't fit — no-op, matching xterm.
                    return;
                }
                // IRM insert must respect horizontal margins like ICH (#7580).
                self.grid
                    .insert_chars_margin(row_u16(width), self.modes.left_right_margin_mode);
            }
            let ok = self
                .grid
                .write_wide_char_at_cursor_packed_ecols(c, colors, flags, ecols);
            (ok, ecols)
        } else {
            if self.modes.insert_mode {
                // IRM insert must respect horizontal margins like ICH (#7580).
                self.grid
                    .insert_chars_margin(row_u16(width), self.modes.left_right_margin_mode);
            }
            self.grid.write_char_at_cursor_packed(c, colors, flags);
            (true, 0) // ecols unused for width-1
        };

        if !did_write {
            return;
        }

        // Phase 2: Apply extras at the correct position.
        // The cursor is still on the written character, so cursor_row/col
        // are the actual write coordinates — unaffected by any future scroll.
        let row = self.grid.cursor_row();
        let col = self.grid.cursor_col();

        // Apply extras at the correct position.
        // Non-BMP: store codepoint in ring buffer (O(1) flat array, ~1ns) instead of
        // HashMap entry (~15ns). No Arc allocation. The COMPLEX flag was set in Phase 1.
        if is_non_bmp {
            self.grid.set_complex_char_ring(row, col, c);
        }
        // Style extras (hyperlinks, underline color, RGB, extended flags)
        // still use the HashMap via the preflagged path.
        if needs_style_extras {
            self.apply_cell_extras_preflagged(row, col, width);
        }

        // Phase 3: Advance cursor (may trigger autowrap + scroll).
        // Wide chars reuse the pre-computed ecols to avoid a third ring-buffer lookup.
        if width == 2 {
            if self.modes.auto_wrap {
                self.grid.advance_cursor_wide_wrap_ecols(ecols);
            } else {
                self.grid.advance_cursor_wide_no_wrap_ecols(ecols);
            }
        } else if self.modes.auto_wrap {
            self.grid.advance_cursor_wrap();
        } else {
            self.grid.advance_cursor_no_wrap();
        }
    }

    /// Apply hyperlink, underline color, RGB, and extended flags to written cell(s).
    ///
    /// Uses `cell_extra_mut_preflagged` — the caller must have already set
    /// the HAS_EXTRAS bit in the cell's PackedColors during the write step.
    fn apply_cell_extras_preflagged(&mut self, row: u16, col: u16, width: usize) {
        let flags = if self.style.protected {
            self.style.flags.union(CellFlags::PROTECTED)
        } else {
            self.style.flags
        };

        let has_hyperlink = self.transient.current_hyperlink.is_some();
        let has_underline_color = self.transient.current_underline_color.is_some();
        let has_extended = flags.has_extended_flags();
        let fg_rgb = if self.style.fg.is_rgb() {
            Some(self.style.fg.rgb_components())
        } else {
            None
        };
        let bg_rgb = if self.style.bg.is_rgb() {
            Some(self.style.bg.rgb_components())
        } else {
            None
        };

        if !has_hyperlink
            && !has_underline_color
            && !has_extended
            && fg_rgb.is_none()
            && bg_rgb.is_none()
        {
            return;
        }

        // Apply to primary cell and optional wide continuation
        let cols = if width == 2 && col + 1 < self.grid.cols() {
            2
        } else {
            1
        };
        for i in 0..cols {
            // HAS_EXTRAS flag already set in PackedColors during write step —
            // skip the redundant ring-buffer row_index lookup.
            let extra = self.grid.cell_extra_mut_preflagged(row, col + i);
            if let Some(ref hyperlink) = self.transient.current_hyperlink {
                extra.set_hyperlink(Some(Arc::clone(hyperlink)));
                if let Some(ref id) = self.transient.current_hyperlink_id {
                    extra.set_hyperlink_id(Some(Arc::clone(id)));
                }
            }
            if let Some(color) = self.transient.current_underline_color {
                extra.set_underline_color_u32(Some(color));
            }
            if has_extended {
                extra.set_extended_flags(flags.extended_flags().bits());
            }
            if let Some((r, g, b)) = fg_rgb {
                extra.set_fg_rgb(Some([r, g, b]));
            }
            if let Some((r, g, b)) = bg_rgb {
                extra.set_bg_rgb(Some([r, g, b]));
            }
        }

        // Enforce hyperlink entry limit to prevent memory exhaustion from
        // OSC 8 spam with unique URLs (#7172). The check is O(1) when under
        // the limit (just a HashMap::len() comparison).
        if has_hyperlink {
            self.grid.enforce_hyperlink_limit();
        }
    }

    /// Find the previous effective cell, skipping wide continuation cells.
    ///
    /// Returns `None` at position (0, 0) where no previous cell exists.
    /// Handles column 0 by wrapping to the last column of the previous row
    /// (for combining chars at the start of a wrapped line). If the target
    /// is a wide continuation cell, returns the main wide cell instead.
    ///
    /// When `pending_wrap` is set, the cursor sits ON the last written
    /// character (not one past it), so the target is the cursor cell itself.
    fn previous_effective_cell(&self) -> Option<(u16, u16)> {
        let row = self.grid.cursor_row();
        let col = self.grid.cursor_col();

        if col == 0 && row == 0 && !self.grid.pending_wrap() {
            return None;
        }

        // When pending_wrap is set, the cursor is ON the last written char.
        // Without pending_wrap, the cursor is one past the last written char.
        let (target_row, target_col) = if self.grid.pending_wrap() {
            (row, col)
        } else if col > 0 {
            (row, col - 1)
        } else {
            // Only cross line boundary if current row is a soft-wrapped
            // continuation. Hard newlines mean column 0 has no predecessor
            // on the previous line.
            let is_continuation = self.grid.row(row).is_some_and(aterm_grid::Row::is_wrapped);
            if !is_continuation {
                return None;
            }
            let prev_row = row.saturating_sub(1);
            (
                prev_row,
                self.grid.effective_cols_for_row(prev_row).saturating_sub(1),
            )
        };

        // Skip a wide continuation cell to land on its main cell. Use the
        // context-aware check: the raw `Cell::is_wide_continuation()` shares
        // bit 10 with PROTECTED, so a DECSCA-protected base char would be
        // misread as a spacer and a following combining mark / VS16 would attach
        // to the wrong cell.
        let (final_row, final_col) =
            if target_col > 0 && self.grid.is_wide_continuation_at(target_row, target_col) {
                (target_row, target_col - 1)
            } else {
                (target_row, target_col)
            };

        Some((final_row, final_col))
    }

    /// Add a combining character to the previous cell.
    ///
    /// Combining characters (like accents) attach to the base character in the
    /// previous cell. For wide characters, we attach to the main cell (not the
    /// continuation).
    fn add_combining_to_previous_cell(&mut self, combining: char) {
        let Some((row, col)) = self.previous_effective_cell() else {
            return;
        };
        self.grid.cell_extra_mut(row, col).add_combining(combining);
        self.grid.damage_mut().mark_cell(row, col);
    }

    /// Check if the previous cell ends with ZWJ (Zero Width Joiner).
    ///
    /// Used to detect ZWJ sequences like emoji family sequences where multiple
    /// emoji should render as a single grapheme (e.g., 👨‍💻 = 👨 + ZWJ + 💻).
    fn should_combine_with_previous_zwj(&self) -> bool {
        const ZWJ: char = '\u{200D}';

        let Some((row, col)) = self.previous_effective_cell() else {
            return false;
        };

        self.grid
            .cell_extra(row, col)
            .and_then(|extra| extra.combining().last().copied())
            == Some(ZWJ)
    }

    /// Widen the previous cell from 1-cell to 2-cell when VS16 (U+FE0F) follows
    /// an emoji-capable base character.
    ///
    /// Modern terminals (kitty, WezTerm, foot) treat VS16 as an emoji presentation
    /// selector that converts text-presentation emoji (width 1) to emoji-presentation
    /// (width 2). This function:
    /// 1. Checks if the previous cell's base char is emoji-capable
    /// 2. Sets the WIDE flag on the base cell
    /// 3. Writes a WIDE_CONTINUATION spacer in the next column
    /// 4. Advances the cursor to account for the extra column consumed
    fn widen_previous_cell_for_vs16(&mut self) {
        let Some((row, col)) = self.previous_effective_cell() else {
            return;
        };

        // Already wide — nothing to do.
        let Some(cell) = self.grid.cell(row, col) else {
            return;
        };
        if cell.is_wide() {
            return;
        }

        // Read the base character and check if it's emoji-capable.
        // For COMPLEX cells (non-BMP), cell.char() returns U+FFFD — resolve
        // the real codepoint from the overflow table (#7457).
        let base_char = if cell.is_complex() {
            self.grid.resolved_char(row, col).unwrap_or('\u{FFFD}')
        } else {
            cell.char()
        };
        if !is_vs16_emoji_capable(base_char) {
            return;
        }

        // Snapshot the base cell's raw data so we can reconstruct it with WIDE.
        let char_data = cell.char_data();
        let colors = cell.colors();
        let base_flags = cell.flags();

        // Determine where the continuation cell goes. The cursor is already
        // past the base cell (at col+1) unless pending_wrap is set.
        let cont_col = col + 1;

        // Check that the continuation column is within bounds.
        // Use effective_cols_for_row to handle DECDWL lines (#7457).
        if cont_col >= self.grid.effective_cols_for_row(row) {
            return;
        }

        // Rebuild the base cell with the WIDE flag added and write it via
        // Row::set(), which sets HAS_WIDE_CHARS and DIRTY on the row.
        let wide_base =
            crate::grid::Cell::from_raw_parts(char_data, colors, base_flags.union(CellFlags::WIDE));
        let cont_cell =
            crate::grid::Cell::from_raw_parts(' ' as u16, colors, CellFlags::WIDE_CONTINUATION);
        // If the continuation column currently holds the first half of a
        // different wide character, its second half will become an orphaned
        // WIDE_CONTINUATION cell. Detect this before writing (#7656).
        let ecols = self.grid.effective_cols_for_row(row);
        let orphan_col = if self.grid.cell(row, cont_col).is_some_and(Cell::is_wide) {
            let oc = cont_col + 1;
            if oc < ecols { Some(oc) } else { None }
        } else {
            None
        };

        if let Some(row_data) = self.grid.row_mut(row) {
            // Wide char fixup: if the cell we're about to overwrite with
            // WIDE_CONTINUATION was itself the first half of a wide char,
            // clear the orphaned continuation at cont_col + 1.
            if row_data
                .flags()
                .contains(crate::grid::RowFlags::HAS_WIDE_CHARS)
            {
                if let Some(existing) = row_data.get(cont_col) {
                    if existing.flags().contains(CellFlags::WIDE) {
                        let orphan_col = cont_col + 1;
                        if orphan_col < row_data.cols() {
                            row_data.set(orphan_col, crate::grid::Cell::EMPTY);
                        }
                    }
                }
            }
            row_data.set(col, wide_base);
            row_data.set(cont_col, cont_cell);
        }

        // Mark all affected cells as damaged.
        if let Some(oc) = orphan_col {
            self.grid.damage_mut().mark_cell(row, oc);
        }
        self.grid.damage_mut().mark_cell(row, col);
        self.grid.damage_mut().mark_cell(row, cont_col);

        // Advance cursor: the continuation cell consumed the column the cursor
        // was sitting on, so we need to move forward by 1. Handle wrap state.
        if self.grid.pending_wrap() {
            // Cursor was already at the last column (pending_wrap set after
            // writing the base char at col). The continuation cell is at col+1
            // which is past end-of-line — we keep pending_wrap set.
            // Nothing more to do.
        } else if self.modes.auto_wrap {
            self.grid.advance_cursor_wrap();
        } else {
            self.grid.advance_cursor_no_wrap();
        }
    }

    /// Narrow the previous cell from 2-cell to 1-cell when VS15 (U+FE0E) follows
    /// a wide emoji character.
    ///
    /// VS15 is the text presentation selector — the inverse of VS16. When it
    /// follows a wide emoji (width 2), this function:
    /// 1. Checks if the previous cell has the WIDE flag
    /// 2. Clears the WIDE flag on the base cell
    /// 3. Sets the continuation cell (spacer) to EMPTY
    /// 4. Does NOT change the cursor position (VS15 is width 0)
    fn narrow_previous_cell_for_vs15(&mut self) {
        let Some((row, col)) = self.previous_effective_cell() else {
            return;
        };

        // Only narrow if the previous cell is currently wide.
        let Some(cell) = self.grid.cell(row, col) else {
            return;
        };
        if !cell.is_wide() {
            return;
        }

        // Snapshot the base cell's raw data so we can reconstruct without WIDE.
        let char_data = cell.char_data();
        let colors = cell.colors();
        let base_flags = cell.flags();

        // The continuation cell is always at col + 1.
        let cont_col = col + 1;
        if cont_col >= self.grid.effective_cols_for_row(row) {
            return;
        }

        // Rebuild the base cell without the WIDE flag.
        let narrow_base = crate::grid::Cell::from_raw_parts(
            char_data,
            colors,
            base_flags.difference(CellFlags::WIDE),
        );

        if let Some(row_data) = self.grid.row_mut(row) {
            row_data.set(col, narrow_base);
            row_data.set(cont_col, crate::grid::Cell::EMPTY);
        }

        // Mark affected cells as damaged.
        self.grid.damage_mut().mark_cell(row, col);
        self.grid.damage_mut().mark_cell(row, cont_col);

        // Cursor position does NOT change — VS15 is a zero-width selector.
    }

    /// Try to combine a skin tone modifier with the previous emoji cell.
    ///
    /// Emoji skin tone modifiers (U+1F3FB-U+1F3FF) should attach to the
    /// preceding emoji base as combining characters, not render as separate
    /// 2-cell wide characters. Returns `true` if the modifier was combined,
    /// `false` if it should fall through to normal rendering.
    fn try_combine_skin_tone_modifier(&mut self, modifier: char) -> bool {
        let Some((row, col)) = self.previous_effective_cell() else {
            return false;
        };

        // Check if the previous cell contains an emoji base.
        let Some(cell) = self.grid.cell(row, col) else {
            return false;
        };

        // The previous cell must be wide (emoji are width 2) to accept a modifier.
        if !cell.is_wide() {
            return false;
        }

        // Resolve the base character to verify it's an emoji modifier base.
        let base_char = if cell.is_complex() {
            self.grid.resolved_char(row, col).unwrap_or('\u{FFFD}')
        } else {
            cell.char()
        };

        if !is_emoji_modifier_base(base_char) {
            return false;
        }

        // Combine the skin tone modifier as a combining character on the base.
        self.add_combining_to_previous_cell(modifier);
        true
    }

    /// Combine the SECOND regional indicator of a flag pair into the first RI's
    /// cell, so `🇺🇸` is one 2-cell grapheme (the colour font has a bitmap for
    /// the pair, not single RIs). Returns `false` unless the previous cell is a
    /// LONE regional indicator — one still waiting for its partner — so RIs pair
    /// left to right and a third RI starts a fresh pair (Unicode GB12/GB13).
    fn try_combine_regional_indicator(&mut self, ri: char) -> bool {
        let Some((row, col)) = self.previous_effective_cell() else {
            return false;
        };
        let Some(cell) = self.grid.cell(row, col) else {
            return false;
        };
        let base = if cell.is_complex() {
            self.grid.resolved_char(row, col).unwrap_or('\u{FFFD}')
        } else {
            cell.char()
        };
        if !is_regional_indicator(base) {
            return false;
        }
        // Already a complete pair (its combining store holds an RI)? Then this
        // RI begins a NEW pair in its own cell rather than extending the old one.
        let already_paired = self.grid.cell_extra(row, col).is_some_and(|e| {
            e.combining()
                .iter()
                .copied()
                .any(is_regional_indicator)
        });
        if already_paired {
            return false;
        }
        self.add_combining_to_previous_cell(ri);
        true
    }
}

/// Check if a character is a Unicode REGIONAL INDICATOR (U+1F1E6–U+1F1FF).
///
/// A pair of these forms one flag emoji (`🇺` + `🇸` = `🇺🇸`); the write path
/// folds the second into the first cell so the pair is one grapheme. Mirrors
/// `aterm_grapheme::is_regional_indicator` (test-only-exported there) as a small
/// local check to avoid widening that crate's public API.
#[inline]
fn is_regional_indicator(c: char) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&(c as u32))
}

/// Check if a character is an emoji skin tone modifier (Fitzpatrick scale).
///
/// U+1F3FB (Type-1-2) through U+1F3FF (Type-6) are the five skin tone
/// modifiers defined in Unicode Technical Standard #51.
#[inline]
fn is_emoji_skin_tone_modifier(c: char) -> bool {
    (0x1F3FB..=0x1F3FF).contains(&(c as u32))
}

/// Check if a character is an emoji modifier base — i.e., can accept a
/// skin tone modifier (U+1F3FB-U+1F3FF).
///
/// This covers the common subset of `Emoji_Modifier_Base` from Unicode
/// emoji-data.txt: people, hand gestures, body parts, and person activities.
#[inline]
fn is_emoji_modifier_base(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        // People and body parts (U+261D, U+26F9)
        0x261D | 0x26F9
        // Dingbats: raised hand, victory hand, writing hand
        | 0x270A..=0x270D
        // Miscellaneous Symbols and Pictographs
        | 0x1F385 // Santa Claus
        | 0x1F3C2..=0x1F3C4 // Snowboarder, Horse Racing, Surfer
        | 0x1F3C7 // Horse Racing
        | 0x1F3CA..=0x1F3CC // Swimmer, Lifter, Golfer
        // People emoji block (U+1F442-U+1F4AA)
        | 0x1F442..=0x1F443 // Ear, Nose
        | 0x1F446..=0x1F450 // Pointing up through open hands
        | 0x1F466..=0x1F478 // Boy through Princess
        | 0x1F47C // Baby Angel
        | 0x1F481..=0x1F483 // Information Desk Person through Dancer
        | 0x1F485..=0x1F487 // Nail Polish through Haircut
        | 0x1F48F // Kiss
        | 0x1F491 // Couple with Heart
        | 0x1F4AA // Flexed Biceps
        // Additional people and activities
        | 0x1F574..=0x1F575 // Man in Business Suit Levitating, Sleuth
        | 0x1F57A // Man Dancing
        | 0x1F590 // Raised Hand with Fingers Splayed
        | 0x1F595..=0x1F596 // Reversed Hand with Middle Finger, Vulcan Salute
        | 0x1F645..=0x1F647 // Face No Good through Person Bowing
        | 0x1F64B..=0x1F64F // Happy Person Raising Hand through Person Praying
        // Supplemental Symbols and Pictographs
        | 0x1F6A3 // Rowboat
        | 0x1F6B4..=0x1F6B6 // Bicyclist through Pedestrian
        | 0x1F6C0 // Bath
        | 0x1F6CC // Sleeping Accommodation
        // Hand gestures and people (Supplemental Symbols block)
        | 0x1F90C..=0x1F90F // Pinched Fingers through Pinching Hand + extras
        | 0x1F918..=0x1F91F // Sign of the Horns through I Love You Gesture
        | 0x1F926 // Face Palm
        | 0x1F930..=0x1F939 // Pregnant Woman through Juggling
        | 0x1F93C..=0x1F93E // Wrestlers through Handball
        | 0x1F9B5..=0x1F9B6 // Leg, Foot
        | 0x1F9B8..=0x1F9B9 // Superhero, Supervillain
        | 0x1F9BB // Ear with Hearing Aid
        | 0x1F9CD..=0x1F9CF // Standing Person through Deaf Person
        | 0x1F9D1..=0x1F9DD // Adult through Elf
        | 0x1FAC3..=0x1FAC5 // Pregnant Man, Pregnant Person, Person with Crown
        | 0x1FAF0..=0x1FAF8 // Hand with Index Finger and Thumb Crossed through Rightwards Pushing Hand
    )
}

/// Check if a character is eligible for VS16 emoji presentation widening.
///
/// Returns `true` for characters that have the Unicode Emoji property and
/// default to text presentation (width 1), meaning VS16 should widen them
/// to 2 cells. This covers the common subset defined in Unicode Emoji data.
///
/// Reference: Unicode Technical Standard #51 (emoji-data.txt, Emoji property).
#[inline]
pub(crate) fn is_vs16_emoji_capable(c: char) -> bool {
    let cp = c as u32;
    match cp {
        // Number sign, asterisk, digits 0-9 (keycap base characters)
        0x0023 | 0x002A | 0x0030..=0x0039 => true,
        // Copyright, Registered
        0x00A9 | 0x00AE => true,
        // Letterlike Symbols
        0x2122 => true, // Trade Mark
        0x2139 => true, // Information Source
        // Arrows — select emoji
        0x2194..=0x2199 => true, // Directional arrows
        0x21A9 | 0x21AA => true, // Curved arrows
        // Miscellaneous Technical — select emoji
        0x231A | 0x231B => true, // Watch, Hourglass
        0x2328 => true,          // Keyboard
        0x23CF => true,          // Eject
        0x23E9..=0x23F3 => true, // Various media controls
        0x23F8..=0x23FA => true, // Pause, Record, etc.
        // Double Exclamation Mark, Exclamation Question Mark
        0x203C | 0x2049 => true,
        // Enclosed Alphanumerics
        0x24C2 => true, // Circled M
        // Geometric Shapes
        0x25AA | 0x25AB => true, // Small squares
        0x25B6 | 0x25C0 => true, // Play buttons
        0x25FB..=0x25FE => true, // Medium squares
        // Miscellaneous Symbols (U+2600-U+26FF) — large emoji block
        0x2600..=0x26FF => true,
        // Dingbats (U+2700-U+27BF) — many emoji-capable chars
        0x2700..=0x27BF => true,
        // Supplemental Arrows-B
        0x2934 | 0x2935 => true, // Curved arrows
        // CJK Symbols
        0x3030 => true, // Wavy Dash
        0x303D => true, // Part Alternation Mark
        // Enclosed CJK Letters
        0x3297 => true, // Circled Ideograph Congratulation
        0x3299 => true, // Circled Ideograph Secret
        // Any SMP char that got width 1 from our lookup (rare edge case)
        0x1F000..=0x1FFFF => true,
        _ => false,
    }
}
