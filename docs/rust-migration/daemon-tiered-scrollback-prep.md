<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->
# Daemon-side prep for E1 tiered scrollback (Wave-3 3A integration note)

**Audience:** the Wave-3 integrator merging 3A (audit E1-modified: tiered
hot+warm store, single total retention limit, E10a byte watermark) with the
3B/3C daemon work. Written by the 3BC track (2026-07-22), which owns the daemon
subsystem this wave, so 3A does not have to re-derive the daemon context.

## Where the daemon constructs its terminal today (ring-only)

- `rpc.rs` `create_or_attach` builds the per-session engine with
  `HeadlessTerminal::with_scrollback(rows, cols, scrollback_rows(payload))`
  (`rust/crates/orca-daemon/src/rpc.rs:425`).
- `scrollback_rows()` is P4's validated forward of the user's
  `terminalScrollbackRows` setting: clamped to `[1_000, 50_000]`, absent or
  non-integer → `DEFAULT_SCROLLBACK` = 5000 (`rpc.rs:55`,
  `rust/crates/orca-terminal/src/headless.rs:46`).
- `HeadlessTerminal::with_scrollback` is ring-only:
  `TerminalBuilder::new().ring_buffer_size(limit.max(1)).build()` — the
  `(Some(ring), None)` builder arm → `Grid::with_scrollback`
  (`headless.rs:121`, `aterm-core/src/terminal/builder.rs:144`).
- Tiered attach is the `(_, Some(scrollback))` arms →
  `Grid::with_tiered_scrollback(rows, cols, ring_size, scrollback)`; a
  `TerminalBuilder::scrollback(Scrollback)` setter already exists
  (`builder.rs:106`).

## Invariants the tiered attach must keep (Codex-corrected E1 scope)

1. **One total retention limit, sourced from P4.** The P4-forwarded rows value
   is the user's whole budget. Today a tiered grid retains the store's
   `line_limit` **plus** the fixed hot ring
   (`aterm-grid/src/grid/accessors.rs:536` — "the limit governs the store; the
   ring stays the fixed-size hot tier"). The daemon must not ship
   `ring + store` > the P4 value: either 3A grows a real total-limit API in the
   engine (preferred) or the attach sizes `ring_size + store.line_limit()` to
   sum to `scrollback_rows(payload)`.
2. **Never inherit `DEFAULT_LINE_LIMIT`.** `Scrollback::new` defaults
   `line_limit` to 100,000 (`aterm-scrollback/src/lib.rs`, #7929) — double the
   50k policy cap (`rpc.rs:56`, `src/shared/terminal-scrollback-policy.ts`).
   The attach must `set_line_limit` explicitly from the P4 value.
3. **Native vs wasm tier split.** The daemon may use the zstd cold tier
   (compiled but unattached today); `aterm-wasm` excludes zstd/disk and is
   LZ4-only (`aterm-wasm/Cargo.toml:12`). Budgets and tier sizes tuned for the
   daemon must not be copied into the wasm ctor, and vice versa.
4. **Truncation/pressure surfaces out-of-band (E10a).** `Scrollback` already
   tracks `memory_budget` + yellow/red `watermark_level`
   (`aterm-scrollback/src/lib.rs:218`). When the daemon surfaces truncation or
   pressure to clients, it must be a protocol event — never sentinel text
   injected into the stream. P2's semantic writer queue is the natural carrier:
   `StreamItem::Event` is never coalesced and flushes pending data first
   (`orca-daemon/src/stream_coalescing.rs`), so a watermark event cannot
   reorder against session output.
5. **Protocol skew stays additive.** P4's skew contract (both directions —
   `src/main/daemon/terminal-scrollback-rows-protocol-skew.test.ts`,
   `rpc.rs` unit tests) must hold unchanged: pre-field clients keep 5k, junk
   clamps, nothing errors. Tiering is invisible on the wire until the
   watermark event lands, and that event needs its own absent-tolerant skew
   test in the same suite.

## Daemon-specific hazards to test before merge

- **Checkpoint decompression cost.** `serialize_ansi` is cached per
  (content-gen, cursor, row-cap) because the 5s/session checkpoint and
  reconnect paths walk grid+scrollback repeatedly (`headless.rs:107`). Under a
  flood the content-gen changes every checkpoint, so with a tiered store each
  checkpoint MISS walks warm (LZ4) and cold (zstd) tiers through decompression.
  Measure with the committed flood harness
  (`tools/benchmarks/daemon-flood-timed.mjs`, native mode) before/after attach;
  if checkpoint time regresses, bound the checkpoint's scrollback cap
  (`getSnapshot` already takes `scrollbackRows` opts) rather than the store.
- **`set_scrollback_text_only(true)`** (`headless.rs:127`) is a global flag
  tuned for the ring path. Verify the warm/cold codecs preserve its contract
  (text + OSC-8 hyperlink spans, no colour) or gate it off for tiered grids —
  `scrollback_row_text`/`scrollback_row_link_spans` parity tests ring-only vs
  tiered at identical content are the cheap oracle.
- **Per-process budget, N sessions.** `memory_budget` is per-`Scrollback`
  (per-session). The daemon hosts every pane in one process; a global budget
  needs registry-level accounting (sum of `total_memory_used()` across
  sessions) — decide in 3A whether Wave 3 ships per-session budgets only
  (documented) or the global cap. The Codex review requires the semantics to
  be explicit either way ("safe unlimited", no silent OOM).
- **P8 (Wave 5) cap raise.** `SCROLLBACK_ROWS_MAX = 50_000` in `rpc.rs:56` is
  the single Rust-side constant to bump when P8 raises the cap after E1 is
  proven; keep it in lockstep with `terminal-scrollback-policy.ts` (the 50k/
  100k inconsistency Codex flagged is tracked under P8, not here).

## Non-coupling note (P2)

The P2 writer-side coalescing operates on the stream plane
(`route_output` → `StreamItem` → per-socket writer). Tiered scrollback changes
the engine/control plane (`serialize_ansi`, `getSnapshot`). They meet only at
the watermark event (above) and in CPU contention on flood workloads — re-run
the flood bench after attach (numbers recorded in
`daemon-pty-drain-investigation.md`) to catch the latter.
