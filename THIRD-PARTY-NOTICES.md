# Third-Party Notices — Rust Binary Artifacts

Orca ships six binary artifacts compiled from Rust sources in this repository:

| Artifact | Built from | Location in the packaged app |
| --- | --- | --- |
| `aterm_wasm_bg.wasm` | `rust/aterm/crates/aterm-wasm` (wasm32-unknown-unknown) | bundled by Vite into the renderer assets inside `app.asar` |
| `aterm_gpu_web_bg.wasm` | `rust/aterm/crates/aterm-gpu-web` (wasm32-unknown-unknown) | bundled by Vite into the renderer assets inside `app.asar` |
| `orca_node.node` | `native/orca-node` (Node-API addon) | `Resources/orca_node.node` (macOS) / `resources/orca_node.node` (Windows, Linux) |
| `orca-daemon` | `rust/crates/orca-daemon` (workspace `rust/Cargo.toml`, native per platform) | `Resources/orca-daemon` (macOS) / `resources/orca-daemon` (Linux) / `resources/orca-daemon.exe` (Windows) |
| `orca_crypto_wasm_bg.wasm` | `rust/orca-crypto-wasm` (wasm32-unknown-unknown), vendored at `src/renderer/src/lib/crypto-wasm/` and base64-embedded via `src/shared/crypto-wasm/` | bundled by Vite into the renderer assets inside `app.asar`; the base64 copy ships in the Node bundles (main process, relay, CLI) |
| `orca_git_wasm_bg.wasm` | `rust/orca-git-wasm` (wasm32-unknown-unknown), vendored at `src/renderer/src/lib/git-wasm/` | bundled by Vite into the renderer assets inside `app.asar` |

This file enumerates every crate compiled into those artifacts and reproduces
the license notices their terms require. A copy ships with the packaged app at
`Resources/licenses/THIRD-PARTY-NOTICES.md` (macOS) /
`resources/licenses/THIRD-PARTY-NOTICES.md` (Windows, Linux), alongside the SIL
OFL 1.1 texts for the bundled JetBrains Mono and Geist fonts
(`JETBRAINS-MONO-OFL.txt`, `GEIST-OFL.txt` — sources in
`src/renderer/src/assets/fonts/`).

Where a crate is offered under a choice of licenses (e.g. `MIT OR Apache-2.0`),
Orca receives it under the MIT option, or under the first-listed option when
MIT is not offered. The full texts of every license that is the sole option
for at least one listed crate are reproduced in the appendix.

The crate lists below are generated mechanically from the pinned lockfiles
(regenerate after dependency changes):

```
cargo tree --manifest-path rust/aterm/Cargo.toml --locked \
  -p aterm-wasm -p aterm-gpu-web --target wasm32-unknown-unknown \
  -e normal --prefix none -f '{p}|{l}'

# union over the five shipped triples
for t in aarch64-apple-darwin x86_64-apple-darwin \
         x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
         x86_64-pc-windows-msvc; do
  cargo tree --manifest-path native/orca-node/Cargo.toml --locked \
    --target $t -e normal --prefix none -f '{p}|{l}'
done

# orca-daemon: union over the same five triples. RUSTFLAGS= neutralizes the
# Trust-toolchain rustflags in rust/.cargo/config.toml on stock rustc.
for t in aarch64-apple-darwin x86_64-apple-darwin \
         x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu \
         x86_64-pc-windows-msvc; do
  RUSTFLAGS= cargo tree --manifest-path rust/Cargo.toml --locked \
    -p orca-daemon --target $t -e normal --prefix none -f '{p}|{l}'
done

# run from the repo root so rust/.cargo/config.toml (offline vendoring) does
# not apply — the wasm workspaces resolve their web deps from crates.io
cargo tree --manifest-path rust/orca-crypto-wasm/Cargo.toml --locked \
  --target wasm32-unknown-unknown -e normal --prefix none -f '{p}|{l}'
cargo tree --manifest-path rust/orca-git-wasm/Cargo.toml --locked \
  --target wasm32-unknown-unknown -e normal --prefix none -f '{p}|{l}'
```

`-e normal` excludes dev- and build-dependencies, which do not ship.
Proc-macro crates execute at compile time only and contribute no code to the
shipped binaries; they are listed for completeness and marked as such.

## 1. The aterm terminal engine (Apache-2.0) — NOTICE propagation

Four of the artifacts — the two aterm wasm modules, `orca_node.node`, and
`orca-daemon` — compile in crates of the aterm terminal engine, vendored
at `rust/aterm/` and licensed under the Apache License, Version 2.0 (full text
in the appendix). Apache-2.0 §4(d) requires carrying the following NOTICE,
reproduced verbatim from `rust/aterm/NOTICE`:

