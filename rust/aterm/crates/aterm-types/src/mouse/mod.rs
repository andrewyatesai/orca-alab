// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared mouse encoding types and functions.
//!
//! This module provides the canonical mouse encoding contract used by both
//! `aterm-core` and `aterm-alacritty-bridge`. Encoding types, button definitions,
//! modifier constants, and pure encoding functions live here so neither crate
//! needs to duplicate mouse byte-encoding logic.

mod encode;
mod types;

pub use encode::{encode_mouse, encode_sgr, encode_urxvt, encode_utf8, encode_x10};
pub use types::{ALT_MASK, CTRL_MASK, MouseButton, MouseEncoding, MouseMode, SHIFT_MASK};
