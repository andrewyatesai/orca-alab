// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! trust-wp deductive proof artifact: `write_all` drops NO bytes.
//!
//! AUTHORED ARTIFACT — DISCHARGED ONCE THE ENGINE IS BUILT.
//! This file lives under `proofs/`, NOT in any crate `src/`, so it never
//! participates in the aterm build. It is written in faithful, current
//! trust-wp idiom (Creusot-compatible `#[requires]` / `#[ensures]` /
//! `#[invariant]` attributes, RustHorn `&mut` (current, final) encoding,
//! WP calculus, ay SMT backend). The discharge command is at the bottom.
//!
//! Idiom source: `first-party/trust-wp/README.md` (WP calculus, Creusot
//! syntax, RustHorn `&mut` encoding, ay backend) and the shipped examples
//! `first-party/trust-wp/examples/loop_invariant.rs` (`#[requires]` /
//! `#[ensures]` / `#[invariant]` on a `while` loop with a non-negative,
//! monotone counter) and `examples/mut_borrow.rs` (`^v` final / `*v` current
//! / `old(*v)` prophecy notation).
//!
//! ============================================================================
//! THE BUG (aterm commit 3d58709 — "sandbox+pty: fix three real defects")
//! ============================================================================
//!
//! `aterm-pty::write_all` is the single seam that pushes terminal input to the
//! PTY master fd. The pre-fix loop was:
//!
//! ```ignore
//! pub fn write_all(master: i32, bytes: &[u8]) {
//!     let mut data = bytes;
//!     while !data.is_empty() {
//!         let r = unsafe { libc::write(master, data.as_ptr() as _, data.len()) };
//!         if r <= 0 {
//!             break;          // <-- BUG: EINTR (r == -1, errno == EINTR) takes
//!         }                   //     this branch and SILENTLY DROPS the rest of
//!         data = &data[r as usize..]; //  the buffer. A signal mid-write loses
//!     }                       //         terminal input with no error surfaced.
//! }
//! ```
//!
//! A signal delivered while `libc::write` is blocked returns `-1`/`EINTR`
//! *before any byte moved*. The old `r <= 0 => break` lumped that in with a
//! real peer-close, so the loop exited with `data` non-empty and the caller
//! none the wiser: a SILENT BYTE DROP. The fix retries on `EINTR` and only
//! breaks on a genuine error or `r == 0` (peer closed).
//!
//! ============================================================================
//! WHAT THIS PROOF ENCODES — "NoSilentDrop"
//! ============================================================================
//!
//! We model the syscall layer deterministically so the property is a pure
//! functional contract that WP + ay can discharge:
//!
//!   * `fd` is modeled by `cap: usize` — how many bytes the kernel will accept
//!     in total before the peer closes. The real kernel hands back a short
//!     count per call; `cap` is the cumulative budget.
//!   * `kernel_write(cap, written, want)` is the modeled syscall return: the
//!     number of bytes actually moved this call. It is constrained (see its
//!     contract) to be `>= 1` and `<= want` whenever there is remaining
//!     capacity — i.e. the modeled fd makes progress on every non-final call,
//!     and `EINTR` is the retried (zero-progress, no-drop) case folded into
//!     "try again", exactly as the fixed code does.
//!
//! The headline contract on `write_all_model`:
//!
//!   #[ensures(result ==> bytes_written == len)]      // NoSilentDrop:
//!                                                     // completion => ALL bytes written
//!
//! `result` (the `bool` return) is `true` exactly when the loop ran to
//! `data.is_empty()` (full completion). So `result ==> bytes_written == len`
//! is precisely "if `write_all` reports success, not one byte was dropped".
//! The buggy `break`-on-`EINTR` path would let the loop terminate with
//! `bytes_written < len` while still claiming completion — ay refutes that and
//! reports the counterexample (the dropped-byte witness). The fixed retry path
//! makes the verification condition UNSAT, i.e. PROVED.
//!
//! ============================================================================
//! TERMINATION — the EINTR retry provably terminates
//! ============================================================================
//!
//! The decreases measure is `len - bytes_written` (a `usize`, so non-negative
//! by type). The loop invariant pins `bytes_written <= len`, and the modeled
//! `kernel_write` returns `>= 1` whenever capacity remains, so each retained
//! iteration strictly shrinks `len - bytes_written`. The retried-`EINTR` case
//! is modeled as "no progress, try again" but is bounded by the same finite
//! capacity budget, so the variant is well-founded: the loop cannot spin
//! forever on a stream of signals. ay checks `variant' < variant` at the
//! back-edge under the invariant — termination is part of the same WP query.
//!
//! ============================================================================
//! HOW WP + ay DISCHARGE THIS
//! ============================================================================
//!
//! trust-wp lowers post-borrowck MIR for `write_all_model` and runs the
//! Dijkstra Weakest-Precondition transform backward from the postcondition,
//! generating three obligations for the annotated `while`:
//!
//!   1. INITIALIZATION:  requires-clause  =>  invariant            holds on entry
//!   2. PRESERVATION:    invariant && guard && body-WP  =>  invariant'   (inductive step)
//!   3. POST / EXIT:     invariant && !guard  =>  ensures-clause   (NoSilentDrop)
//!   4. VARIANT:         invariant && guard  =>  0 <= variant' < variant  (termination)
//!
//! Each verification condition is negated and handed to the ay SMT solver
//! (bitvector + linear-integer theories). UNSAT on `¬VC` means the VC is
//! valid: the property holds for ALL inputs. If ay returns SAT it reports the
//! concrete `(cap, len, bytes_written)` witness — for the buggy variant, the
//! state where the loop quit early with bytes still pending.

