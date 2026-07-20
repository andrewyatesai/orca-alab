<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->
# Daemon PTY gather-drain: bringing aterm's flood win to the embedded terminal

**Date:** 2026-07-20. **Status:** landed. Ports aterm-gui's O_NONBLOCK
gather-drain (the on-glass cat-flood campaign that took native aterm 181 → 272
MB/s) into orca's **embedded** terminal, whose PTY path is separate from
aterm-pty.

## The gap

The native `aterm-gui` drain lives in the `aterm-pty` crate. Orca's embedded
terminal does NOT use it: the daemon (`orca-daemon`) owns PTYs via `orca-pty`
(a thin wrapper over the vendored `portable-pty`) and reads each session's
master in `pump_output` (`rust/crates/orca-daemon/src/rpc.rs`) with a **blocking
read loop** — one `read()` → UTF-8 decode → lock engine → `process` + record →
route, per read. macOS caps each master read at ~1 KiB, so this locksteps the
whole pipeline with the writer and pays the decode/engine-lock/record/route cost
**per ~1 KiB** instead of per batch. Measured drain-only ceiling: **~182 MB/s**.

## The graft (nix safe wrappers only — the workspace forbids `unsafe`)

- **`orca-pty`**: `clone_read_fd()` returns a `MasterReadFd` — an owned,
  `O_NONBLOCK`, `dup`'d handle over the master. Owning the dup keeps the master
  open-file-description alive for the pump's whole life, so the gather never
  reads/polls a recycled fd during a concurrent teardown. `gather_drain(fd, buf)`
  drains to `EAGAIN` into a 64 KiB batch with the aterm bridge (16 immediate
  re-reads then a 1 ms poll over the writer's µs refill gaps; `< 1 KiB` at first
  quiet delivers immediately for interactive echo; a batch is bounded by
  `min(64 KiB, 3 ms)`).
- **`write_all` is now `EAGAIN`-safe**: `O_NONBLOCK` lives on the *shared* OFD
  (portable-pty's `try_clone_reader`/`take_writer`/`as_raw_fd` are all dups of
  one fd), so it also affects terminal input. `write_all` parks in
  `poll(POLLOUT)` and retries — and retries `EINTR` (process-wide `SIGCHLD` fires
  whenever any child exits), matching the old blocking `write_all`'s
  Interrupted-retry so input is never silently truncated.
- **`orca-daemon::pump_output`**: unix now owns the `MasterReadFd`, gathers a
  batch, and runs the fan-out **once per batch** via an extracted `feed_batch`
  helper (byte-identical to the old per-read body — the VT parser is a streaming
  state machine, so a 64 KiB batch ≡ the per-1 KiB chunks it replaces). Windows
  keeps the blocking reader (ConPTY has no per-read cap, so gathering buys
  nothing there).

## Measured (quiet M5 Max, drain-only ceiling via `orca-pty`'s `drain_bench`)

| shape | MB/s |
|---|---|
| blocking (old pump) | 182 |
| poll-guarded (no O_NONBLOCK — considered, rejected) | 155 (worse) |
| **gather (this graft)** | **326 (1.79×)** |

The safe poll-guarded alternative was measured and **loses** (the poll-per-topup
+ 1 ms refill sleep — the same result aterm found): only the O_NONBLOCK gather
wins, which is why the shared-OFD write path had to become `EAGAIN`-safe. The
end-to-end pump number (drain + real `engine.process`, `orca-daemon`'s
`pump_bench`) is a serial single-thread combination of the drain and the engine;
a clean-machine reading is pending a quiet window (this is a shared host).

## Correctness (3-lens adversarial review, all SHIP_WITH_FIXES, fixes applied)

fd lifetime/teardown (single-owner RAII dup; kill/detach/EOF all wake the pump's
poll via child-death → master HUP), batched-feed equivalence (barrier scan,
UTF-8 boundary decode, engine-lock atomicity all preserved), interactive latency
(< 1 KiB first-quiet delivery unchanged). Fixes landed: `clone_read_fd` closes
the dup on every error path; `write_all` retries `EINTR` and fails loud on the
impossible no-fd case; a POLLHUP guard in the idle park. Tests: `orca-pty` 5/5
(incl. a multi-batch flood test asserting every byte + exactly-one-EOF),
`orca-daemon` 7/7, `parity:daemon` PASSED.

## Known follow-up (pre-existing, NOT introduced by this change)

`write_all`'s `poll(POLLOUT)` park is unbounded, and every call site holds the
global registry lock across it — so a child that wedges its own stdin can stall
the daemon. The old blocking `write_all` had the **identical** freeze under the
same lock (this change slightly reduces its likelihood via 64 KiB read
buffering), so it is not a regression. The structural fix — resolve the session
handle under the lock, then write outside it (mirroring `route_output`) — is
tracked separately.
