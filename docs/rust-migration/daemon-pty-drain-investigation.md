<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->
# Daemon PTY drain: why the aterm gather-drain does NOT port to the embed

**Date:** 2026-07-20. **Status:** investigated, prototyped, **REJECTED and
reverted** (`b78a5d0ff` → reverted). The knowledge is the deliverable.

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

The drain-only number says gather wins 2.1×. The **engine-inclusive** number —
the one that matters — says gather **LOSES 1.45×**. Confirmed across 5 quiet
runs; the gap is stable and tight, not noise.

## Why (the load-bearing insight)

Note that blocking goes **175 → 337** when the engine is added — adding work
made it *faster*. That is the whole story:

- **Blocking loop** = `read(≈1 KiB)` → `process(1 KiB)` → repeat. While the
  engine processes one chunk, `cat` produces the next **in parallel** (separate
  core, buffered by the kernel PTY queue). The blocking read parks (yields the
  core to `cat`). Producer and consumer **overlap** → throughput ≈
  `max(produce, consume)`, and the "1 KiB lockstep" penalty that motivated the
  aterm campaign is *hidden* behind the per-chunk processing.
- **Gather loop** = drain-to-EAGAIN into 64 KiB (busy-reading, keeping the queue
  empty, `cat` blocked) → `process(64 KiB)` (engine runs, `cat` refills). The
  batch **serializes** drain-then-process in the one daemon thread → throughput
  ≈ `produce + consume`. The faster drain is irrelevant because the engine, not
  the drain, is the bottleneck, and batching destroyed the free pipelining.

**aterm-gui wins with the gather because it has a SEPARATE parse thread** (the
gather thread hands 64 KiB batches to a parse thread over a channel — drain ∥
parse across two threads). Orca's daemon pump is **single-threaded** (drain and
engine in one thread), so the identical batching regresses. The drain-only
ceiling is a **misleading proxy** for a single-threaded, engine-bound pump.

## Consequences

- **Reverted the graft.** The blocking pump stays; the deployed daemon is the
  fast path (~337 MB/s daemon-side, single-user — already faster than native
  aterm-gui's 272 on its different on-glass axis).
- **The embed's daemon pump is NOT the flood bottleneck.** If the embedded
  terminal ever feels slow under a flood, the cost is downstream — the socket
  frame plane, the renderer, or the SAB ring — not the PTY drain. Measure those
  before touching the pump.
- **If the pump ever IS the bottleneck**, the correct lever is a **drain ∥
  engine thread split** (aterm's #13209 shape), which restores overlap — NOT
  batching in one thread. Even then, payoff is uncertain given the blocking loop
  already pipelines to ~337.

## Reproduce

`rust/crates/orca-daemon/examples/pump_bench.rs` (self-contained; measures
blocking vs an inlined gather feeding a real `HeadlessTerminal`):

```
cd rust && rustup run trust cargo run --release -p orca-daemon \
  --example pump_bench -- /tmp/atbench/flood_500.vt
```

Run on a QUIET machine (loadavg < ~3): under contention the gather's bounded
busy-reads steal cores from `cat` and the numbers invert unreliably.
