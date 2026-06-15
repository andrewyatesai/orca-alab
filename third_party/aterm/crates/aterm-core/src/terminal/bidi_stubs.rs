// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! No-op stubs for BiDi methods when the `bidi` feature is disabled.

use super::Terminal;

#[allow(dead_code, reason = "stub methods for disabled bidi feature")]
#[allow(
    clippy::unused_self,
    reason = "stubs mirror the &mut self signatures of the real bidi methods so call sites are identical whether or not the feature is enabled"
)]
impl Terminal {
    /// No-op: BiDi feature is disabled.
    pub(crate) fn invalidate_bidi_all(&mut self) {}

    /// No-op: BiDi feature is disabled.
    pub(super) fn sync_bidi_resolver_from_config(&mut self) {}

    /// No-op: BiDi feature is disabled.
    pub(super) fn sync_bidi_from_damage(&mut self) {}
}
