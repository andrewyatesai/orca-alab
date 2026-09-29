#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Solver resolution for the orca-git proof bundles (sourced, not executed).
# `ay` (the Trust SAT/SMT solver) is the toolchain of record. AY_SOLVER=z3 (or
# the aggregator's --solver z3) re-checks the same bundles with stock z3 as an
# OPT-IN independent portability check — it never replaces ay for the record.
#
# Sets SOLVER_KIND (ay|z3) and SOLVER_BIN (empty when nothing runnable), and
# defines solve_verdict <file> → first bare sat/unsat/unknown line, lowercased.

# ay ladder: $AY → PATH (the atpkg-managed Trust toolchain). Never ~/trust/build
# — the compiler repo's exclusive build output, whose ay goes stale. An explicit
# $AY that does not run is FATAL (below), never a fall-through to another solver.
resolve_ay() {
  local c
  for c in \
    "${AY:-}" \
    "$(command -v ay 2>/dev/null || true)"; do
    if [ -n "$c" ] && "$c" --version >/dev/null 2>&1; then
      echo "$c"
      return 0
    fi
  done
  return 1
}

SOLVER_KIND="${AY_SOLVER:-ay}"
case "$SOLVER_KIND" in
  ay)
    if [ -n "${AY:-}" ] && ! "$AY" --version >/dev/null 2>&1; then
      echo "FATAL: \$AY='$AY' is not a runnable ay"
      exit 1
    fi
    SOLVER_BIN="$(resolve_ay || true)"
    ;;
  z3) SOLVER_BIN="${Z3:-$(command -v z3 2>/dev/null || true)}" ;;
  *)
    echo "FATAL: unknown AY_SOLVER='$SOLVER_KIND' (expected ay or z3)"
    exit 1
    ;;
esac
if [ "$SOLVER_KIND" = "z3" ] && [ -n "$SOLVER_BIN" ] && ! "$SOLVER_BIN" --version >/dev/null 2>&1; then
  SOLVER_BIN=""
fi

solver_banner() {
  echo "solver = $SOLVER_BIN ($SOLVER_KIND)"
  if [ "$SOLVER_KIND" = "z3" ]; then
    echo "  independent portability check — ay remains the toolchain of record"
  fi
}

# Every orca-git bundle is QF_BV check-sat, where ay and z3 verdicts agree 1:1.
# A CHC/HORN bundle must NOT reuse this unmapped: under a fixedpoint engine
# sat = inductive invariant exists (SAFE), unsat = error state reachable.
solve_verdict() { # <file>
  case "$SOLVER_KIND" in
    ay) "$SOLVER_BIN" solve "$1" -t:120000 2>/dev/null ;;
    z3) "$SOLVER_BIN" -T:120 "$1" 2>/dev/null ;;
  esac | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]'
}
