// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Batch path tests: advance_batch parity with advance, UTF-8 handling,
//! sequence dispatch, cancel/interrupt, and partial sequence completion.

use super::super::*;
use super::RecordingSink;

#[test]
fn advance_batch_handles_utf8() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    parser.advance_batch("Hello €".as_bytes(), &mut sink);

    assert_eq!(sink.prints, vec!['H', 'e', 'l', 'l', 'o', ' ', '€']);
}

#[test]
fn advance_batch_invalid_utf8_then_escape_replays_escape() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // 0xE2 starts a 3-byte sequence. ESC interrupts it, so the broken UTF-8
    // must emit U+FFFD and then replay ESC normally.
    parser.advance_batch(&[0xE2, 0x1B, b'7'], &mut sink);

    assert_eq!(
        sink.prints,
        vec![char::REPLACEMENT_CHARACTER],
        "broken UTF-8 lead should emit a single replacement char"
    );
    assert_eq!(
        sink.esc_dispatches,
        vec![(Vec::new(), b'7')],
        "ESC after broken UTF-8 must still dispatch normally"
    );
    assert_eq!(parser.state(), State::Ground);
}

#[test]
fn advance_batch_c1_disabled_prints_replacement() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    parser.advance_batch(&[0x9B], &mut sink);

    assert_eq!(sink.prints, vec![char::REPLACEMENT_CHARACTER]);
    assert_eq!(parser.state(), State::Ground);
}

#[test]
fn advance_batch_osc_dispatch() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // OSC 0 (set title) via BEL terminator through batch path
    parser.advance_batch(b"\x1b]0;My Title\x07", &mut sink);

    assert_eq!(sink.osc_dispatches.len(), 1, "batch path must dispatch OSC");
    assert_eq!(
        sink.osc_dispatches[0],
        vec![b"0".to_vec(), b"My Title".to_vec()],
        "batch path must parse OSC params correctly"
    );
}

#[test]
fn advance_batch_csi_dispatch() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // CSI 1;31;42m through batch path
    parser.advance_batch(b"\x1b[1;31;42m", &mut sink);

    assert_eq!(sink.csi_dispatches.len(), 1, "batch path must dispatch CSI");
    assert_eq!(
        sink.csi_dispatches[0],
        (vec![1, 31, 42], vec![], b'm'),
        "batch path CSI dispatch must match advance() semantics"
    );
}

#[test]
fn advance_batch_dcs_roundtrip() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // DCS 0q (Sixel) with data, terminated by ST
    parser.advance_batch(b"\x1bP0q#0!10~\x1b\\", &mut sink);

    assert_eq!(sink.dcs_hooks.len(), 1, "batch path must hook DCS");
    assert_eq!(sink.dcs_hooks[0].2, b'q', "DCS final byte must be 'q'");
    assert!(
        !sink.dcs_puts.is_empty(),
        "batch path must forward DCS put data"
    );
    assert_eq!(sink.dcs_unhooks, 1, "batch path must unhook DCS");
}

