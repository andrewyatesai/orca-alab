#!/usr/bin/env bash
# Survey a single orc Rust crate through Trust's verifier and emit deterministic
# per-obligation JSON, bounded so no single hard obligation can hang the run.
#
# WHY each guard exists (learned the hard way — see docs/rust-migration/trust-verification.md
# builds #36-#38): a verifier must never be able to hang on one obligation.
#   - -Ztrust-verify-function-budget-ms  per-function wall-clock budget, enforced at
#       obligation boundaries with SOUND degradation (Timeout / Unsupported — never
#       Proved). Bounds a function whose obligations each return.
#   - -Ztrust-verify-timeout-ms          per-obligation verifier deadline. Bounds a
#       SINGLE obligation that would otherwise spin inside the solver.
#     Both reach `targo trust` through TRUSTFLAGS, its tracked policy channel. The old
#     TRUST_VERIFY_FN_BUDGET_MS / TRUST_TIMEOUT_MS / TRUST_SKIP_FUNCTIONS environment
#     knobs are REFUSED by targo trust (measured 2026-09-28: "untracked Trust
#     semantic/codegen control"), so this script unsets them.
#   - perl alarm backstop        process-level wall clock; kills the whole compile if an
#       UNCOVERED engine path (no thread watchdog) still spins. macOS has no `timeout(1)`.
#
# Usage: survey-orca-verify.sh <crate> [out-dir] [--contracts]
#   crate        cargo package name, e.g. orca-core (default: orca-core)
#   out-dir      where to drop the JSON + summary (default: /tmp/trust-survey)
#   --contracts  accepted for compatibility and a no-op: `targo trust` turns verification
#                on, and the compiler injects cfg(trust_verify) whenever it is on, so
#                #[cfg_attr(trust_verify, trust::…)] contracts are always active here
#   --skip       no longer supported: targo trust refuses TRUST_SKIP_FUNCTIONS and has no
#                tracked per-function exclusion
set -uo pipefail

CRATE="orca-core"
OUT_DIR="/tmp/trust-survey"
CONTRACTS=0
positional=0
while [ $# -gt 0 ]; do
  case "$1" in
    --contracts) CONTRACTS=1 ;;
    --skip)
      echo "--skip is no longer supported: targo trust refuses TRUST_SKIP_FUNCTIONS (an untracked control)" >&2
      exit 2 ;;
    -*) echo "unknown flag: $1" >&2; exit 2 ;;
    *) if [ "$positional" = 0 ]; then CRATE="$1"; positional=1; else OUT_DIR="$1"; fi ;;
  esac
  shift
done

# The Trust toolchain's targo (atpkg) — `--unverified --version` proves it IS targo
# (upstream cargo rejects the flag). Never the ~/trust/build tree: that is the compiler
# repo's exclusive build output.
TARGO="${CARGO:-targo}"
"$TARGO" --unverified --version >/dev/null 2>&1 \
  || { echo "FATAL: \`$TARGO\` is not targo — install the Trust toolchain (atpkg) and put targo on PATH" >&2; exit 2; }

# The Orca Rust workspace Cargo.toml lives under rust/, not the repo root — targo must
# run from there or it finds no manifest and degrades to the transport:missing-json probe.
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
export WS="${ORC_RUST:-$REPO_ROOT/rust}"
[ -f "$WS/Cargo.toml" ] || { echo "FATAL: no Cargo.toml at $WS (set ORC_RUST)" >&2; exit 2; }

mkdir -p "$OUT_DIR"
STAMP="$(date '+%Y%m%d-%H%M%S')"
JSON="$OUT_DIR/${CRATE}-${STAMP}.json"
LOG="$OUT_DIR/${CRATE}-${STAMP}.log"

# Bounds (override via env). Defaults: 90s/obligation, 120s/function, 45min whole run.
# The legacy knob names are still read as override INPUTS, then unset, because targo
# trust refuses to run while they are in its environment.
FN_BUDGET_MS="${SURVEY_FN_BUDGET_MS:-${TRUST_VERIFY_FN_BUDGET_MS:-120000}}"
OBL_TIMEOUT_MS="${SURVEY_OBL_TIMEOUT_MS:-${TRUST_TIMEOUT_MS:-90000}}"
RUN_TIMEOUT_S="${SURVEY_RUN_TIMEOUT_S:-2700}"
unset TRUST_VERIFY_FN_BUDGET_MS TRUST_TIMEOUT_MS TRUST_SKIP_FUNCTIONS TRUST_VERIFY_FUNCTIONS
export TRUSTFLAGS="${TRUSTFLAGS:+$TRUSTFLAGS }-Ztrust-verify-function-budget-ms=$FN_BUDGET_MS -Ztrust-verify-timeout-ms=$OBL_TIMEOUT_MS"

