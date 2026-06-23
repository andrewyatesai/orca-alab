// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! RFC 1951 (DEFLATE) + RFC 1950 (zlib) decompression — zero dependencies.
//!
//! A from-scratch, panic-free `inflate` for decompressing attacker-supplied
//! streams (the Kitty graphics `o=z` transport). Every decode is bounded by a
//! caller-supplied `max_output` ceiling, so a decompression bomb fails with
//! [`InflateError::OutputTooLarge`] instead of exhausting memory. Huffman tables
//! use the canonical count/symbol construction (Mark Adler's `puff.c`), kept
//! deliberately simple and TOTAL — every input either decodes or returns an
//! error; none panic, none allocate without bound.

/// Why a DEFLATE/zlib stream could not be decompressed. Never a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflateError {
    /// The bit stream ended before the data did.
    Truncated,
    /// The 2-byte zlib header is not `CM=8`/`FCHECK`-valid, or asks for a preset
    /// dictionary (unsupported).
    BadZlibHeader,
    /// A DEFLATE block declared the reserved block type `11`.
    BadBlockType,
    /// A stored block's `LEN`/`NLEN` ones-complement check failed.
    BadStoredLength,
    /// A Huffman code did not resolve to any symbol (corrupt table or stream).
    BadSymbol,
    /// A back-reference distance is zero or points before the output start.
    BadDistance,
    /// Decompression would exceed the caller's `max_output` ceiling (anti-bomb).
    OutputTooLarge,
    /// The zlib trailer's Adler-32 did not match the decompressed data.
    BadChecksum,
}

const MAXBITS: usize = 15;

// RFC 1951 §3.2.5 — length codes 257..285.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
// RFC 1951 §3.2.5 — distance codes 0..29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
// RFC 1951 §3.2.7 — the order code-length code lengths are transmitted in.
const CLCL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// LSB-first bit reader over a byte slice (DEFLATE bit order).
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u32, InflateError> {
        let byte = *self
            .data
            .get(self.byte_pos)
            .ok_or(InflateError::Truncated)?;
        let bit = (byte >> self.bit_pos) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(u32::from(bit))
    }

    fn read_bits(&mut self, n: u32) -> Result<u32, InflateError> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.read_bit()? << i;
        }
        Ok(v)
    }

    /// Discard bits up to the next byte boundary (for stored blocks).
    fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    /// Read a byte-aligned `u8` (caller must be byte-aligned).
    fn read_aligned_byte(&mut self) -> Result<u8, InflateError> {
        let b = *self
            .data
            .get(self.byte_pos)
            .ok_or(InflateError::Truncated)?;
        self.byte_pos += 1;
        Ok(b)
    }

    /// Read a byte-aligned little-endian `u16`.
    fn read_aligned_u16(&mut self) -> Result<u16, InflateError> {
        let lo = self.read_aligned_byte()?;
        let hi = self.read_aligned_byte()?;
        Ok(u16::from_le_bytes([lo, hi]))
    }
}

/// Canonical Huffman decode table (count/symbol form, RFC 1951 / `puff.c`).
struct Huffman {
    count: [u16; MAXBITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Build from per-symbol code lengths (0 = symbol unused).
    fn new(lengths: &[u16]) -> Result<Self, InflateError> {
        let mut count = [0u16; MAXBITS + 1];
        for &len in lengths {
            let l = len as usize;
            if l > MAXBITS {
                return Err(InflateError::BadSymbol);
            }
            count[l] += 1;
        }
        let mut offs = [0u16; MAXBITS + 2];
        for len in 1..=MAXBITS {
            offs[len + 1] = offs[len] + count[len];
        }
        let total = lengths.iter().filter(|&&l| l != 0).count();
        let mut symbol = vec![0u16; total];
        let mut next = offs;
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let l = len as usize;
                symbol[next[l] as usize] = sym as u16;
                next[l] += 1;
            }
        }
        Ok(Self { count, symbol })
    }

    /// Decode one symbol, reading one bit at a time (MSB-accumulated code).
    fn decode(&self, br: &mut BitReader) -> Result<u16, InflateError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAXBITS {
            code |= br.read_bit()? as i32;
            let cnt = i32::from(self.count[len]);
            if code - first < cnt {
                let idx = (index + (code - first)) as usize;
                return self.symbol.get(idx).copied().ok_or(InflateError::BadSymbol);
            }
            index += cnt;
            first += cnt;
            first <<= 1;
            code <<= 1;
        }
        Err(InflateError::BadSymbol)
    }
}

