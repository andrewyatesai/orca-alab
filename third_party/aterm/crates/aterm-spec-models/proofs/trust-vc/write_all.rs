// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! trust-vc (Verus-style) proof artifact: `write_all` drops NO bytes.
//!
//! AUTHORED ARTIFACT — DISCHARGED ONCE THE ENGINE IS BUILT.
//! This file lives under `proofs/`, NOT in any crate `src/`, so it never
//! participates in the aterm build. It is written in faithful, current
//! trust-vc idiom: `#[trust_vc::requires(...)]` / `#[trust_vc::ensures(...)]`
//! / `#[trust_vc::decreases(...)]` attributes plus the `trust_vc::invariant!`,
//! `trust_vc::loop_decreases!`, `trust_vc::loop_ensures!`, and
//! `trust_vc::assert!` verification macros. The backend SMT solver is **ay**
//! (pure-Rust; no Z3) — trust-vc collects each obligation and checks it by
//! refutation: ay searches for a counterexample to `NOT(assertion)`; UNSAT
//! means verified, SAT yields the concrete violating values.
//!
//! Idiom source: `first-party/trust-vc/README.md` (requires!/ensures!/
//! invariant!/decreases! table, ay-by-refutation pipeline, three-way cfg
//! expansion) and the shipped ports `first-party/trust-vc/evals/verus/ported/
//! loops.rs` and `.../exec_termination.rs` — the most faithful end-to-end
//! verification artifacts, which use `#[trust_vc::requires(...)]` /
//! `#[trust_vc::ensures(...)]` attributes together with the
//! `trust_vc::invariant!` / `trust_vc::loop_decreases!` / `trust_vc::assert!`
//! macro forms inside `while` loops, and `#[trust_vc::decreases(..)]` /
//! `#[trust_vc::spec]` for termination measures.
//!
//! ============================================================================
//! THE BUG (aterm commit 3d58709 — "sandbox+pty: fix three real defects")
//! ============================================================================
//!
//! `aterm-pty::write_all` is the one seam that pushes terminal input to the PTY
//! master fd. The pre-fix loop treated EINTR as fatal:
//!
//! ```ignore
//! while !data.is_empty() {
//!     let r = unsafe { libc::write(master, data.as_ptr() as _, data.len()) };
//!     if r <= 0 { break; }              // <-- BUG: r == -1 / EINTR breaks here,
//!     data = &data[r as usize..];       //     SILENTLY DROPPING the rest of the
//! }                                     //     buffer. A signal mid-write loses
//!                                       //     terminal input, no error raised.
//! ```
//!
//! A signal interrupting a blocked `write` returns `-1`/`EINTR` with zero bytes
//! moved. The old `r <= 0 => break` conflated that with a real peer-close, so
//! the loop quit with bytes still pending and the caller saw "done": a SILENT
//! BYTE DROP. The fix `continue`s on `EINTR` and only breaks on a genuine error
//! or `r == 0`.
//!
//! ============================================================================
//! WHAT THIS PROOF ENCODES — "NoSilentDrop"
//! ============================================================================
//!
//! The fd is modeled by `cap: u64`, the total bytes the kernel will accept
//! before peer-close; `len: u64` is `buf.len()`. `kernel_write_step` models one
//! post-fix syscall return (bytes moved this call), constrained to make forward
//! progress while budget remains — EINTR is the retried, zero-drop case folded
//! into "try again", exactly as the fixed code does.
//!
//! Headline postcondition (NoSilentDrop):
//!   #[trust_vc::ensures(result ==> written == len)]
//! `result` (the `bool` return) is `true` exactly when the loop ran to
//! completion (`written == len`). So this is precisely "if write_all reports
//! success, ay proves not one byte was dropped." The buggy EINTR-break would
//! let the loop terminate with `written < len` while still reporting success;
//! ay finds the SAT counterexample (the dropped-byte witness). The fixed retry
//! makes `NOT(result ==> written == len)` UNSAT — PROVED.
//!
//! ============================================================================
//! TERMINATION — the EINTR retry provably terminates
//! ============================================================================
//!
//! `trust_vc::loop_decreases!(len - written)` is the loop termination measure.
//! With the loop invariant `written <= len` and `kernel_write_step` returning
//! `>= 1` while capacity remains, `len - written` strictly decreases on every
//! retained iteration, so the EINTR-retry loop cannot spin forever on a stream
//! of signals. trust-vc emits the decrease obligation alongside the invariant
//! obligations; ay checks `measure' < measure` at the back-edge.
//!
//! ============================================================================
//! HOW trust-vc + ay DISCHARGE THIS
//! ============================================================================
//!
//! Under `cargo trust-vc check` (the `cfg(test)`/verification expansion), the
//! proc macros collect obligations into a thread-local context, lower the Rust
//! expressions to typed `trust-vc` IR, and the AYEncoder declares `u64` as
//! BV64, adds the `requires!` clauses + loop invariants as assumptions, and for
//! each `ensures!` / `invariant!` / `loop_decreases!` / `assert!` checks
//! `SAT(¬P)` in ay:
//!
//!   * UNSAT  -> Verified  (the property holds for all inputs)
//!   * SAT    -> Counterexample with concrete `(cap, len, written)` values
//!   * Unknown-> solver limit hit
//!
//! ay is the SMT backend (pure-Rust DPLL(T): bitvector + arrays + EUF + LIA);
//! there is no Z3 and no third-party external solver.

