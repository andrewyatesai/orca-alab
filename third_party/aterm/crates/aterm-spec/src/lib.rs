// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::all)]

//! TLA+ specification coverage tracking for aterm formal verification.
//!
//! Provides tools to cross-reference Rust Kani proofs against TLA+ specifications,
//! tracking which TLA+ actions have corresponding Rust refinements.
//!
//! # Derived, drift-free TLA+ ([`derive`] + [`ty_model`])
//!
//! Prefer DERIVED models over hand-written `.tla`: write a bounded state machine
//! once as a Rust [`derive::Model`] (or via the [`ty_model`] macro) and the
//! `ty`-checkable spec AND the executable interpreter are generated from that one
//! source, so they cannot drift from each other or from the code. See
//! `docs/RFC-ty-embed-derived-tla.md` and the `AGENTS.md` at the repo root for the
//! workflow (Tier-0 `ty check` of the derived spec; Tier-1 conformance binding the
//! model to the real code). Existing models: [`derive::ring_model`],
//! [`derive::kernel_model`], [`derive::subscribe_model`], [`derive::snapshot_model`],
//! [`derive::transact_model`], [`derive::cursor_model`].

pub use aterm_spec_macros::{refines, spec_unmodeled, ty_model};

pub mod coverage;
pub mod derive;
pub mod tla_check;

/// Refinement mapping trait.
///
/// A concrete implementation type `C` that `impl Refines<A>` can project
/// its state to the abstract TLA+ model type `A`. This projection is the
/// Rust analogue of a TLA+ refinement mapping: every reachable concrete
/// state maps to a valid abstract state.
///
/// The trait is intentionally minimal — it exists to document and enforce
/// the correspondence, not to drive runtime behavior.
pub trait Refines<Abstract> {
    /// Project the concrete state to the abstract model.
    fn project(&self) -> Abstract;
}
