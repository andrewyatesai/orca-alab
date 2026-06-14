#!/usr/bin/env bash
# Survey a single orc Rust crate through Trust's verifier and emit deterministic
# per-obligation JSON, bounded so no single hard obligation can hang the run.
#
# WHY each guard exists (learned the hard way — see docs/rust-migration/trust-verification.md
# builds #36-#38): a verifier must never be able to hang on one obligation.
#   - TRUST_VERIFY_FN_BUDGET_MS  per-function wall-clock budget, enforced at obligation
#       boundaries across BMC / trust-vc / trust-wp with SOUND degradation (Timeout /
#       Unsupported — never Proved). Bounds a function whose obligations each return.
#   - TRUST_TIMEOUT_MS           per-obligation typed-CHC/PDR deadline; feeds
#       options.timeout -> the native solve watchdog ceiling (timeout + 2s). Bounds a
#       SINGLE obligation that would otherwise spin inside ay_dpll (trust-mc be05d7f).
#   - perl alarm backstop        process-level wall clock; kills the whole compile if an
#       UNCOVERED engine path (no thread watchdog) still spins. macOS has no `timeout(1)`.
#
# Usage: survey-orca-verify.sh <crate> [out-dir] [--contracts] [--skip fn1,fn2]
#   crate        cargo package name, e.g. orca-core (default: orca-core)
#   out-dir      where to drop the JSON + summary (default: /tmp/trust-survey)
#   --contracts  compile with --cfg trust_verify so #[cfg_attr(trust_verify, trust::requires)]
#                contracts activate (otherwise a pure as-written baseline survey)
#   --skip       comma-separated TRUST_SKIP_FUNCTIONS patterns (exclude known-hard fns)
set -uo pipefail

CRATE="orca-core"
OUT_DIR="/tmp/trust-survey"
CONTRACTS=0
SKIP=""
positional=0
while [ $# -gt 0 ]; do
  case "$1" in
    --contracts) CONTRACTS=1 ;;
    --skip) SKIP="${2:-}"; shift ;;
    -*) echo "unknown flag: $1" >&2; exit 2 ;;
    *) if [ "$positional" = 0 ]; then CRATE="$1"; positional=1; else OUT_DIR="$1"; fi ;;
  esac
  shift
done

TRUST="${TRUST_HOME:-$HOME/trust}"
# Prefer the freshly-built stage2 tools-bin tcargo-trust; fall back to the sysroot copy.
TCARGO=""
for cand in \
  "$TRUST/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/tcargo-trust" \
  "$TRUST/build/host/stage2/bin/tcargo-trust" \
  "$TRUST/build/aarch64-apple-darwin/stage0-sysroot/bin/tcargo-trust"; do
  [ -x "$cand" ] && { TCARGO="$cand"; break; }
done
[ -n "$TCARGO" ] || { echo "FATAL: no tcargo-trust binary found under $TRUST/build" >&2; exit 2; }

# The orc Rust workspace Cargo.toml lives under rust/, not the repo root — tcargo must
# run from there or it finds no manifest and degrades to the transport:missing-json probe.
export WS="${ORC_RUST:-$HOME/orc/rust}"
[ -f "$WS/Cargo.toml" ] || { echo "FATAL: no Cargo.toml at $WS (set ORC_RUST)" >&2; exit 2; }

mkdir -p "$OUT_DIR"
STAMP="$(date '+%Y%m%d-%H%M%S')"
JSON="$OUT_DIR/${CRATE}-${STAMP}.json"
LOG="$OUT_DIR/${CRATE}-${STAMP}.log"

# Bounds (override via env). Defaults: 90s/obligation, 120s/function, 45min whole run.
FN_BUDGET_MS="${TRUST_VERIFY_FN_BUDGET_MS:-120000}"
OBL_TIMEOUT_MS="${TRUST_TIMEOUT_MS:-90000}"
RUN_TIMEOUT_S="${SURVEY_RUN_TIMEOUT_S:-2700}"

export TRUST_VERIFY_SURVEY=1
export TRUST_VERIFY_POLICY="verify-example-corpus"
export TRUST_VERIFY_FN_BUDGET_MS="$FN_BUDGET_MS"
export TRUST_TIMEOUT_MS="$OBL_TIMEOUT_MS"
# Bound the direct-SMT (execute_direct) path too — it otherwise runs unbounded and a
# non-converging QF_LRA propagation spins forever (the typed-CHC watchdog doesn't cover
# it). Enables the ay solver's own deadline->should_stop->budget-check abort.
export AY_DIRECT_SOLVE_TIMEOUT_MS="$OBL_TIMEOUT_MS"
[ -n "$SKIP" ] && export TRUST_SKIP_FUNCTIONS="$SKIP"
[ "$CONTRACTS" = 1 ] && export RUSTFLAGS="${RUSTFLAGS:-} --cfg trust_verify"

echo "tcargo-trust : $TCARGO"                              | tee    "$LOG"
echo "crate        : $CRATE"                               | tee -a "$LOG"
echo "contracts    : $CONTRACTS  skip=[${SKIP:-none}]"     | tee -a "$LOG"
echo "bounds       : obl=${OBL_TIMEOUT_MS}ms fn=${FN_BUDGET_MS}ms run=${RUN_TIMEOUT_S}s" | tee -a "$LOG"
echo "json         : $JSON"                                | tee -a "$LOG"
echo "start        : $(date '+%H:%M:%S')"                  | tee -a "$LOG"

# perl alarm = process-level backstop (no timeout(1) on macOS). Run from the workspace
# root so cargo resolves the manifest; --manifest-path alone doesn't fix cwd-relative probes.
perl -e 'chdir $ENV{WS} or die "chdir $ENV{WS}: $!"; alarm shift; exec @ARGV' "$RUN_TIMEOUT_S" \
  "$TCARGO" trust check -p "$CRATE" --format json --allow-l0-gaps >"$JSON" 2>>"$LOG"
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
