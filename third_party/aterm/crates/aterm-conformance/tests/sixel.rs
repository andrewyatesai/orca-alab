// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// Sixel graphics — behavioral locks for what the engine ACTUALLY implements.
//
// HONEST SUPPORT STATEMENT (verified against source, 2026-06):
// aterm's sixel support is CONSUME-ONLY. The DCS dispatcher recognizes
// `DCS Ps q <data> ST` (handler_dcs.rs), but the decode pipeline is gated
// behind `#[cfg(feature = "sixel")]` and that feature is NOT declared in
// aterm-core's Cargo.toml (nor is the `aterm-sixel` decoder crate present in
// the workspace), so the decoder is permanently compiled out. A sixel DCS
// payload is therefore: byte-counted against the global DCS memory budget
// (10 MiB, callbacks/mod.rs), optionally surfaced to a host DCS callback,
// and otherwise DROPPED. No image is decoded, stored, or observable through
// any public Terminal API; no grid cells change; the cursor does not move.
//
// What IS live, and what this file locks:
//   * Graceful consumption: payloads (valid, truncated, garbage, huge DECGRI
//     repeats) never leak onto the grid, never panic, never corrupt parser
//     state. (DEC STD 070 / VT510: unrecognized device control strings are
//     consumed and ignored.)
//   * DECSDM (DEC private mode 80) set/reset tracking, observable via DECRQM
//     (VT510: CSI ? Ps $ p -> CSI ? Ps ; Pm $ y), and its reset on DECSTR
//     (xterm behavior, engine #7496).
//   * XTSMGRAPHICS (xterm ctlseqs: CSI ? Pi ; Pa ; Pv S), read-only, with
//     the engine's documented fallback limits: 1024 color registers and
//     4096x4096 max sixel geometry (handler_xtsmgraphics.rs).
//
// DOCUMENTED LIMITATION (spec expectation, not implemented): per the VT330/
// VT340 Programmer Reference (EK-VT3XX-TP) and xterm, after a sixel image in
// sixel *scrolling* mode the text cursor moves to the line following the
// image; in sixel *display* mode (DECSDM set) the image is painted from the
// home position and the cursor does not move. aterm renders no image, so the
// cursor never moves in either DECSDM state. The cursor tests below lock the
// current (unmoved) behavior in BOTH states; they are behavior locks, not
// spec-conformance claims.

use aterm_conformance::{Screen, run};

/// A well-formed minimal 4x6 two-color sixel image:
/// raster attrs 1;1;4;6, color 0 = black, color 1 = red (RGB% space),
/// select color 1, four full sixel columns, graphics-CR, graphics-NL.
const SIXEL_4X6: &[u8] = b"\x1bP0;0;8q\"1;1;4;6#0;2;0;0;0#1;2;100;0;0#1~~~~$-\x1b\\";

// --- A. graceful consumption ------------------------------------------------

#[test]
fn sixel_dcs_consumed_then_text_renders_normally() {
    // The must-have: a valid sixel DCS is swallowed whole. No payload byte
    // ('~', '#', '"', digits...) may leak onto the grid, the screen stays
    // empty, no response is generated, and following text prints at the
    // cursor as if the DCS never happened.
    let mut s = Screen::new(24, 80);
    s.feed(SIXEL_4X6);
    assert_eq!(s.screen(), "", "sixel payload bytes leaked onto the grid");
    assert_eq!(s.take_response(), None, "sixel DCS must not generate a reply");
    s.feed(b"AB");
    assert_eq!(s.row(0), "AB");
    assert_eq!(s.cursor(), (0, 2));
}

#[test]
fn sixel_dcs_leaves_parser_state_intact_for_subsequent_csi() {
    // Parser must return to Ground after ST: a following CUP and CPR (DSR 6)
    // must work exactly as on a fresh terminal (VT510 CPR: CSI 6 n ->
    // CSI Pr ; Pc R, 1-indexed).
    let mut s = Screen::new(24, 80);
    s.feed(SIXEL_4X6);
    s.feed(b"\x1b[5;10H\x1b[6n");
    assert_eq!(s.cursor(), (4, 9));
    assert_eq!(s.response_string(), "\x1b[5;10R");
}

#[test]
fn sixel_dcs_split_across_feeds_is_still_consumed() {
    // Chunk-boundary safety: the same payload split mid-sequence across
    // process() calls must behave identically to a single feed.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;4;6");
    s.feed(b"#0;2;0;0;0#1;2;100;0;0");
    s.feed(b"#1~~");
    s.feed(b"~~$-");
    s.feed(b"\x1b\\");
    s.feed(b"OK");
    assert_eq!(s.screen(), "OK");
    assert_eq!(s.cursor(), (0, 2));
}