```
aterm
Copyright 2026 Andrew Yates

This product is licensed under the Apache License, Version 2.0 (the "License").
You may obtain a copy of the License in the LICENSE file at the root of this
repository, or at http://www.apache.org/licenses/LICENSE-2.0.

The terminal as a whole, and every crate under `crates/` and `apps/` except
where a more specific license is declared below, is original work of The aterm
Authors and is distributed under Apache-2.0. aterm is a foundations-first,
zero-external-dependency project: it deliberately avoids the registry-dependency
tree, so this ledger is short by design. It will be kept current as the single
source of truth for any third-party code carried in-tree.

================================================================================
THIRD-PARTY SOFTWARE
================================================================================

The following components are vendored (copied in-tree) from third-party
projects. Their original licenses apply to the corresponding files and are
reproduced in full at the referenced paths. Their SPDX-License-Identifier
headers identify them at the file level.

--------------------------------------------------------------------------------
1. lz4_flex  (vendored as crate `aterm-lz4`)
--------------------------------------------------------------------------------

   Component   : aterm-lz4 — block-mode-only subset of the LZ4 compressor.
   Upstream    : lz4_flex 0.11.5  (https://github.com/pseitz/lz4_flex)
   Author      : Pascal Seitz et al.
   License     : MIT
   SPDX        : MIT  (crate as a whole: "MIT AND Apache-2.0")
   License text: crates/aterm-lz4/LICENSE-MIT

   The files under `crates/aterm-lz4/src/block/`, `crates/aterm-lz4/src/sink.rs`,
   `crates/aterm-lz4/src/fastcpy.rs`, and `crates/aterm-lz4/src/fastcpy_unsafe.rs`
   are copied verbatim from upstream lz4_flex 0.11.5 and remain under the MIT
   license (see crates/aterm-lz4/LICENSE-MIT). Only `crates/aterm-lz4/src/lib.rs`
   carries local aterm modifications (crate docs, module wiring, re-exports) and
   is therefore dual-licensed "Apache-2.0 AND MIT". The crate's `Cargo.toml`
   declares `license = "MIT AND Apache-2.0"` accordingly.

   MIT License (MIT)

   Copyright (c) 2020 Pascal Seitz

   Permission is hereby granted, free of charge, to any person obtaining a copy
   of this software and associated documentation files (the "Software"), to deal
   in the Software without restriction, including without limitation the rights
   to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
   copies of the Software, and to permit persons to whom the Software is
   furnished to do so, subject to the following conditions:

   The above copyright notice and this permission notice shall be included in
   all copies or substantial portions of the Software.

   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
   AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
   OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
   SOFTWARE.

================================================================================
UNICODE DATA
================================================================================

The crate `aterm-grapheme` ships lookup tables generated from the Unicode
Character Database (Unicode 16.0.0) — `crates/aterm-grapheme/src/tables/width.rs`
and `crates/aterm-grapheme/src/tables/gcb.rs`. These tables are generated data,
not vendored source: they are produced by aterm's own generator from the UCD and
are distributed under Apache-2.0 as part of aterm. Use of the underlying Unicode
data is governed by the Unicode License Agreement
(https://www.unicode.org/license.txt).
```

The aterm crates compiled into the shipped artifacts therefore carry, beyond
Apache-2.0:

- `aterm-lz4` (`MIT AND Apache-2.0`): vendors the block-mode subset of
  `lz4_flex` 0.11.5 (MIT, Copyright (c) 2020 Pascal Seitz) — MIT text and the
  exact file provenance are in the NOTICE above.
- `aterm-grapheme`: ships lookup tables generated from the Unicode Character
  Database (Unicode 16.0.0); use of the underlying data is governed by the
  Unicode License (see the Unicode License v3 text in the appendix).

## 2. `aterm_wasm_bg.wasm` + `aterm_gpu_web_bg.wasm` — crate closure

