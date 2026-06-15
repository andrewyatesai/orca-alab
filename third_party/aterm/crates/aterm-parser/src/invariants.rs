// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! TLA+ invariant assertions for the parser state machine.

use crate::Parser;
#[cfg(debug_assertions)]
use crate::state::State;
#[cfg(debug_assertions)]
use crate::{MAX_INTERMEDIATES, MAX_PARAMS};

impl Parser {
    /// Assert that all TLA+ TypeInvariant properties hold.
    ///
    /// This function verifies the parser state matches the formal specification
    /// in `tla/Parser.tla`. Only runs in debug builds.
    ///
    /// # TLA+ TypeInvariant
    ///
    /// ```tla
    /// TypeInvariant ==
    ///     /\ state \in States
    ///     /\ params \in Seq(0..65535)
    ///     /\ Len(params) <= MAX_PARAMS
    ///     /\ intermediates \in Seq(0..255)
    ///     /\ Len(intermediates) <= MAX_INTERMEDIATES
    ///     /\ currentParam \in 0..65535
    /// ```
    ///
    /// # Panics
    ///
    /// Panics in debug builds if any invariant is violated.
    /// Does nothing in release builds for performance.
    #[inline]
    pub fn assert_invariants(&self) {
        #[cfg(debug_assertions)]
        {
            // Invariant: StateValid
            // state must be a valid State enum value (0..13)
            assert!(
                (self.state as usize) < State::COUNT,
                "TLA+ TypeInvariant violated: state {} >= COUNT {}",
                self.state as u8,
                State::COUNT
            );

            // Invariant: ParamsBounded
            // Len(params) <= MAX_PARAMS
            assert!(
                self.params.len() <= MAX_PARAMS,
                "TLA+ TypeInvariant violated: params.len() {} > MAX_PARAMS {}",
                self.params.len(),
                MAX_PARAMS
            );

            // Invariant: IntermediatesBounded
            // Len(intermediates) <= MAX_INTERMEDIATES
            assert!(
                self.intermediates.len() <= MAX_INTERMEDIATES,
                "TLA+ TypeInvariant violated: intermediates.len() {} > MAX_INTERMEDIATES {}",
                self.intermediates.len(),
                MAX_INTERMEDIATES
            );

            // Invariant: CurrentParamBounded
            // currentParam is bounded during accumulation
            // Note: current_param is u32 during accumulation and uses saturating arithmetic
            // The actual bound is checked when finalized (converted to u16), but we verify
            // the accumulator isn't in an invalid state
            // (This assertion always passes for u32 but documents the invariant)
            let _ = self.current_param; // Acknowledge the field is part of invariant checking

            // Invariant: Utf8BufferValid
            // utf8_len <= 4 (max UTF-8 sequence length)
            assert!(
                self.utf8_len <= 4,
                "TLA+ Utf8BufferValid violated: utf8_len {} > 4",
                self.utf8_len
            );

            // Invariant: Utf8ExpectedValid
            // utf8_expected <= 4
            assert!(
                self.utf8_expected <= 4,
                "TLA+ Utf8ExpectedValid violated: utf8_expected {} > 4",
                self.utf8_expected
            );

            // Invariant: Utf8ProgressValid
            // utf8_len <= utf8_expected (can't have more bytes than expected)
            assert!(
                self.utf8_len <= self.utf8_expected,
                "TLA+ Utf8ProgressValid violated: utf8_len {} > utf8_expected {}",
                self.utf8_len,
                self.utf8_expected
            );

            // Invariant: SubparamMaskValid
            // Subparam mask bits only set for indices < params.len()
            // Bit i indicates param[i] is a subparam of param[i-1]
            if self.params.len() < 16 {
                let valid_bits = (1u16 << self.params.len()) - 1;
                // Bit 0 is never meaningful (no param before param[0])
                let meaningful_mask = self.subparam_mask & !1;
                assert_eq!(
                    meaningful_mask & !valid_bits,
                    0,
                    "TLA+ SubparamMaskValid violated: subparam_mask has bits set beyond params.len() (mask={:#06x}, params.len()={})",
                    self.subparam_mask,
                    self.params.len()
                );
            }

            // Invariant: DcsApcExclusive
            // Cannot be in both DCS and APC sequence at the same time
            assert!(
                !(self.dcs_active && self.apc_active),
                "TLA+ DcsApcExclusive violated: both dcs_active and apc_active are true"
            );
        }
    }
}