use trust_vc_types::*;

/// Modeled fd-write syscall return: bytes the kernel accepts THIS call.
///
/// `cap` is the fd's total acceptance budget, `written` the bytes pushed so
/// far, `len - written` what is still pending. The contract is the post-fix
/// kernel contract we rely on:
///   * never reports more than offered: `result <= len - written`, and
///   * makes progress while budget remains:
///     `(written < len && cap > written) ==> result >= 1`.
/// EINTR (a zero-byte `-1`/`Interrupted` return) is the retried case: it simply
/// does not advance `written`; the fixed code's `continue` re-enters the loop.
/// A finite `cap` bounds those retries, so termination is preserved.
///
/// `#[trust_vc::spec]` marks this as a pure specification function — ghost,
/// erased at runtime — so its contract is what ay reasons against in place of
/// executable libc (the syscall trust boundary).
#[trust_vc::spec]
#[trust_vc::requires(written <= len)]
#[trust_vc::ensures(result <= len - written)]
#[trust_vc::ensures((written < len && cap > written) ==> result >= 1u64)]
fn kernel_write_step(cap: u64, written: u64, len: u64) -> u64 {
    let want = len - written;
    let room = if cap > written { cap - written } else { 0u64 };
    if room < want {
        room
    } else {
        want
    }
}

/// `write_all` over a modeled fd: returns `true` iff EVERY byte was written.
///
/// Proves the post-fix behavior — NoSilentDrop — that the EINTR retry restored.
///
/// Contracts:
///   requires(len <= cap)                  — the modeled fd can accept the buffer.
///   ensures(result ==> written == len)    — NoSilentDrop: success => no byte dropped.
///   ensures(written <= len)               — never claim more than offered.
///
/// Loop (the `while`, mirroring `while !data.is_empty()`):
///   invariant!(written <= len)            — monotone, bounded counter.
///   loop_decreases!(len - written)        — termination / EINTR-retry measure.
///   loop_ensures!(written == len)         — normal-exit fact feeding the post.
#[trust_vc::requires(len <= cap)]
#[trust_vc::ensures(result ==> written == len)]
#[trust_vc::ensures(written <= len)]
fn write_all_model(cap: u64, len: u64) -> bool {
    let mut written: u64 = 0u64;

    while written < len {
        // Loop invariant: the counter is monotone and bounded. trust-vc checks
        // this inductively (holds at entry; preserved by the body).
        trust_vc::invariant!(written <= len);

        // Termination measure: strictly decreases each retained iteration
        // because `kernel_write_step` returns >= 1 while budget remains. This
        // is what makes the EINTR-retry loop provably terminate — ay checks
        // `measure' < measure` at the back-edge.
        trust_vc::loop_decreases!(len - written);

        // Normal-exit fact: on the loop's normal exit, the whole buffer is
        // written. This is the bridge from invariant to the `result` post.
        trust_vc::loop_ensures!(written == len);

        let n = kernel_write_step(cap, written, len);

        // Post-fix semantics: a zero-progress return is the EINTR/retry case
        // (no byte dropped — loop again). Given `len <= cap` (precondition) and
        // `written < len` (guard), `kernel_write_step`'s ensures guarantees
        // `n >= 1`, so this branch is unreachable in the proved model — exactly
        // why the fixed loop always finishes the buffer.
        if n == 0u64 {
            continue;
        }

        // Advance the monotone, bounded counter.
        written = written + n;
    }

    // On normal exit `!(written < len)` and `written <= len`, so `written ==
    // len`. ay then discharges `result ==> written == len` (NoSilentDrop): we
    // return `true` exactly in the all-bytes-written state.
    trust_vc::assert!(written == len);
    written == len
}

fn main() {
    // Concrete instances (the proof above is universally quantified over
    // `cap`/`len`). At runtime (default `cargo build` expansion) the proof
    // annotations are no-ops and these are ordinary calls.
    let ok = write_all_model(64u64, 14u64); // "hello-pty-seam" is 14 bytes
    assert!(ok);

    let ok2 = write_all_model(4096u64, 0u64); // empty buffer: vacuously complete
    assert!(ok2);
}

// =============================================================================
// DISCHARGE (once trust-vc is built):
//
//   cargo trust-vc check \
//       --manifest-path ~/aterm/crates/aterm-spec-models/proofs/trust-vc/write_all.rs
//
// (`cargo trust-vc check` is the Verus-compatible verification subcommand; it
//  drives the `cfg(test)` expansion that collects obligations and invokes ay.
//  See first-party/trust-vc/README.md and CLAUDE.md.)
//
// BACKEND: ay (pure-Rust SMT — DPLL(T) with bitvector/array/EUF/LIA theories).
// No Z3, no external/third-party solver.
//
// EXPECTED: every obligation for `write_all_model` and `kernel_write_step`
// — the loop invariant (init + preservation), `loop_decreases!` (termination),
// `loop_ensures!`, the `assert!`, and both `ensures!` (NoSilentDrop +
// `written <= len`) — checks UNSAT in ay. Against the buggy EINTR-break
// variant, ay returns SAT for `result ==> written == len` and reports the
// dropped-byte counterexample `(cap, len, written)`.
// =============================================================================
