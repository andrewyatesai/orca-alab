// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Base64 and hex encoding/decoding for aterm.
//!
//! Zero external dependencies. Provides:
//!
//! - [`base64`] — standard and URL-safe Base64 with optional padding.
//! - [`hex`] — hexadecimal encoding and decoding.
//!
//! ## Usage
//!
//! ```rust
//! use aterm_codec::{base64, hex};
//!
//! // Base64
//! let encoded = base64::encode(b"Hello, world!");
//! assert_eq!(encoded, "SGVsbG8sIHdvcmxkIQ==");
//! let decoded = base64::decode(&encoded).unwrap();
//! assert_eq!(decoded, b"Hello, world!");
//!
//! // URL-safe Base64 (no padding)
//! let encoded = base64::encode_url_safe_no_pad(b"Hello, world!");
//! let decoded = base64::decode_url_safe_no_pad(&encoded).unwrap();
//! assert_eq!(decoded, b"Hello, world!");
//!
//! // Hex
//! let encoded = hex::encode(b"\xde\xad\xbe\xef");
//! assert_eq!(encoded, "deadbeef");
//! let decoded = hex::decode(&encoded).unwrap();
//! assert_eq!(decoded, b"\xde\xad\xbe\xef");
//! ```

#![deny(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod base64;
pub mod hex;