Resolved dependency closure of `aterm-wasm` and `aterm-gpu-web` for
`wasm32-unknown-unknown` (normal dependencies only), from
`rust/aterm/Cargo.lock`. This includes the wgpu WebGL/WebGPU renderer tree
(`wgpu`, `wgpu-core`, `wgpu-hal`, `naga` — MIT OR Apache-2.0) used by the GPU
web crate.

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | crates.io |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | crates.io |
| aho-corasick | 1.1.4 | Unlicense OR MIT | crates.io |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | crates.io |
| arrayvec | 0.7.6 | MIT OR Apache-2.0 | crates.io |
| aterm-alloc | 0.56.0 | Apache-2.0 | in-tree |
| aterm-bits | 0.56.0 | Apache-2.0 | in-tree |
| aterm-codec | 0.56.0 | Apache-2.0 | in-tree |
| aterm-containment | 0.56.0 | Apache-2.0 | in-tree |
| aterm-core | 0.56.0 | Apache-2.0 | in-tree |
| aterm-effects | 0.56.0 | Apache-2.0 | in-tree |
| aterm-error | 0.56.0 | Apache-2.0 | in-tree |
| aterm-error-derive | 0.56.0 | Apache-2.0 | in-tree (build-time proc-macro) |
| aterm-ffi-types | 0.56.0 | Apache-2.0 | in-tree |
| aterm-gpu | 0.56.0 | Apache-2.0 | in-tree |
| aterm-gpu-web | 0.0.1 | MIT | in-tree |
| aterm-grapheme | 0.56.0 | Apache-2.0 | in-tree |
| aterm-grid | 0.56.0 | Apache-2.0 | in-tree |
| aterm-hash | 0.56.0 | Apache-2.0 | in-tree |
| aterm-lexicon | 0.56.0 | Apache-2.0 | in-tree |
| aterm-log | 0.56.0 | Apache-2.0 | in-tree |
| aterm-lz4 | 0.56.0 | MIT AND Apache-2.0 | in-tree |
| aterm-parser | 0.56.0 | Apache-2.0 | in-tree |
| aterm-policy | 0.56.0 | Apache-2.0 | in-tree |
| aterm-predict | 0.56.0 | Apache-2.0 | in-tree |
| aterm-provenance | 0.56.0 | Apache-2.0 | in-tree |
| aterm-render | 0.56.0 | Apache-2.0 | in-tree |
| aterm-render-api | 0.56.0 | Apache-2.0 | in-tree |
| aterm-rle | 0.56.0 | Apache-2.0 | in-tree |
| aterm-scene | 0.56.0 | Apache-2.0 | in-tree |
| aterm-scrollback | 0.56.0 | Apache-2.0 | in-tree |
| aterm-search | 0.56.0 | Apache-2.0 | in-tree |
| aterm-selection | 0.56.0 | Apache-2.0 | in-tree |
| aterm-shell-integration | 0.56.0 | Apache-2.0 | in-tree |
| aterm-sixel | 0.56.0 | Apache-2.0 | in-tree |
| aterm-tempfile | 0.56.0 | Apache-2.0 | in-tree |
| aterm-types | 0.56.0 | Apache-2.0 | in-tree |
| aterm-vi | 0.56.0 | Apache-2.0 | in-tree |
| aterm-wasm | 0.0.1 | MIT | in-tree |
| bit-set | 0.9.1 | Apache-2.0 OR MIT | crates.io |
| bit-vec | 0.9.1 | Apache-2.0 OR MIT | crates.io |
| bitflags | 1.3.2 | MIT/Apache-2.0 | crates.io |
| bitflags | 2.13.0 | MIT OR Apache-2.0 | crates.io |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates.io |
| bytemuck | 1.25.0 | Zlib OR Apache-2.0 OR MIT | crates.io |
| bytemuck_derive | 1.10.2 | Zlib OR Apache-2.0 OR MIT | crates.io (build-time proc-macro) |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | crates.io |
| codespan-reporting | 0.13.1 | Apache-2.0 | crates.io |
| console_error_panic_hook | 0.1.7 | Apache-2.0/MIT | crates.io |
| core_maths | 0.1.1 | MIT | crates.io |
| crc32fast | 1.5.0 | MIT OR Apache-2.0 | crates.io |
| document-features | 0.2.12 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | crates.io |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | crates.io |
| flate2 | 1.1.9 | MIT OR Apache-2.0 | crates.io |
| foldhash | 0.1.5 | Zlib | crates.io |
| foldhash | 0.2.0 | Zlib | crates.io |
| font8x8 | 0.3.1 | MIT | crates.io |
| fontdue | 0.9.3 | MIT OR Apache-2.0 OR Zlib | crates.io |
| futures-core | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-task | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-util | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | crates.io |
| glow | 0.17.0 | MIT OR Apache-2.0 OR Zlib | crates.io |
| half | 2.7.1 | MIT OR Apache-2.0 | crates.io |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 | crates.io |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | crates.io |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | crates.io |
| hexf-parse | 0.2.1 | CC0-1.0 | crates.io |
| indexmap | 2.14.0 | Apache-2.0 OR MIT | in-tree |
| js-sys | 0.3.85 | MIT OR Apache-2.0 | crates.io |
| libm | 0.2.16 | MIT | in-tree |
| litrs | 1.0.0 | MIT OR Apache-2.0 | crates.io |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | crates.io |
| log | 0.4.32 | MIT OR Apache-2.0 | crates.io |
| memchr | 2.8.1 | Unlicense OR MIT | crates.io |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | crates.io |
| naga | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | crates.io |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | crates.io |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | crates.io |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | crates.io |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | crates.io |
| png | 0.17.16 | MIT OR Apache-2.0 | crates.io |
| proc-macro2 | 1.0.106 | MIT OR Apache-2.0 | crates.io |
| profiling | 1.0.18 | MIT OR Apache-2.0 | crates.io |
| quote | 1.0.45 | MIT OR Apache-2.0 | crates.io |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | crates.io |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | crates.io |
| regex | 1.12.3 | MIT OR Apache-2.0 | crates.io |
| regex-automata | 0.4.14 | MIT OR Apache-2.0 | crates.io |
| regex-syntax | 0.8.10 | MIT OR Apache-2.0 | crates.io |
| rustc-hash | 1.1.0 | Apache-2.0/MIT | crates.io |
| rustybuzz | 0.20.1 | MIT | crates.io |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | crates.io |
| serde | 1.0.228 | MIT OR Apache-2.0 | crates.io |
| serde_core | 1.0.228 | MIT OR Apache-2.0 | crates.io |
| serde_derive | 1.0.228 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates.io |
| simd-adler32 | 0.3.9 | MIT | crates.io |
| slab | 0.4.12 | MIT | crates.io |
| slotmap | 1.1.1 | Zlib | crates.io |
| smallvec | 1.15.1 | MIT OR Apache-2.0 | crates.io |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | crates.io |
| syn | 2.0.117 | MIT OR Apache-2.0 | crates.io |
| thiserror | 2.0.18 | MIT OR Apache-2.0 | crates.io |
| thiserror-impl | 2.0.18 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates.io |
| toml_datetime | 0.6.11 | MIT OR Apache-2.0 | crates.io |
| toml_edit | 0.22.27 | MIT OR Apache-2.0 | crates.io |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates.io |
| ttf-parser | 0.21.1 | MIT OR Apache-2.0 | crates.io |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | crates.io |
| unicode-bidi-mirroring | 0.4.0 | MIT/Apache-2.0 | crates.io |
| unicode-ccc | 0.4.0 | MIT/Apache-2.0 | crates.io |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | crates.io |
| unicode-properties | 0.1.4 | MIT/Apache-2.0 | crates.io |
| unicode-script | 0.5.8 | MIT OR Apache-2.0 | crates.io |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-futures | 0.4.58 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-macro | 0.2.108 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| wasm-bindgen-macro-support | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-shared | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| web-sys | 0.3.85 | MIT OR Apache-2.0 | crates.io |
| web-time | 1.1.0 | MIT OR Apache-2.0 | crates.io |
| wgpu | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| wgpu-core | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| wgpu-core-deps-wasm | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| wgpu-hal | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| wgpu-naga-bridge | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| wgpu-types | 29.0.3 | MIT OR Apache-2.0 | crates.io |
| winnow | 0.7.15 | MIT | in-tree |
| zerocopy | 0.8.50 | BSD-2-Clause OR Apache-2.0 OR MIT | crates.io |
| zerocopy-derive | 0.8.50 | BSD-2-Clause OR Apache-2.0 OR MIT | crates.io (build-time proc-macro) |

