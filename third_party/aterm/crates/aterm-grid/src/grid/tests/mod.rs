// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Grid tests — migrated from aterm-core as part of #6556 Batch 1.

use super::*;

mod algorithm_boundary;
mod basic;
mod boundary;
mod deferred_line_equivalence;
mod extras_performance;
mod hyperlink_perf;
mod reflow;
mod scroll_damage;
mod scroll_region;
mod scrollback;
mod scrollback_grapheme_edge;
mod scrollback_materialize;
mod scrollback_materialize_len;
mod scrollback_style_roundtrip;
mod stale_extras_overwrite;
mod style_perf;
mod style_perf_complexity;
mod tab_stops;