export TRUST_VERIFY_SURVEY=1
export TRUST_VERIFY_POLICY="verify-example-corpus"
# Verify ONLY the surveyed crate, not its deps. Per-crate is the survey's whole point, and
# it dodges trustc MIR-opt query cycles in vendored deps (regex-syntax E0391 blocks
# orca-text/config/agents — every regex-dependent crate). Set TRUST_VERIFY_PRIMARY_ONLY=0 to
# restore whole-dependency-graph verification.
export TRUST_VERIFY_PRIMARY_ONLY="${TRUST_VERIFY_PRIMARY_ONLY:-1}"
# Bound the direct-SMT (execute_direct) path too — a defense-in-depth backstop.
# The ay-lra implied-bound propagation now has a per-state no-progress guard that
# makes it CONVERGE BY DEFAULT, so this deadline should never fire on a solvable
# obligation. SURVEY_NO_AY_TIMEOUT=1 disables it to PROVE convergence-by-default
# (the per-obligation -Ztrust-verify-timeout-ms stays on, so a non-LRA path can't
# masquerade as an LRA hang).
if [ "${SURVEY_NO_AY_TIMEOUT:-0}" = 1 ]; then
  unset AY_DIRECT_SOLVE_TIMEOUT_MS
  echo "AY direct-solve : timeout DISABLED (convergence-by-default proof mode)" | tee -a "$LOG"
else
  export AY_DIRECT_SOLVE_TIMEOUT_MS="$OBL_TIMEOUT_MS"
fi
echo "targo        : $("$TARGO" --unverified --version 2>/dev/null)" | tee    "$LOG"
echo "crate        : $CRATE"                               | tee -a "$LOG"
echo "contracts    : $CONTRACTS (always active under targo trust)" | tee -a "$LOG"
echo "bounds       : obl=${OBL_TIMEOUT_MS}ms fn=${FN_BUDGET_MS}ms run=${RUN_TIMEOUT_S}s" | tee -a "$LOG"
echo "json         : $JSON"                                | tee -a "$LOG"
echo "start        : $(date '+%H:%M:%S')"                  | tee -a "$LOG"

# Bust cargo's build cache for the crate — verification runs DURING compilation, so a
# cached (unchanged) crate makes trustc skip re-verifying and targo emits a degraded
# empty/transport probe. Touching the crate root forces a recompile + re-verify.
CRATE_DIR="$WS/crates/${CRATE#orca-}"
[ -d "$WS/crates/$CRATE" ] && CRATE_DIR="$WS/crates/$CRATE"
if [ -f "$CRATE_DIR/src/lib.rs" ]; then touch "$CRATE_DIR/src/lib.rs"; fi

# perl alarm = process-level backstop (no timeout(1) on macOS). Run from the workspace
# root so cargo resolves the manifest; --manifest-path alone doesn't fix cwd-relative probes.
perl -e 'chdir $ENV{WS} or die "chdir $ENV{WS}: $!"; alarm shift; exec @ARGV' "$RUN_TIMEOUT_S" \
  "$TARGO" trust check -p "$CRATE" --format json --allow-l0-gaps >"$JSON" 2>>"$LOG"
RC=$?

echo "exit         : $RC at $(date '+%H:%M:%S')"           | tee -a "$LOG"
if [ "$RC" = 142 ] || [ "$RC" = 14 ]; then
  echo "!! WHOLE-RUN TIMEOUT after ${RUN_TIMEOUT_S}s — an UNCOVERED engine path still hangs." | tee -a "$LOG"
  echo "   Re-run per-function via TRUST_VERIFY_FUNCTIONS to isolate the culprit."            | tee -a "$LOG"
fi

# Outcome histogram (best-effort; rows carry outcome.status / outcome.reason).
echo "--- outcome histogram ---" | tee -a "$LOG"
grep -oE '"status"[: ]*"[a-zA-Z_]+"' "$JSON" 2>/dev/null | sort | uniq -c | sort -rn | tee -a "$LOG"
echo "json bytes   : $(wc -c < "$JSON" 2>/dev/null)" | tee -a "$LOG"
exit "$RC"