## 3. `orca_node.node` — crate closure

Resolved dependency closure of `orca-node` (normal dependencies only), union
over the five shipped target triples (macOS x64/arm64, Linux x64/arm64,
Windows x64), from `native/orca-node/Cargo.lock`. `zstd-sys` statically links
the zstd C library (BSD-3-Clause OR GPL-2.0; Orca receives it under
BSD-3-Clause — text in the appendix). `libsqlite3-sys` statically links the
bundled SQLite C library, which is in the public domain
(https://www.sqlite.org/copyright.html) and carries no notice requirement.

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| ahash | 0.8.12 | MIT OR Apache-2.0 | crates.io |
| aho-corasick | 1.1.4 | Unlicense OR MIT | crates.io |
| aterm-alloc | 0.56.0 | Apache-2.0 | in-tree |
| aterm-bits | 0.56.0 | Apache-2.0 | in-tree |
| aterm-codec | 0.56.0 | Apache-2.0 | in-tree |
| aterm-containment | 0.56.0 | Apache-2.0 | in-tree |
| aterm-core | 0.56.0 | Apache-2.0 | in-tree |
| aterm-error | 0.56.0 | Apache-2.0 | in-tree |
| aterm-error-derive | 0.56.0 | Apache-2.0 | in-tree (build-time proc-macro) |
| aterm-ffi-types | 0.56.0 | Apache-2.0 | in-tree |
| aterm-grapheme | 0.56.0 | Apache-2.0 | in-tree |
| aterm-grid | 0.56.0 | Apache-2.0 | in-tree |
| aterm-hash | 0.56.0 | Apache-2.0 | in-tree |
| aterm-log | 0.56.0 | Apache-2.0 | in-tree |
| aterm-lz4 | 0.56.0 | MIT AND Apache-2.0 | in-tree |
| aterm-parser | 0.56.0 | Apache-2.0 | in-tree |
| aterm-policy | 0.56.0 | Apache-2.0 | in-tree |
| aterm-provenance | 0.56.0 | Apache-2.0 | in-tree |
| aterm-rle | 0.56.0 | Apache-2.0 | in-tree |
| aterm-scrollback | 0.56.0 | Apache-2.0 | in-tree |
| aterm-search | 0.56.0 | Apache-2.0 | in-tree |
| aterm-selection | 0.56.0 | Apache-2.0 | in-tree |
| aterm-shell-integration | 0.56.0 | Apache-2.0 | in-tree |
| aterm-sixel | 0.56.0 | Apache-2.0 | in-tree |
| aterm-tempfile | 0.56.0 | Apache-2.0 | in-tree |
| aterm-types | 0.56.0 | Apache-2.0 | in-tree |
| aterm-vi | 0.56.0 | Apache-2.0 | in-tree |
| bitflags | 2.13.0 | MIT OR Apache-2.0 | crates.io |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | crates.io |
| convert_case | 0.11.0 | MIT | crates.io |
| ctor | 1.0.7 | Apache-2.0 OR MIT | crates.io |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | crates.io |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 | crates.io |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 | crates.io |
| futures | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-channel | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-core | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-executor | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-io | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-macro | 0.3.32 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| futures-sink | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-task | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-util | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | crates.io |
| hashbrown | 0.14.5 | MIT OR Apache-2.0 | crates.io |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | crates.io |
| hashlink | 0.9.1 | MIT OR Apache-2.0 | crates.io |
| indexmap | 2.14.0 | Apache-2.0 OR MIT | crates.io |
| itoa | 1.0.18 | MIT OR Apache-2.0 | crates.io |
| libc | 0.2.186 | MIT OR Apache-2.0 | crates.io |
| libloading | 0.9.0 | ISC | crates.io |
| libsqlite3-sys | 0.28.0 | MIT | crates.io |
| memchr | 2.8.2 | Unlicense OR MIT | crates.io |
| napi | 3.9.3 | MIT | crates.io |
| napi-derive | 3.5.6 | MIT | crates.io (build-time proc-macro) |
| napi-derive-backend | 5.0.4 | MIT | crates.io |
| napi-sys | 3.2.2 | MIT | crates.io |
| nohash-hasher | 0.2.0 | Apache-2.0 OR MIT | crates.io |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | crates.io |
| orca-agents | 0.0.1 | MIT | in-tree |
| orca-config | 0.0.1 | MIT | in-tree |
| orca-core | 0.0.1 | MIT | in-tree |
| orca-dispatch | 0.0.1 | MIT | in-tree |
| orca-flow-control | 0.0.1 | MIT | in-tree |
| orca-git | 0.0.1 | MIT | in-tree |
| orca-net | 0.0.1 | MIT | in-tree |
| orca-node | 0.0.1 | MIT | in-tree |
| orca-provider-backoff | 0.0.1 | MIT | in-tree |
| orca-relay | 0.0.1 | MIT | in-tree |
| orca-runtime | 0.0.1 | MIT | in-tree |
| orca-ssh | 0.0.1 | MIT | in-tree |
| orca-store | 0.0.1 | MIT | in-tree |
| orca-terminal | 0.0.1 | MIT | in-tree |
| orca-text | 0.0.1 | MIT | in-tree |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | crates.io |
| proc-macro2 | 1.0.106 | MIT OR Apache-2.0 | crates.io |
| quote | 1.0.45 | MIT OR Apache-2.0 | crates.io |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | crates.io |
| regex | 1.12.4 | MIT OR Apache-2.0 | crates.io |
| regex-automata | 0.4.14 | MIT OR Apache-2.0 | crates.io |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | crates.io |
| rusqlite | 0.31.0 | MIT | crates.io |
| rustc-hash | 2.1.2 | Apache-2.0 OR MIT | crates.io |
| semver | 1.0.28 | MIT OR Apache-2.0 | crates.io |
| serde | 1.0.228 | MIT OR Apache-2.0 | crates.io |
| serde_core | 1.0.228 | MIT OR Apache-2.0 | crates.io |
| serde_derive | 1.0.228 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| serde_json | 1.0.150 | MIT OR Apache-2.0 | crates.io |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | crates.io |
| slab | 0.4.12 | MIT | crates.io |
| smallvec | 1.15.2 | MIT OR Apache-2.0 | crates.io |
| syn | 2.0.118 | MIT OR Apache-2.0 | crates.io |
| toml | 0.8.23 | MIT OR Apache-2.0 | crates.io |
| toml_datetime | 0.6.11 | MIT OR Apache-2.0 | crates.io |
| toml_edit | 0.22.27 | MIT OR Apache-2.0 | crates.io |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | crates.io |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | crates.io |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | crates.io |
| web-time | 1.1.0 | MIT OR Apache-2.0 | crates.io |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | crates.io |
| winnow | 0.7.15 | MIT | crates.io |
| zerocopy | 0.8.53 | BSD-2-Clause OR Apache-2.0 OR MIT | crates.io |
| zmij | 1.0.21 | MIT | crates.io |
| zstd | 0.13.3 | MIT | crates.io |
| zstd-safe | 7.2.4 | MIT OR Apache-2.0 | crates.io |
| zstd-sys | 2.0.16+zstd.1.5.7 | MIT/Apache-2.0 | crates.io |

## 4. `orca-daemon` — crate closure

Resolved dependency closure of `orca-daemon` (normal dependencies only), union
over the five shipped target triples (macOS x64/arm64, Linux x64/arm64,
Windows x64), from `rust/Cargo.lock`. Platform-conditional dependencies are
unioned: e.g. `nix`, `serial-unix`, and `termios` ship in the Unix builds
only; `orca-winpipe`, `serial-windows`, `winapi`, and `winreg` in the Windows
build only. The `rust/` workspace builds offline by construction: every
third-party crate resolves from its vendored, stripped copy checked in under
`rust/vendor` (`rust/.cargo/config.toml` replaces crates.io with the vendor
directory) — such crates are marked "in-tree (vendored)" below, with the
crates.io release they were vendored from identified by name and version.
`zstd-sys` statically links the zstd C library (BSD-3-Clause OR GPL-2.0;
Orca receives it under BSD-3-Clause — text in the appendix).

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| aho-corasick | 1.1.4 | Unlicense OR MIT | in-tree (vendored) |
| anyhow | 1.0.102 | MIT OR Apache-2.0 | in-tree (vendored) |
| aterm-alloc | 0.56.0 | Apache-2.0 | in-tree |
| aterm-bits | 0.56.0 | Apache-2.0 | in-tree |
| aterm-codec | 0.56.0 | Apache-2.0 | in-tree |
| aterm-containment | 0.56.0 | Apache-2.0 | in-tree |
| aterm-core | 0.56.0 | Apache-2.0 | in-tree |
| aterm-error | 0.56.0 | Apache-2.0 | in-tree |
| aterm-error-derive | 0.56.0 | Apache-2.0 | in-tree (build-time proc-macro) |
| aterm-ffi-types | 0.56.0 | Apache-2.0 | in-tree |
| aterm-grapheme | 0.56.0 | Apache-2.0 | in-tree |
| aterm-grid | 0.56.0 | Apache-2.0 | in-tree |
| aterm-hash | 0.56.0 | Apache-2.0 | in-tree |
| aterm-log | 0.56.0 | Apache-2.0 | in-tree |
| aterm-lz4 | 0.56.0 | MIT AND Apache-2.0 | in-tree |
| aterm-parser | 0.56.0 | Apache-2.0 | in-tree |
| aterm-policy | 0.56.0 | Apache-2.0 | in-tree |
| aterm-provenance | 0.56.0 | Apache-2.0 | in-tree |
| aterm-rle | 0.56.0 | Apache-2.0 | in-tree |
| aterm-scrollback | 0.56.0 | Apache-2.0 | in-tree |
| aterm-search | 0.56.0 | Apache-2.0 | in-tree |
| aterm-selection | 0.56.0 | Apache-2.0 | in-tree |
| aterm-shell-integration | 0.56.0 | Apache-2.0 | in-tree |
| aterm-sixel | 0.56.0 | Apache-2.0 | in-tree |
| aterm-tempfile | 0.56.0 | Apache-2.0 | in-tree |
| aterm-types | 0.56.0 | Apache-2.0 | in-tree |
| aterm-vi | 0.56.0 | Apache-2.0 | in-tree |
| bitflags | 1.3.2 | MIT/Apache-2.0 | in-tree (vendored) |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | in-tree (vendored) |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 | in-tree (vendored) |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | in-tree (vendored) |
| filedescriptor | 0.8.3 | MIT | in-tree (vendored) |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | in-tree (vendored) |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | in-tree (vendored) |
| indexmap | 2.14.0 | Apache-2.0 OR MIT | in-tree (vendored) |
| ioctl-rs | 0.1.6 | MIT | in-tree (vendored) |
| itoa | 1.0.18 | MIT OR Apache-2.0 | in-tree (vendored) |
| lazy_static | 1.5.0 | MIT OR Apache-2.0 | in-tree (vendored) |
| libc | 0.2.186 | MIT OR Apache-2.0 | in-tree (vendored) |
| log | 0.4.32 | MIT OR Apache-2.0 | in-tree (vendored) |
| memchr | 2.8.1 | Unlicense OR MIT | in-tree (vendored) |
| memoffset | 0.6.5 | MIT | in-tree (vendored) |
| nix | 0.25.1 | MIT | in-tree (vendored) |
| orca-daemon | 0.0.1 | MIT | in-tree |
| orca-net | 0.0.1 | MIT | in-tree |
| orca-pty | 0.0.1 | MIT | in-tree |
| orca-terminal | 0.0.1 | MIT | in-tree |
| orca-winpipe | 0.0.1 | MIT | in-tree |
| pin-utils | 0.1.0 | MIT OR Apache-2.0 | in-tree (vendored) |
| portable-pty | 0.8.1 | MIT | in-tree (vendored) |
| proc-macro2 | 1.0.106 | MIT OR Apache-2.0 | in-tree (vendored) |
| quote | 1.0.45 | MIT OR Apache-2.0 | in-tree (vendored) |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | in-tree (vendored) |
| regex | 1.12.3 | MIT OR Apache-2.0 | in-tree (vendored) |
| regex-automata | 0.4.14 | MIT OR Apache-2.0 | in-tree (vendored) |
| regex-syntax | 0.8.10 | MIT OR Apache-2.0 | in-tree (vendored) |
| serde | 1.0.228 | MIT OR Apache-2.0 | in-tree (vendored) |
| serde_core | 1.0.228 | MIT OR Apache-2.0 | in-tree (vendored) |
| serde_derive | 1.0.228 | MIT OR Apache-2.0 | in-tree (vendored, build-time proc-macro) |
| serde_json | 1.0.150 | MIT OR Apache-2.0 | in-tree (vendored) |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | in-tree (vendored) |
| serial | 0.4.0 | MIT | in-tree (vendored) |
| serial-core | 0.4.0 | MIT | in-tree (vendored) |
| serial-unix | 0.4.0 | MIT | in-tree (vendored) |
| serial-windows | 0.4.0 | MIT | in-tree (vendored) |
| shared_library | 0.1.9 | Apache-2.0/MIT | in-tree (vendored) |
| shell-words | 1.1.1 | MIT/Apache-2.0 | in-tree (vendored) |
| syn | 2.0.117 | MIT OR Apache-2.0 | in-tree (vendored) |
| termios | 0.2.2 | MIT | in-tree (vendored) |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | in-tree (vendored) |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | in-tree (vendored, build-time proc-macro) |
| toml | 0.8.23 | MIT OR Apache-2.0 | in-tree (vendored) |
| toml_datetime | 0.6.11 | MIT OR Apache-2.0 | in-tree (vendored) |
| toml_edit | 0.22.27 | MIT OR Apache-2.0 | in-tree (vendored) |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | in-tree (vendored) |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | in-tree (vendored) |
| web-time | 1.1.0 | MIT OR Apache-2.0 | in-tree (vendored) |
| winapi | 0.3.9 | MIT/Apache-2.0 | in-tree (vendored) |
| winnow | 0.7.15 | MIT | in-tree (vendored) |
| winreg | 0.10.1 | MIT | in-tree (vendored) |
| zmij | 1.0.21 | MIT | in-tree (vendored) |
| zstd | 0.13.3 | MIT | in-tree (vendored) |
| zstd-safe | 7.2.4 | MIT OR Apache-2.0 | in-tree (vendored) |
| zstd-sys | 2.0.16+zstd.1.5.7 | MIT/Apache-2.0 | in-tree (vendored) |

## 5. `orca_crypto_wasm_bg.wasm` — crate closure

Resolved dependency closure of `orca-crypto-wasm` for `wasm32-unknown-unknown`
(normal dependencies only), from `rust/orca-crypto-wasm/Cargo.lock`. This is
the sealed-payload cryptography module (`crypto_box`/`crypto_secretbox` over
`curve25519-dalek`); the generated `orca_crypto_wasm_bg.wasm` is vendored at
`src/renderer/src/lib/crypto-wasm/` and base64-embedded via
`src/shared/crypto-wasm/`. `curve25519-dalek` and `subtle` are offered under
BSD-3-Clause as their sole option — copyright lines and conditions are
reproduced in the appendix.

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| aead | 0.5.2 | MIT OR Apache-2.0 | crates.io |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates.io |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | crates.io |
| cipher | 0.4.4 | MIT OR Apache-2.0 | crates.io |
| console_error_panic_hook | 0.1.7 | Apache-2.0/MIT | crates.io |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | crates.io |
| crypto_box | 0.9.1 | Apache-2.0 OR MIT | crates.io |
| crypto_secretbox | 0.1.1 | Apache-2.0 OR MIT | crates.io |
| curve25519-dalek | 4.1.3 | BSD-3-Clause | crates.io |
| generic-array | 0.14.7 | MIT | crates.io |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | crates.io |
| inout | 0.1.4 | MIT OR Apache-2.0 | crates.io |
| js-sys | 0.3.85 | MIT OR Apache-2.0 | crates.io |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | crates.io |
| opaque-debug | 0.3.1 | MIT OR Apache-2.0 | crates.io |
| orca-crypto | 0.0.1 | MIT | in-tree |
| orca-crypto-wasm | 0.0.1 | MIT | in-tree |
| poly1305 | 0.8.0 | Apache-2.0 OR MIT | crates.io |
| proc-macro2 | 1.0.106 | MIT OR Apache-2.0 | crates.io |
| quote | 1.0.46 | MIT OR Apache-2.0 | crates.io |
| salsa20 | 0.10.2 | MIT OR Apache-2.0 | crates.io |
| subtle | 2.6.1 | BSD-3-Clause | crates.io |
| syn | 2.0.118 | MIT OR Apache-2.0 | crates.io |
| typenum | 1.20.1 | MIT OR Apache-2.0 | crates.io |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | crates.io |
| universal-hash | 0.5.1 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-macro | 0.2.108 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| wasm-bindgen-macro-support | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-shared | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| zeroize | 1.9.0 | Apache-2.0 OR MIT | crates.io |

## 6. `orca_git_wasm_bg.wasm` — crate closure

Resolved dependency closure of `orca-git-wasm` for `wasm32-unknown-unknown`
(normal dependencies only), from `rust/orca-git-wasm/Cargo.lock`; the
generated `orca_git_wasm_bg.wasm` is vendored at
`src/renderer/src/lib/git-wasm/`. The closure is the wasm build of in-tree
`orca-*` crates plus the wasm-bindgen web glue.

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| aho-corasick | 1.1.4 | Unlicense OR MIT | crates.io |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | crates.io |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | crates.io |
| console_error_panic_hook | 0.1.7 | Apache-2.0/MIT | crates.io |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | crates.io |
| futures-core | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-task | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| futures-util | 0.3.32 | MIT OR Apache-2.0 | crates.io |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | crates.io |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | crates.io |
| indexmap | 2.14.0 | Apache-2.0 OR MIT | crates.io |
| itoa | 1.0.18 | MIT OR Apache-2.0 | crates.io |
| js-sys | 0.3.85 | MIT OR Apache-2.0 | crates.io |
| memchr | 2.8.2 | Unlicense OR MIT | crates.io |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | crates.io |
| orca-agents | 0.0.1 | MIT | in-tree |
| orca-config | 0.0.1 | MIT | in-tree |
| orca-core | 0.0.1 | MIT | in-tree |
| orca-dispatch | 0.0.1 | MIT | in-tree |
| orca-flow-control | 0.0.1 | MIT | in-tree |
| orca-git | 0.0.1 | MIT | in-tree |
| orca-git-wasm | 0.0.1 | MIT | in-tree |
| orca-net | 0.0.1 | MIT | in-tree |
| orca-provider-backoff | 0.0.1 | MIT | in-tree |
| orca-relay | 0.0.1 | MIT | in-tree |
| orca-ssh | 0.0.1 | MIT | in-tree |
| orca-text | 0.0.1 | MIT | in-tree |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | crates.io |
| proc-macro2 | 1.0.106 | MIT OR Apache-2.0 | crates.io |
| quote | 1.0.46 | MIT OR Apache-2.0 | crates.io |
| regex | 1.12.4 | MIT OR Apache-2.0 | crates.io |
| regex-automata | 0.4.14 | MIT OR Apache-2.0 | crates.io |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | crates.io |
| serde | 1.0.228 | MIT OR Apache-2.0 | crates.io |
| serde_core | 1.0.228 | MIT OR Apache-2.0 | crates.io |
| serde_json | 1.0.150 | MIT OR Apache-2.0 | crates.io |
| slab | 0.4.12 | MIT | crates.io |
| syn | 2.0.118 | MIT OR Apache-2.0 | crates.io |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | crates.io |
| wasm-bindgen | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-futures | 0.4.58 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-macro | 0.2.108 | MIT OR Apache-2.0 | crates.io (build-time proc-macro) |
| wasm-bindgen-macro-support | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| wasm-bindgen-shared | 0.2.108 | MIT OR Apache-2.0 | crates.io |
| zmij | 1.0.21 | MIT | crates.io |

## Appendix — license texts

Full texts of every license that is the sole option for at least one crate
listed above (dual-licensed `... OR ...` crates are received under MIT or the
first-listed option, per the note at the top).

### MIT License

Applies to the MIT-licensed crates listed above; each crate's copyright line
is carried in its upstream repository (linked from its crates.io page).

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

### zlib License

Applies to `foldhash` (Copyright (c) 2024 Orson Peters) and `slotmap`
(Copyright (c) 2021 Orson Peters).

```
This software is provided 'as-is', without any express or implied warranty. In
no event will the authors be held liable for any damages arising from the use
of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it freely,
subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim
   that you wrote the original software. If you use this software in a
   product, an acknowledgment in the product documentation would be
   appreciated but is not required.

2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.
```

### ISC License

Applies to `libloading` (Copyright (c) 2015, Simonas Kazlauskas).

```
Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
```

### BSD 3-Clause License

Applies to the zstd C library statically linked by `zstd-sys`
(Copyright (c) Meta Platforms, Inc. and affiliates), and to the
`curve25519-dalek` and `subtle` crates compiled into
`orca_crypto_wasm_bg.wasm` (BSD-3-Clause is their sole license option;
their copyright lines and conditions follow the zstd text below).

The zstd text:

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

 * Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

 * Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

 * Neither the name Facebook, nor Meta, nor the names of its contributors may
   be used to endorse or promote products derived from this software without
   specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

The `curve25519-dalek` (Copyright (c) 2016-2021 isis agora lovecruft,
Copyright (c) 2016-2021 Henry de Valence) and `subtle` (Copyright (c)
2016-2017 Isis Agora Lovecruft, Henry de Valence, Copyright (c) 2016-2024
Isis Agora Lovecruft) text, reproduced from the crates' LICENSE files:

```
Copyright (c) 2016-2021 isis agora lovecruft. All rights reserved.
Copyright (c) 2016-2021 Henry de Valence. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

curve25519-dalek's LICENSE additionally carries the following notice for
portions originally derived from Adam Langley's Go ed25519 implementation
(https://github.com/agl/ed25519/), reproduced verbatim:

```
Copyright (c) 2012 The Go Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Unicode License v3

Applies to the Unicode Character Database tables embedded in `unicode-ident`
(`(MIT OR Apache-2.0) AND Unicode-3.0`) and to the UCD-derived tables in
`aterm-grapheme`.

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

### CC0 1.0 Universal

Applies to `hexf-parse`, which its author dedicated to the public domain
under CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/legalcode).
CC0 waives all copyright conditions and imposes no notice-preservation
requirement; the crate ships no license file of its own.

### Apache License, Version 2.0

Applies to the aterm engine crates (Copyright 2026 Andrew Yates) and to the
Apache-2.0-only crates listed above (`ab_glyph_rasterizer`,
`codespan-reporting`; neither ships a NOTICE file).

```
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright 2026 Andrew Yates

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
```

## Upstream Orca (MIT License)

Orca: ALab Edition is a downstream fork of
[Orca](https://github.com/stablyai/orca). Portions of this software derived
from upstream Orca are Copyright (c) 2026 Lovecast Inc. and distributed under
the MIT License, reproduced in full below as required by its terms:

```
MIT License

Copyright (c) 2026 Lovecast Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
