// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! TLA+ models of aterm's load-bearing invariants (ROADMAP WS-I).
//!
//! The specs live in `specs/*.tla` (+ `.cfg`). They are model-checked by the
//! `ty` explicit-state checker from `tests/model_check.rs`, so model-checking
//! runs under `cargo test` and the CI gate — not as a separate manual step.
//!
//! Families (ATERM_DESIGN §6.4):
//! - CONSISTENCY — the buffer kernel (event-log monotonicity, snapshot/branch).
//! - ISOLATION — the capability/sandbox boundary.
//! - META — refinement / coverage.