fn fixed_lit() -> Huffman {
    let mut lengths = [0u16; 288];
    lengths[0..=143].fill(8);
    lengths[144..=255].fill(9);
    lengths[256..=279].fill(7);
    lengths[280..=287].fill(8);
    // Lengths are all within range, so construction cannot fail.
    Huffman::new(&lengths).unwrap_or(Huffman {
        count: [0; MAXBITS + 1],
        symbol: Vec::new(),
    })
}

fn fixed_dist() -> Huffman {
    let lengths = [5u16; 30];
    Huffman::new(&lengths).unwrap_or(Huffman {
        count: [0; MAXBITS + 1],
        symbol: Vec::new(),
    })
}

fn read_dynamic_tables(br: &mut BitReader) -> Result<(Huffman, Huffman), InflateError> {
    let hlit = br.read_bits(5)? as usize + 257;
    let hdist = br.read_bits(5)? as usize + 1;
    let hclen = br.read_bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 || hclen > 19 {
        return Err(InflateError::BadSymbol);
    }
    let mut cl_lengths = [0u16; 19];
    for &slot in CLCL_ORDER.iter().take(hclen) {
        cl_lengths[slot] = br.read_bits(3)? as u16;
    }
    let cl_huff = Huffman::new(&cl_lengths)?;

    let mut lengths = vec![0u16; hlit + hdist];
    let mut i = 0;
    while i < lengths.len() {
        let sym = cl_huff.decode(br)?;
        match sym {
            0..=15 => {
                lengths[i] = sym;
                i += 1;
            }
            16 => {
                // Repeat the previous length 3..6 times.
                if i == 0 {
                    return Err(InflateError::BadSymbol);
                }
                let prev = lengths[i - 1];
                let repeat = 3 + br.read_bits(2)? as usize;
                for _ in 0..repeat {
                    if i >= lengths.len() {
                        return Err(InflateError::BadSymbol);
                    }
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 => {
                // Repeat zero 3..10 times.
                let repeat = 3 + br.read_bits(3)? as usize;
                for _ in 0..repeat {
                    if i >= lengths.len() {
                        return Err(InflateError::BadSymbol);
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            18 => {
                // Repeat zero 11..138 times.
                let repeat = 11 + br.read_bits(7)? as usize;
                for _ in 0..repeat {
                    if i >= lengths.len() {
                        return Err(InflateError::BadSymbol);
                    }
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => return Err(InflateError::BadSymbol),
        }
    }

    let lit = Huffman::new(&lengths[..hlit])?;
    let dist = Huffman::new(&lengths[hlit..])?;
    Ok((lit, dist))
}

fn inflate_stored(
    br: &mut BitReader,
    out: &mut Vec<u8>,
    max_output: usize,
) -> Result<(), InflateError> {
    br.align_to_byte();
    let len = br.read_aligned_u16()? as usize;
    let nlen = br.read_aligned_u16()?;
    if (len as u16) != !nlen {
        return Err(InflateError::BadStoredLength);
    }
    if out.len().saturating_add(len) > max_output {
        return Err(InflateError::OutputTooLarge);
    }
    out.reserve(len);
    for _ in 0..len {
        let b = br.read_aligned_byte()?;
        out.push(b);
    }
    Ok(())
}

fn inflate_block(
    br: &mut BitReader,
    out: &mut Vec<u8>,
    max_output: usize,
    lit: &Huffman,
    dist: &Huffman,
) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(br)?;
        if sym < 256 {
            if out.len() >= max_output {
                return Err(InflateError::OutputTooLarge);
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(()); // end of block
        } else {
            let li = sym as usize - 257;
            let length = *LENGTH_BASE.get(li).ok_or(InflateError::BadSymbol)? as usize
                + br.read_bits(LENGTH_EXTRA[li])? as usize;
            let dsym = dist.decode(br)? as usize;
            let distance = *DIST_BASE.get(dsym).ok_or(InflateError::BadDistance)? as usize
                + br.read_bits(DIST_EXTRA[dsym])? as usize;
            if distance == 0 || distance > out.len() {
                return Err(InflateError::BadDistance);
            }
            if out.len().saturating_add(length) > max_output {
                return Err(InflateError::OutputTooLarge);
            }
            out.reserve(length);
            let start = out.len() - distance;
            // Byte-by-byte so overlapping copies (distance < length, i.e. RLE) work.
            for i in 0..length {
                let b = out[start + i];
                out.push(b);
            }
        }
    }
}

/// Decompress a raw DEFLATE (RFC 1951) stream, bounded by `max_output` bytes.
///
/// # Errors
/// Returns an [`InflateError`] for any malformed input or if the decompressed
/// size would exceed `max_output` (decompression-bomb guard). Never panics.
pub fn inflate(input: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    let mut br = BitReader::new(input);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let bfinal = br.read_bit()?;
        let btype = br.read_bits(2)?;
        match btype {
            0 => inflate_stored(&mut br, &mut out, max_output)?,
            1 => inflate_block(&mut br, &mut out, max_output, &fixed_lit(), &fixed_dist())?,
            2 => {
                let (lit, dist) = read_dynamic_tables(&mut br)?;
                inflate_block(&mut br, &mut out, max_output, &lit, &dist)?;
            }
            _ => return Err(InflateError::BadBlockType),
        }
        if bfinal == 1 {
            return Ok(out);
        }
    }
}

/// The RFC 1950 Adler-32 checksum of `data`.
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

/// Decompress a zlib (RFC 1950) stream, bounded by `max_output` bytes, verifying
/// the header and the trailing Adler-32.
///
/// # Errors
/// Returns an [`InflateError`] for a bad header, malformed DEFLATE data, a size
/// over `max_output`, or an Adler-32 mismatch. Never panics.
pub fn zlib_decompress(input: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    // 2-byte header + at least an empty deflate stream + 4-byte Adler-32.
    if input.len() < 6 {
        return Err(InflateError::Truncated);
    }
    let cmf = input[0];
    let flg = input[1];
    if (cmf & 0x0f) != 8 {
        return Err(InflateError::BadZlibHeader); // CM must be 8 (DEFLATE)
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err(InflateError::BadZlibHeader); // FCHECK
    }
    if (flg & 0x20) != 0 {
        return Err(InflateError::BadZlibHeader); // preset dictionary unsupported
    }
    let deflate = &input[2..input.len() - 4];
    let out = inflate(deflate, max_output)?;
    let trailer = &input[input.len() - 4..];
    let expected = u32::from_be_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    if adler32(&out) != expected {
        return Err(InflateError::BadChecksum);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference vectors produced by python3 `zlib.compress` (cross-checked).
    const EMPTY: &[u8] = &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
    const HELLO: &[u8] = &[
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0xd7, 0x51, 0x28, 0xcf, 0x2f, 0xca, 0x49, 0x01,
        0x00, 0x1d, 0x54, 0x04, 0x89,
    ];
    const STORED: &[u8] = &[
        0x78, 0x01, 0x01, 0x0a, 0x00, 0xf5, 0xff, 0x41, 0x42, 0x43, 0x44, 0x45, 0x41, 0x42, 0x43,
        0x44, 0x45, 0x0e, 0x5b, 0x02, 0x9f,
    ];
    const BACKREFS: &[u8] = &[
        0x78, 0x9c, 0x4b, 0x4c, 0x4a, 0xa4, 0x2a, 0x04, 0x00, 0xd2, 0x74, 0x1e, 0x79,
    ];
    // BTYPE=2 dynamic-Huffman block (level 9, ~1.6KB of skewed text).
    const DYNAMIC: &[u8] = &[
        0x78, 0xda, 0xed, 0xce, 0x41, 0x12, 0x02, 0x21, 0x0c, 0x44, 0xd1, 0xab, 0xf4, 0x09, 0xbc,
        0x13, 0x42, 0x18, 0xa2, 0x40, 0x30, 0x01, 0x71, 0xe6, 0xf4, 0x52, 0xde, 0xc2, 0x2a, 0x56,
        0xbd, 0xf8, 0x8b, 0x7e, 0x3d, 0x11, 0x5e, 0x83, 0xfd, 0x13, 0x77, 0x95, 0x59, 0x11, 0xe5,
        0x83, 0xc7, 0x28, 0xcd, 0x20, 0x6f, 0x52, 0xf4, 0x95, 0xb3, 0xbb, 0x4e, 0x04, 0x39, 0x30,
        0x13, 0x67, 0x82, 0x43, 0x53, 0x39, 0xd4, 0x95, 0xb2, 0xba, 0x77, 0x4a, 0x71, 0xe4, 0x7c,
        0x62, 0x2a, 0x77, 0xb2, 0x55, 0x03, 0xc5, 0xec, 0x3a, 0xad, 0xf5, 0x52, 0x9a, 0x92, 0x99,
        0x28, 0xb8, 0x42, 0x87, 0x75, 0x4c, 0xee, 0x09, 0x17, 0xa9, 0xac, 0xdc, 0xa8, 0x06, 0xaa,
        0x9e, 0xc9, 0x6e, 0xbf, 0x9f, 0xcd, 0xd8, 0x8c, 0xcd, 0xd8, 0x8c, 0xcd, 0xf8, 0x73, 0xc6,
        0x17, 0xc1, 0xd0, 0x5c, 0x27,
    ];

    #[test]
    fn empty_stream() {
        assert_eq!(zlib_decompress(EMPTY, 1 << 20).unwrap(), b"");
    }

    #[test]
    fn fixed_huffman_literals() {
        assert_eq!(zlib_decompress(HELLO, 1 << 20).unwrap(), b"hello, world");
    }

    #[test]
    fn stored_block() {
        assert_eq!(zlib_decompress(STORED, 1 << 20).unwrap(), b"ABCDEABCDE");
    }

    #[test]
    fn fixed_huffman_backreferences() {
        let expected = "ab".repeat(40).into_bytes();
        assert_eq!(zlib_decompress(BACKREFS, 1 << 20).unwrap(), expected);
    }

    #[test]
    fn dynamic_huffman_block() {
        let base = "the quick brown fox jumps over the lazy dog while a programmer \
                    carefully writes a deflate decompressor in rust with zero dependencies. ";
        let expected = base.repeat(12).into_bytes();
        assert_eq!(zlib_decompress(DYNAMIC, 1 << 20).unwrap(), expected);
    }

    #[test]
    fn raw_inflate_without_zlib_wrapper() {
        // Strip the 2-byte header + 4-byte Adler trailer -> a bare DEFLATE stream.
        let raw = &HELLO[2..HELLO.len() - 4];
        assert_eq!(inflate(raw, 1 << 20).unwrap(), b"hello, world");
    }

    #[test]
    fn output_cap_rejects_bomb() {
        // The dynamic vector expands to 1620 bytes; a 10-byte ceiling must reject it.
        assert_eq!(
            zlib_decompress(DYNAMIC, 10),
            Err(InflateError::OutputTooLarge)
        );
    }

    #[test]
    fn truncated_input_errors_not_panics() {
        for cut in 0..HELLO.len() {
            // Any prefix must return an error (or, by luck, decode) — never panic.
            let _ = zlib_decompress(&HELLO[..cut], 1 << 20);
            let _ = inflate(&HELLO[..cut], 1 << 20);
        }
    }

    #[test]
    fn bad_zlib_header_rejected() {
        assert_eq!(
            zlib_decompress(&[0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01], 1 << 20),
            Err(InflateError::BadZlibHeader)
        );
    }

    #[test]
    fn corrupt_checksum_rejected() {
        let mut bad = HELLO.to_vec();
        let n = bad.len();
        bad[n - 1] ^= 0xff; // flip the last Adler byte
        assert_eq!(
            zlib_decompress(&bad, 1 << 20),
            Err(InflateError::BadChecksum)
        );
    }

    #[test]
    fn adler32_known_value() {
        // Adler-32("hello, world") cross-checked against zlib.
        assert_eq!(adler32(b"hello, world"), 0x1d54_0489);
    }

    /// One step of a deterministic LCG (same constants as the engine fuzz), so any
    /// failure reproduces exactly.
    fn next(state: &mut u64) -> u32 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*state >> 33) as u32
    }

    #[test]
    fn fuzz_never_panics_and_respects_cap() {
        // Inflate parses ATTACKER bytes (the Kitty o=z transport). Throw arbitrary
        // and adversarially-mutated streams at it: it must always return Ok/Err,
        // never panic, and never produce more than `max_output` bytes.
        const CAP: usize = 4096;
        let seeds: [&[u8]; 5] = [EMPTY, HELLO, STORED, BACKREFS, DYNAMIC];
        let mut state = 0x1234_5678_9abc_def0u64;
        for _ in 0..20_000 {
            // Build a buffer: sometimes pure noise, sometimes a mutated valid stream.
            let len = (next(&mut state) % 64) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| next(&mut state) as u8).collect();
            if next(&mut state) & 1 == 0 {
                // Mutate a real vector: truncate and flip some bytes.
                let seed = seeds[(next(&mut state) as usize) % seeds.len()];
                let cut = (next(&mut state) as usize) % (seed.len() + 1);
                buf = seed[..cut].to_vec();
                for _ in 0..(next(&mut state) % 4) {
                    if !buf.is_empty() {
                        let idx = (next(&mut state) as usize) % buf.len();
                        buf[idx] ^= (next(&mut state) as u8) | 1;
                    }
                }
            }
            if let Ok(out) = zlib_decompress(&buf, CAP) {
                assert!(out.len() <= CAP, "zlib_decompress exceeded the cap");
            }
            if let Ok(out) = inflate(&buf, CAP) {
                assert!(out.len() <= CAP, "inflate exceeded the cap");
            }
        }
    }
}
