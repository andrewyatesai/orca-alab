//! Crash-recovery decision cores ported from `src/main/crash-reporting`.
//!
//! Two small deterministic state machines that gate Orca's automatic crash
//! recovery so a deterministic fault can't spin forever:
//!
//! - [`renderer_recovery`] — a rolling-window rate limiter on renderer auto-reloads
//!   (at most N reloads in any window W, else the breaker opens).
//! - [`gpu_fallback`] — a one-shot latch that engages software rendering after a
//!   burst of GPU-child crashes right after launch (engages at most once).
//!
//! Both take the clock as a plain integer argument (no timers, no IO) and use only
//! integer arithmetic, so the TS↔Rust parity is bit-exact. Same E1 pair as the
//! other decision-core crates: proven equivalent to the TS by the shared
//! `*-parity-corpus.txt` (a replayed operation trace), proven correct by
//! `proofs/ay/{rr,gf}_*.smt2`.

#![forbid(unsafe_code)]

pub mod gpu_fallback;
pub mod renderer_recovery;
