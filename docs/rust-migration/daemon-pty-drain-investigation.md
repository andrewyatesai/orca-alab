<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->
# Embed cat-flood: the bottleneck is the socket fan-out, not the PTY drain

**Date:** 2026-07-20. **Status:** RESOLVED. The drain-focused gather graft was
prototyped, measured against the read+engine loop, and **REVERTED**
(`b78a5d0ff` → `548b56997`); an external gpt-5.6/ultra review reproduced the
numbers, confirmed the revert, and redirected me to measure the *full* path. A
receiver-timed end-to-end measurement then found the real bottleneck is the
**socket fan-out** (~152 MB/s full-path vs 337 read+engine), and **coalescing
`route_output` frames lifts the embed flood +58% (156 → 248 MB/s)** with the
fast blocking drain untouched. See "RESOLVED" below. The `ORCA_PUMP_FRAME_KIB`
knob is a default-off instrument pending the interactive-safe flush + review.

## Hypothesis

Native `aterm-gui`'s cat-flood campaign lifted on-glass throughput 181 → 272
MB/s by replacing a blocking PTY read with an O_NONBLOCK **gather** (drain to
EAGAIN into a 64 KiB batch, bridge refill gaps, hand off once per batch). Orca's
**embedded** terminal doesn't share that code: its daemon (`orca-daemon`) reads
each session's master in `pump_output` (`rpc.rs`) with a blocking loop. Porting
the gather looked like a free ~2× win.

## Measurement (quiet M5 Max, `cat` of a 524 MB ASCII corpus, tight variance)

| | blocking (current pump) | gather (aterm shape) |
|---|---|---|
| **drain only** (no engine) | 175 MB/s | **372 MB/s** |
| **+ real `engine.process`** (the actual pump) | **337 MB/s** | 240 MB/s |

The drain-only number says gather wins 2.1×. On this read+engine loop the
gather **LOSES 1.45×**, stable across runs.

**Scope (corrected after an external gpt-5.6 review, which independently
reproduced these numbers):** this proves that *this particular unconditional
64 KiB inline gather* regresses *the narrow `read + HeadlessTerminal::process`
loop* on one macOS cat-flood. It does **not** establish that the gather loses
the *full* production `pump_output`, nor that the pump is not the embed
bottleneck. The bench (`pump_bench.rs`) omits the per-read UTF-8 decode + String
alloc (`rpc.rs:766`), the barrier scan (`rpc.rs:778`), the engine-mutex +
pending-record (`rpc.rs:798`), and the frame alloc + channel send of
`route_output` (`registry.rs:220`) — precisely the *fixed-per-handoff* costs a
gather **amortizes**. The 337→240 gap is only ~1.23 µs per 1 KiB read; the
omitted work could plausibly exceed that. Direction on the full path is
**unknown** and untested. See "Open question" below.

## Why the microbench inverts (mechanism, corrected)

Note blocking goes **175 → 337** when the engine is added — adding work made it
*faster*. The cause is producer/consumer **overlap through the kernel PTY
queue**:

- **Blocking loop** = `read(≈1 KiB)` → `process(1 KiB)` → repeat. While the
  engine processes one chunk, `cat` refills the small PTY queue on another core,
  so the *next* read is usually ready immediately (queue-mediated pacing — the
  read rarely has to park). Read and engine **overlap** → elapsed time ≈
  `max(drain, engine)` per unit, i.e. throughput ≈ the **slower** stage's rate.
- **Gather loop** = drain-to-EAGAIN into 64 KiB (`cat` blocked while draining) →
  `process(64 KiB)`. This **serializes** drain-then-process in the one thread →
  elapsed ≈ `drain + engine`, i.e. throughput ≈ the **harmonic** combination.