#[test]
fn truncated_sixel_consumes_following_text_until_escape_breaks_out() {
    // Missing ST: per the VT500 parser model the terminal stays in
    // DCS-passthrough, so following printable text is payload, NOT display
    // text. The next ESC (here: starting CSI 6 n) terminates the string via
    // the "anywhere" ESC transition and the CSI executes normally — this is
    // exactly how a real `ESC \` (ESC then `\`) terminates DCS anyway.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;4;6#1~~"); // no ST
    s.feed(b"XYZ"); // still inside the DCS: must NOT print
    assert_eq!(s.screen(), "", "text after unterminated DCS leaked to grid");
    s.feed(b"\x1b[6n"); // ESC breaks out, CSI runs
    assert_eq!(s.response_string(), "\x1b[1;1R");
    s.feed(b"OK");
    assert_eq!(s.row(0), "OK");
}

#[test]
fn garbage_inside_sixel_payload_is_contained() {
    // C0 controls (BEL, TAB, LF, CR), 8-bit bytes, and non-sixel characters
    // inside the payload are all DCS data per the parser tables
    // (aterm-parser table/dcs_osc.rs): nothing executes, nothing prints,
    // and the terminal recovers at ST.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;4;6#1~");
    s.feed(&[0x07, 0x09, 0x0a, 0x0d, 0x80, 0xfe, b'(', b'%']);
    s.feed(b"~~\x1b\\");
    assert_eq!(s.screen(), "");
    assert_eq!(s.cursor(), (0, 0), "C0 bytes inside DCS must not move cursor");
    s.feed(b"OK");
    assert_eq!(s.row(0), "OK");
}

#[test]
fn can_aborts_sixel_dcs_and_returns_to_ground() {
    // CAN (0x18) cancels a control string from any state (VT500 "anywhere"
    // transition); subsequent text must print normally.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;4;6#1~~");
    s.feed(&[0x18]);
    s.feed(b"OK");
    assert_eq!(s.row(0), "OK");
    assert_eq!(s.cursor(), (0, 2));
}

#[test]
fn zero_size_sixel_image_is_harmless() {
    // Raster attributes declaring a 0x0 image with no data.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;0;0\x1b\\");
    assert_eq!(s.screen(), "");
    assert_eq!(s.take_response(), None);
    s.feed(b"OK");
    assert_eq!(s.row(0), "OK");
}

#[test]
fn enormous_decgri_repeat_count_no_panic_no_oom() {
    // DECGRI: `! Pn <char>` repeats the sixel <char> Pn times. A hostile
    // Pn (here u32::MAX, plus several more) must not OOM or panic. In the
    // current consume-only engine these are inert payload bytes; if a
    // decoder ever lands, the 10 MiB global DCS budget (callbacks/mod.rs
    // MAX_DCS_GLOBAL_BUDGET) and decoder dimension caps are the backstop.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;4;6#1");
    s.feed(b"!4294967295~!999999999~!0~!123456789012345678901234567890~");
    s.feed(b"\x1b\\OK");
    assert_eq!(s.row(0), "OK");
}

#[test]
fn payload_larger_than_global_dcs_budget_is_dropped_without_panic() {
    // 11 MiB of sixel data exceeds MAX_DCS_GLOBAL_BUDGET (10 MiB): the
    // engine must keep consuming (dropping) bytes and recover cleanly at ST.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1bP0;0;8q\"1;1;4;6#1");
    let chunk = vec![b'~'; 1 << 20]; // 1 MiB of full sixel columns
    for _ in 0..11 {
        s.feed(&chunk);
    }
    s.feed(b"\x1b\\OK");
    assert_eq!(s.row(0), "OK");
    assert_eq!(s.cursor(), (0, 2));
}

// --- B. DECSDM (DEC private mode 80) -----------------------------------------

#[test]
fn decsdm_mode80_set_reset_roundtrip_via_decrqm() {
    // VT510 DECRQM: CSI ? 80 $ p -> CSI ? 80 ; Pm $ y (1=set, 2=reset).
    // Power-on default is reset; CSI ? 80 h sets, CSI ? 80 l resets.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1b[?80$p");
    assert_eq!(s.response_string(), "\x1b[?80;2$y", "DECSDM default must be reset");
    s.feed(b"\x1b[?80h\x1b[?80$p");
    assert_eq!(s.response_string(), "\x1b[?80;1$y");
    s.feed(b"\x1b[?80l\x1b[?80$p");
    assert_eq!(s.response_string(), "\x1b[?80;2$y");
}

