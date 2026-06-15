// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared compression helpers and search utilities for scrollback tiers.

use super::ScrollbackError;

/// Default lines per compressed block.
pub(crate) const DEFAULT_BLOCK_SIZE: usize = 100;

/// Default hot tier limit (lines).
pub(crate) const DEFAULT_HOT_LIMIT: usize = 1000;

/// Default warm tier limit (lines).
pub(crate) const DEFAULT_WARM_LIMIT: usize = 10000;

/// Default memory budget (100 MB).
pub(crate) const DEFAULT_MEMORY_BUDGET: usize = 100 * 1024 * 1024;

/// Default line limit (lines).
///
/// Caps total scrollback to prevent runaway memory growth (#7929 / HN F09-1).
/// A runaway process writing to stdout would otherwise grow scrollback without
/// bound until `memory_budget` is exhausted (and for disk-backed storage, fill
/// the disk). 100,000 lines is a pragmatic default: generous for typical
/// interactive sessions, but bounded for attacker workloads.
///
/// Hosts that need unbounded history can opt in via
/// `Scrollback::set_line_limit(None)` or
/// `ConfigBuilder::unlimited_scrollback()`.
pub const DEFAULT_LINE_LIMIT: usize = 100_000;

/// Maximum decompressed size for a single scrollback page (64 MiB).
pub(crate) const MAX_DECOMPRESSED_SCROLLBACK_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// Binary search on a sorted cumulative array, counting iterations via `on_step`.
pub(crate) fn binary_search_counted(
    cumulative: &[usize],
    target: usize,
    mut on_step: impl FnMut(),
) -> Result<usize, usize> {
    let mut left = 0usize;
    let mut right = cumulative.len();

    while left < right {
        on_step();
        let mid = left + (right - left) / 2;
        match cumulative[mid].cmp(&target) {
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid,
            std::cmp::Ordering::Equal => return Ok(mid),
        }
    }

    Err(left)
}

/// Decode zstd-compressed data with an output-size cap.
///
/// Only available with the `zstd` feature (the on-disk `.dtrm` cold format and
/// the in-memory zstd cold tier). The default build uses LZ4 for the cold tier
/// (see [`decode_cold_bounded`]).
#[cfg(feature = "zstd")]
pub(crate) fn decode_zstd_bounded(compressed: &[u8]) -> Result<Vec<u8>, ScrollbackError> {
    use std::io::Read;

    let decoder = zstd::Decoder::new(compressed)?;
    let max_plus_one = (MAX_DECOMPRESSED_SCROLLBACK_PAGE_BYTES as u64).saturating_add(1);
    let mut limited = decoder.take(max_plus_one);
    let mut decoded = Vec::with_capacity(compressed.len());
    limited.read_to_end(&mut decoded)?;
    if decoded.len() > MAX_DECOMPRESSED_SCROLLBACK_PAGE_BYTES {
        return Err(ScrollbackError::Decompression(format!(
            "decompressed size exceeds {MAX_DECOMPRESSED_SCROLLBACK_PAGE_BYTES} byte limit"
        )));
    }
    Ok(decoded)
}

/// Compress a serialized scrollback block for the in-memory cold tier.
///
/// Uses zstd (better ratio) when the `zstd` feature is on, and otherwise falls
/// back to the LZ4 path that already backs the warm tier. The codec is fixed at
/// compile time, so cold pages produced in a process are always decodable by the
/// matching [`decode_cold_bounded`] in the same build.
pub(crate) fn encode_cold_block(serialized: &[u8]) -> Result<Vec<u8>, ScrollbackError> {
    #[cfg(feature = "zstd")]
    {
        zstd::encode_all(serialized, 3).map_err(ScrollbackError::Io)
    }
    #[cfg(not(feature = "zstd"))]
    {
        crate::lz4::compress_prepend_size(serialized)
            .map_err(|err| ScrollbackError::Decompression(format!("LZ4: {err}")))
    }
}

/// Decode an in-memory cold-tier block with an output-size cap.
///
/// Mirror of [`encode_cold_block`]: zstd when the feature is on, otherwise the
/// size-prepended LZ4 path used by the warm tier.
pub(crate) fn decode_cold_bounded(compressed: &[u8]) -> Result<Vec<u8>, ScrollbackError> {
    #[cfg(feature = "zstd")]
    {
        decode_zstd_bounded(compressed)
    }
    #[cfg(not(feature = "zstd"))]
    {
        decompress_lz4_bounded(compressed)
    }
}

/// Decompress LZ4 data with a validated prepended size.
pub(crate) fn decompress_lz4_bounded(compressed: &[u8]) -> Result<Vec<u8>, ScrollbackError> {
    if compressed.len() < 4 {
        return Err(ScrollbackError::Decompression(
            "LZ4 data too short for size prefix".to_string(),
        ));
    }

    let claimed_size =
        u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]) as usize;
    if claimed_size > MAX_DECOMPRESSED_SCROLLBACK_PAGE_BYTES {
        return Err(ScrollbackError::Decompression(format!(
            "LZ4 prepended size {claimed_size} exceeds {MAX_DECOMPRESSED_SCROLLBACK_PAGE_BYTES} byte limit"
        )));
    }

    crate::lz4::decompress_size_prepended(compressed)
        .map_err(|err| ScrollbackError::Decompression(format!("LZ4: {err}")))
}