Independently measured stage rates (gpt-5.6 rerun): gather-drain ≈ 378 MB/s,
parser-only ≈ 779 MB/s. The models predict gather = `1/(1/378 + 1/779) ≈ 255`
(observed 245, ~4%) and blocking = `min(378, 779) ≈ 378` (observed 352, ~7%) —
strong agreement, so **serialization is very likely the primary cause**. Two
corrections to an earlier draft of this doc: (1) the engine is **not** the
slower stage (isolated ~779 > drain ~378) — its cost is what *supplies the
pause* that lets `cat` refill; (2) the win is queue-mediated overlap, not the
read "yielding its core" (on multicore that isn't needed).

**aterm-gui wins with the gather because it has a SEPARATE parse thread** (drain
∥ parse across threads — `aterm-gui/src/spawn.rs:1607,1850`, plus a
parser-in-flight–aware bridge, `aterm-pty/src/unix.rs:1762`). Orca's daemon pump
is single-threaded, so *inline* batching serializes. The drain-only ceiling is a
misleading proxy for a single-threaded loop.

## Consequences

- **Reverted the graft** — correctly: it was justified mainly by the drain-only
  number, changed *every* Unix host, made the shared-OFD master writer
  nonblocking (needing a new EAGAIN-safe write path), and the one
  engine-inclusive reading regressed. Low-effort/uncertain — revert, don't ship.
- **"Pump is not the bottleneck" is NOT established.** 337 MB/s is only the
  `read + engine` loop (~91% of the 378 drain ceiling); it excludes the socket
  completion, the renderer worker, and presentation. Comparing it to aterm-gui's
  272 *on-glass* is apples-to-oranges.
- **Downstream suspects (corrected):** NOT a SAB ring — the design explicitly
  rejected `SharedArrayBuffer` (`docs/rust-migration/aterm-single-engine-worker.md:104`).
  The real candidates, unranked: the renderer worker parse/render/present path;
  **production's small-frame rate** — `route_output` enqueues ~one channel item +
  socket frame *per ~1 KiB read* (`rpc.rs:813`), and the v1020 socket bench that
  reports 777/1214 MB/s sends 64 KiB chunks (`stream-throughput-bench.rs:26`), so
  it never exercises the ~1 KiB-frame-rate path production actually hits; then
  the pump loop. Batching would *help* that downstream frame rate even if it
  slightly hurts the local loop — which is exactly why the question is open.

## Open question (what the microbench did NOT answer)

Does a **small, bounded, alternating gather** (e.g. 4–8 KiB: gather a little,
process, gather again — NOT sub-chunking an already-drained 64 KiB, which
restores no overlap because the bytes already left the kernel) beat blocking on
the **full** production path? A small quantum keeps producer overlap while
amortizing the per-frame decode/lock/route/channel/socket costs the microbench
omits — and coalesces the ~1 KiB frame rate. A full drain∥engine thread split is
a separate lever, but its ideal ceiling is only ~378 (~7–10% over 337) before
channel/recycle overhead, so it likely isn't worth the per-session machinery
unless the full-path fanout shows large-batch gains.

### RESOLVED: the bottleneck is the socket FAN-OUT, and coalescing frames wins

I built the receiver-timed harness the review recommended
(`scratchpad/daemon-flood-timed.mjs`: launch the real daemon, attach a v1020
binary stream, flood `cat`, time until the client consumes+verifies the last
byte). Results (quiet M5 Max, 524 MB corpus, 5 trials, tight):

- **Full production path, current per-read routing: ~152–156 MB/s.** That is
  *less than half* the read+engine loop's 337 — so the socket fan-out
  (`route_output` → channel → writer thread → v1020 frame per ~1 KiB read), NOT
  the drain or the engine, is the real bottleneck. The drain was a red herring.
- **Coalescing `route_output` into ~N KiB frames** (bench knob
  `ORCA_PUMP_FRAME_KIB`, `rpc.rs`; keeps the fast blocking read+engine loop
  untouched — no O_NONBLOCK, no write-path change):

  | frame KiB | 0 | 4 | 8 | 16 | 32 | 64 |
  |---|---|---|---|---|---|---|
  | MB/s | 156 | 174 | 176 | 212 | **248** | 246 |

  **~32 KiB lifts the embed's end-to-end flood +58% (156 → 248)**, recovering
  most of the gap toward the 337 read+engine ceiling. This is the opposite lever
  from the reverted gather: keep the fine-grained drain (it already pipelines),
  and cut the DOWNSTREAM frame rate.

**Shippable form (next):** an interactive-safe version must flush the coalesced
buffer at the size cap OR when the burst pauses, so a prompt/echo isn't delayed
until the cap fills. The clean way (no O_NONBLOCK, so no write-path change):
`poll(master_fd, POLLIN, 0)` before each blocking read — if not ready, the burst
ended, flush now (echo delivers immediately); during a flood it fills the cap
and flushes by size. macOS main already batches PTY output on a ~2 ms / 16 KiB
window (`src/main/ipc/pty.ts:1684,2484`), so a bounded daemon-side coalesce adds
no more perceptible latency than already exists. `ORCA_PUMP_FRAME_KIB` stays a
default-off instrument until that interactive-safe flush + a review land.

## Reproduce

`rust/crates/orca-daemon/examples/pump_bench.rs` (self-contained; measures
blocking vs an inlined gather feeding a real `HeadlessTerminal`):

```
cd rust && rustup run trust cargo run --release -p orca-daemon \
  --example pump_bench -- /tmp/atbench/flood_500.vt
```

Run on a QUIET machine (loadavg < ~3): under contention the gather's bounded
busy-reads steal cores from `cat` and the numbers invert unreliably.

Caveats (this bench is a *narrow* proxy, not the production pump): it runs 3
blocking then 3 gather trials with no interleave/warmup (should be ABBA); it
does not consume the terminal state (add a grid hash / `black_box` under release
LTO); `assert!(total >= bytes)` doesn't verify byte-integrity; and the CRLF
corpus through portable-pty's default termios pays `OPOST`/`ONLCR` on both arms
(biases absolutes, not the ratio). It deliberately omits the decode / lock /
record / `route_output` / socket work — the whole point of "Open question".