#[test]
fn decsdm_is_reset_by_decstr_soft_reset() {
    // Engine behavior (handler_report.rs, #7496), matching xterm: DECSTR
    // (CSI ! p) resets sixel display mode.
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1b[?80h\x1b[!p\x1b[?80$p");
    assert_eq!(s.response_string(), "\x1b[?80;2$y");
}

// --- C. XTSMGRAPHICS (CSI ? Pi ; Pa ; Pv S) ----------------------------------
// xterm ctlseqs: response is CSI ? Pi ; Ps ; Pv S with Ps: 0=success,
// 1=error in Pi, 2=error in Pa, 3=failure. The engine is read-only and
// reports its documented limits (handler_xtsmgraphics.rs): 1024 color
// registers, 4096 max sixel dimension.

#[test]
fn xtsmgraphics_color_registers_read_and_read_max() {
    // Pi=1 Pa=1 (read) and Pa=4 (read max) both report 1024.
    assert_eq!(run(b"\x1b[?1;1S").response_string(), "\x1b[?1;0;1024S");
    assert_eq!(run(b"\x1b[?1;4S").response_string(), "\x1b[?1;0;1024S");
}

#[test]
fn xtsmgraphics_sixel_geometry_read_falls_back_to_max_without_window_callback() {
    // Pi=2 Pa=1 reads the current text-area pixel size via the host window
    // callback (#7470); the conformance harness registers none, so the
    // engine's documented fallback is the max dimension on both axes.
    assert_eq!(run(b"\x1b[?2;1S").response_string(), "\x1b[?2;0;4096;4096S");
}

#[test]
fn xtsmgraphics_sixel_geometry_read_max() {
    // Pi=2 Pa=4: maximum sixel geometry, width;height.
    assert_eq!(run(b"\x1b[?2;4S").response_string(), "\x1b[?2;0;4096;4096S");
}

#[test]
fn xtsmgraphics_regis_geometry_reports_failure() {
    // Pi=3 (ReGIS) is unsupported: status 3 (failure).
    assert_eq!(run(b"\x1b[?3;1S").response_string(), "\x1b[?3;3;0S");
}

#[test]
fn xtsmgraphics_set_and_reset_rejected_read_only() {
    // Pa=3 (set) and Pa=2 (reset) are rejected with status 3: the engine's
    // graphics limits are read-only.
    assert_eq!(run(b"\x1b[?1;3;99S").response_string(), "\x1b[?1;3;0S");
    assert_eq!(run(b"\x1b[?2;2S").response_string(), "\x1b[?2;3;0S");
}

#[test]
fn xtsmgraphics_invalid_item_and_action_report_errors() {
    // Unknown Pi -> status 1 (error in Pi); unknown/omitted Pa -> status 2
    // (error in Pa). The raw Pi is echoed back in both cases.
    assert_eq!(run(b"\x1b[?9;1S").response_string(), "\x1b[?9;1;0S");
    assert_eq!(run(b"\x1b[?0;1S").response_string(), "\x1b[?0;1;0S");
    assert_eq!(run(b"\x1b[?1;9S").response_string(), "\x1b[?1;2;0S");
    assert_eq!(run(b"\x1b[?1S").response_string(), "\x1b[?1;2;0S");
}

// --- D. cursor position after sixel, both DECSDM states ----------------------
// BEHAVIOR LOCK, not spec conformance — see the header. Spec expectation
// (VT340 / xterm): scrolling mode (DECSDM reset in xterm >= 369 semantics)
// leaves the cursor on the line after the image; display mode (DECSDM set)
// paints from home and leaves the cursor unmoved. aterm displays nothing and
// never moves the cursor. If image display ever lands, these two tests MUST
// be revisited.

#[test]
fn cursor_unmoved_after_sixel_with_decsdm_reset() {
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1b[?80l\x1b[5;1H");
    s.feed(SIXEL_4X6);
    assert_eq!(s.cursor(), (4, 0), "consume-only engine: cursor must not move");
}

#[test]
fn cursor_unmoved_after_sixel_with_decsdm_set() {
    let mut s = Screen::new(24, 80);
    s.feed(b"\x1b[?80h\x1b[5;1H");
    s.feed(SIXEL_4X6);
    assert_eq!(s.cursor(), (4, 0), "consume-only engine: cursor must not move");
}

// --- E. image observability ---------------------------------------------------
// No decode-correctness test is possible or appropriate: the sixel decoder is
// compiled out (no `sixel` feature, no aterm-sixel crate in the workspace),
// so there is no stored image state to observe and no accessor was added —
// adding one would expose permanently-empty state. If a decoder lands, add
// dimension + pixel-color assertions for SIXEL_4X6 here (4x6, color 1 = red).
