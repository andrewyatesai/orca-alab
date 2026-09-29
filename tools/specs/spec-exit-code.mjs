// Exit codes for the model-checked PTY/terminal specs.
//
// The problem this exists to fix: all four spec scripts SKIP when `ty` is absent
// (it ships with the atpkg-managed Trust toolchain, which not every machine has)
// and skipping exited 0. A run that model-checked NOTHING was indistinguishable
// from a run where every invariant held — including to `spec:protocols`, which
// chains two of them with `&&`.
//
// Skipping is still legitimate: a developer without the Trust toolchain should
// not be blocked. What is not legitimate is a skip that reads as a pass. So the
// skip keeps its own code, distinct from both success and a real refutation.
//
// Mirrors tools/terminal-bench/gauntlet-exit-code.mjs, which solved the identical
// problem for the gauntlet axes — same numbering on purpose.

/** Every invariant checked and held. */
export const SPEC_PASS = 0
/** A refutation, a broken negative control, or the checker itself failing. */
export const SPEC_FAIL = 1
/**
 * Nothing was proven — the model checker is unavailable, so no invariant was
 * evaluated. NOT a pass, and deliberately not 1 either: a caller that wants to
 * tolerate a missing toolchain can single out this code, which it cannot do if
 * the skip is dressed up as either outcome.
 */
export const SPEC_SKIPPED_NOTHING_PROVEN = 3

/**
 * Report a skip and exit. `label` is the spec's own prefix (e.g. `exit-spec`).
 *
 * ORCA_SPECS_ALLOW_SKIP=1 restores the historical exit 0 for environments that
 * genuinely cannot install the checker and still need a green chain. It is an
 * explicit, greppable opt-out rather than the silent default it used to be.
 */
export function skipNothingProven(label, reason) {
  const tolerated = process.env.ORCA_SPECS_ALLOW_SKIP === '1'
  const tail = tolerated ? ' (ORCA_SPECS_ALLOW_SKIP=1, exiting 0)' : ''
  console.log(
    `[${label}] SKIP — ${reason}\n[${label}] nothing was model-checked; this is NOT a pass${tail}`
  )
  process.exit(tolerated ? SPEC_PASS : SPEC_SKIPPED_NOTHING_PROVEN)
}