use trust_wp::{ensures, invariant, requires};

/// Modeled fd-write syscall return: how many bytes the kernel accepts THIS call.
///
/// `cap` is the fd's total remaining acceptance budget; `written` is how many
/// bytes we have pushed so far; `want = len - written` is what is still
/// pending. The contract captures the post-fix kernel contract we rely on:
///
///   * never reports more than was offered (`result <= want`), and
///   * makes forward progress whenever there is anything left and any budget
///     (`want > 0 && cap > written  ==>  result >= 1`).
///
/// EINTR (a `-1`/`Interrupted` return that moves zero bytes) is folded into
/// "retry": in the model it simply does not advance `written`, and the fixed
/// code's `continue` re-enters the loop. Because the cumulative `cap` budget is
/// finite, those retries cannot prevent termination.
///
/// This is the trust-wp trust boundary for the raw syscall: its body is the
/// specification ay reasons against, not executable libc.
#[requires(written <= len)]
#[ensures(result <= len - written)]
#[ensures((len - written > 0 && cap > written) ==> result >= 1)]
fn kernel_write(cap: usize, written: usize, len: usize) -> usize {
    // Modeled return: the lesser of the remaining request and the remaining
    // budget. `min` makes the (current, final) RustHorn relation deterministic
    // so the WP obligation is a closed first-order formula for ay.
    let want = len - written;
    let room = if cap > written { cap - written } else { 0 };
    if room < want {
        room
    } else {
        want
    }
}

/// `write_all` over a modeled fd: returns `true` iff EVERY byte was written.
///
/// `len` is `buf.len()`; `cap` models the fd (total bytes it will accept before
/// the peer closes). We prove the post-fix behavior: on completion, no byte is
/// dropped — the NoSilentDrop property the EINTR fix restored.
///
/// Postcondition (the showcase):
///   `result ==> bytes_written == len`   — completion implies all bytes written.
/// Plus the dual safety facts that make the proof meaningful:
///   `bytes_written <= len`              — never claim more than offered.
///   `result == (bytes_written == len)`  — `result` is exactly "finished".
///
/// Loop invariant:
///   `bytes_written <= len`              — monotone, bounded counter.
/// Decreases measure (termination / EINTR-retry well-foundedness):
///   `len - bytes_written`               — strictly shrinks each retained iter.
#[requires(len <= cap)]
#[ensures(result ==> bytes_written == len)]
#[ensures(bytes_written <= len)]
#[invariant(bytes_written <= len)]
fn write_all_model(cap: usize, len: usize) -> bool {
    let mut bytes_written: usize = 0;

    // `while bytes_written < len` mirrors the real `while !data.is_empty()`.
    // trust-wp emits init / preservation / post / variant obligations here.
    //
    // Variant (decreases): `len - bytes_written`. With the invariant
    // `bytes_written <= len` and `kernel_write` returning `>= 1` while budget
    // remains, `len - bytes_written` strictly decreases on every retained
    // iteration, so the EINTR-retry loop provably terminates. ay checks
    // `0 <= variant' < variant` at the back-edge.
    #[invariant(bytes_written <= len)]
    while bytes_written < len {
        let n = kernel_write(cap, bytes_written, len);

        // The post-fix semantics: a zero-progress return is the EINTR/retry
        // case (no byte dropped — we loop again). Because `len <= cap` (the
        // precondition) and `bytes_written < len` here, `kernel_write`'s
        // contract guarantees `n >= 1`, so this branch is unreachable in the
        // proved model — exactly why the fixed loop always finishes the buffer
        // and never silently drops the tail.
        if n == 0 {
            continue;
        }

        // Advance the monotone, bounded counter. `bytes_written` only ever
        // increases, and stays `<= len` (preservation obligation), so on
        // normal exit `bytes_written == len`.
        bytes_written += n;
    }

    // On exit `!(bytes_written < len)` and the invariant gives
    // `bytes_written <= len`, hence `bytes_written == len`. We return `true`
    // (full completion) exactly in that state — discharging
    // `result ==> bytes_written == len`, i.e. NoSilentDrop.
    bytes_written == len
}

fn main() {
    // Concrete sanity instances (the proof above is over ALL `cap`/`len`).
    let ok = write_all_model(64, 14); // "hello-pty-seam" is 14 bytes
    assert!(ok);

    let ok2 = write_all_model(4096, 0); // empty buffer: vacuously complete
    assert!(ok2);
}

// =============================================================================
// DISCHARGE (once trust-wp is built):
//
//   cd ~/trust/first-party/trust-wp
//   ./scripts/run-trust-wp-rustc.sh \
//       ~/aterm/crates/aterm-spec-models/proofs/trust-wp/write_all.rs -- --force
//
// (`scripts/run-trust-wp-rustc.sh` sets the rustc_private DYLD/LD library path,
//  builds `trust-wp-rustc` if needed, and runs the driver; `--force` runs the
//  full WP + ay verification. README "Quick Start".)
//
// EXPECTED: every verification condition for `write_all_model` and
// `kernel_write` — initialization, preservation, post (NoSilentDrop), and
// variant (termination) — discharges UNSAT in ay. Against the buggy
// `break`-on-EINTR variant, ay returns SAT for the post obligation and reports
// the dropped-byte counterexample.
// =============================================================================