#[test]
fn advance_batch_matches_advance() {
    // Side-by-side parity test: advance_batch must produce identical dispatches
    // to advance() for all sequence types. This is the batch-path equivalent of
    // csi_fast_path_matches_basic_parser.
    let test_cases: &[&[u8]] = &[
        // CSI sequences
        b"\x1b[31m",
        b"\x1b[1;31m",
        b"\x1b[38;5;196m",
        b"\x1b[38;2;255;128;64m",
        b"\x1b[?1049h",
        b"\x1b[10;20H",
        b"\x1b[H",
        b"\x1b[0m",
        // OSC sequences (BEL terminator)
        b"\x1b]0;My Title\x07",
        b"\x1b]52;c;dGVzdA==\x07",
        // OSC sequences (ST terminator)
        b"\x1b]0;Title\x1b\\",
        // ESC sequences
        b"\x1b7",
        b"\x1b(B",
        // DCS sequences
        b"\x1bPq$s\x1b\\",
        b"\x1bP0qABC\x1b\\",
        // Plain text
        b"Hello, World!",
        // Mixed text + sequences
        b"Hello\x1b[31mWorld\x1b[0m!",
        // Control characters
        b"\n\r\t",
    ];

    for input in test_cases {
        let mut parser_advance = Parser::new();
        let mut sink_advance = RecordingSink::default();
        parser_advance.advance(input, &mut sink_advance);

        let mut parser_batch = Parser::new();
        let mut sink_batch = RecordingSink::default();
        parser_batch.advance_batch(input, &mut sink_batch);

        assert_eq!(
            sink_advance.prints, sink_batch.prints,
            "prints mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.executes, sink_batch.executes,
            "executes mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.csi_dispatches, sink_batch.csi_dispatches,
            "csi_dispatches mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.esc_dispatches, sink_batch.esc_dispatches,
            "esc_dispatches mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.osc_dispatches, sink_batch.osc_dispatches,
            "osc_dispatches mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.dcs_hooks, sink_batch.dcs_hooks,
            "dcs_hooks mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.dcs_puts, sink_batch.dcs_puts,
            "dcs_puts mismatch for input: {:?}",
            input
        );
        assert_eq!(
            sink_advance.dcs_unhooks, sink_batch.dcs_unhooks,
            "dcs_unhooks mismatch for input: {:?}",
            input
        );
    }
}

#[test]
fn advance_batch_cancel_aborts_sequence() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // Start CSI sequence, then CAN (0x18) aborts it — same as cancel_aborts_sequence
    // but through the batch path
    parser.advance_batch(b"\x1b[31\x18Hello", &mut sink);

    assert!(sink.executes.contains(&0x18), "batch path must execute CAN");
    assert_eq!(
        sink.csi_dispatches.len(),
        0,
        "batch path CAN must abort CSI"
    );
    assert_eq!(
        sink.prints.len(),
        5,
        "batch path must print 'Hello' after CAN"
    );
}

#[test]
fn advance_batch_esc_interrupts_sequence() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // Start CSI sequence, then ESC starts new sequence — same as esc_interrupts_sequence
    parser.advance_batch(b"\x1b[31\x1b[32m", &mut sink);

    assert_eq!(
        sink.csi_dispatches.len(),
        1,
        "batch path: only second CSI should complete"
    );
    assert_eq!(
        sink.csi_dispatches[0].0,
        vec![32],
        "batch path: completed CSI should have param 32"
    );
}

#[test]
fn advance_batch_colon_subparams() {
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    // SGR 4:3 (curly underline) through batch path — subparam handling parity
    parser.advance_batch(b"\x1b[4:3m", &mut sink);

    assert_eq!(
        sink.csi_dispatches.len(),
        0,
        "batch path: colons should trigger subparam dispatch"
    );
    assert_eq!(
        sink.csi_dispatches_with_subparams.len(),
        1,
        "batch path: should dispatch with subparams"
    );
    let (params, _, final_byte, subparam_mask) = &sink.csi_dispatches_with_subparams[0];
    assert_eq!(params, &vec![4, 3], "batch path subparam values");
    assert_eq!(*final_byte, b'm', "batch path subparam final byte");
    assert_eq!(*subparam_mask, 0b10, "batch path subparam mask");
}

#[test]
fn advance_batch_partial_then_complete() {
    // Verify batch path handles partial sequences across multiple calls
    let mut parser = Parser::new();
    let mut sink = RecordingSink::default();

    parser.advance_batch(b"\x1b[1;2", &mut sink);
    assert!(
        sink.csi_dispatches.is_empty(),
        "batch path: partial CSI should not dispatch"
    );

    parser.advance_batch(b"m", &mut sink);
    assert_eq!(
        sink.csi_dispatches.len(),
        1,
        "batch path: completed CSI should dispatch"
    );
    assert_eq!(
        sink.csi_dispatches[0].0,
        vec![1, 2],
        "batch path: params should be [1, 2]"
    );
}
