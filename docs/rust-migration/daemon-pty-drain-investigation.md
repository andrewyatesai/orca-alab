<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->
# Embed cat-flood: the bottleneck is the socket fan-out, not the PTY drain

**Date:** 2026-07-20. **Status:** RESOLVED. The drain-focused gather graft was
prototyped, measured against the read+engine loop, and **REVERTED**
(`b78a5d0ff` → `548b56997`); an external gpt-5.6/ultra review reproduced the
numbers, confirmed the revert, and redirected me to measure the *full* path. A
receiver-timed end-to-end measurement then found the real bottleneck is the
**socket fan-out** (~152 MB/s full-path vs 337 read+engine), and **coalescing
`route_output` frames lifts the embed flood +47–58%** with the fast blocking
drain untouched. Two reviews (workflow + gpt-5.6/ultra) rejected default-on-ing
the *pump-side* coalescing (frame reorder + reattach duplication); the correct
form batches in the connection writer (recipient bound per-read) — to build +
review on its own. The `ORCA_PUMP_FRAME_KIB` knob stays default-off (proves the
+58%). See "Shippable form" below.

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

**Shippable form — TWO reviews (adversarial workflow + gpt-5.6/ultra) both said
DO-NOT-default-ON the pump-side design; the correct fix is writer-side.** The
size-cap knob (`ORCA_PUMP_FRAME_KIB`, default 0) stays a default-off instrument
that proves the +58% ceiling. What was tried and why it's not the shipping form:

- **Pump-side per-session flush timer (built, measured +47%, then REJECTED).** A
  second thread flushing a shared `Arc<Mutex<String>>` every 2 ms REORDERS
  frames: both flush sites `mem::take` under the lock but call `route_output`
  *outside* it, so a later prefix can reach the client's channel before an
  earlier one → terminal corruption (both reviewers, independently — a blocker).
  Reworking it to a single condvar-driven emitter fixes ordering, but the deeper
  flaw remains: batching in the pump DEFERS recipient selection, so a reattaching
  client gets pre-snapshot bytes in *both* its snapshot AND its deferred live
  tail (no seq/watermark to dedup — `pty-connection.ts:7346`, `protocol.rs:114`)
  — a duplication the reattach test's contract forbids. Plus a thread/session and
  a control-heavy-TUI latency risk (a redraw coalesced past main's 16 KiB
  immediate-path threshold pays main's extra 2 ms — `pty.ts:2022`).
- **In-loop `poll(fd, POLLIN, timeout)` burst-flush: REJECTED** — dropped the
  flood to ~137 MB/s (a poll per read can't tell a flood's ~1 KiB refill gap from
  a real pause).

**The correct design (gpt-5.6/ultra, to implement): batch in the connection
writer, not the pump.** `route_output` binds recipients *immediately* per read
(so NO reattach dup) and enqueues a *semantic* `Data{session, text}` item; the
existing per-client writer thread (`connection.rs::spawn_stream_drain`) owns the
coalescing — `recv_timeout(2 ms)`, concatenate adjacent same-session data to
~32 KiB, flush before control/exit events, block indefinitely when idle. That is
one emitter (order-safe), zero new threads, zero idle polling, and no snapshot/
stream duplication. Requires changing `StreamSender`/the channel item from
pre-encoded frames to the semantic enum + moving encoding into the writer — a
contained streaming-layer change, but on every terminal's live-output path, so
it gets built + reviewed on its own rather than rushed.

**SHIPPED (2026-07-21, P2):** `stream_coalescing.rs` implements exactly this —
`route_output` enqueues semantic `StreamItem::Data`/`Event` per read (recipients
bound immediately), and each socket's writer thread encodes per its negotiated
format and merges adjacent same-session data to 32 KiB via `try_recv`-only
coalescing (an empty queue flushes immediately — zero added interactive
latency; the doc's 2 ms `recv_timeout` was dropped for that reason). Events are
never merged and flush all pending data first, both formats. Measured with
`examples/stream_flood_bench.rs` (end-to-end serve() + real client, 500 MB
corpus) on a LOADED M-series host (loadavg 7–10, not the quiet-machine
condition above): NDJSON 109 → 133 MB/s mean (+22%; medians 103 → 140), binary
135 → 140 MB/s (within noise). Writer-side coalescing only engages when the
socket writer is the bottleneck — under host contention the ~1 KiB PTY reads
are, so the quiet-machine +58% ceiling was not reproducible here and remains
unclaimed for the writer form.

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
