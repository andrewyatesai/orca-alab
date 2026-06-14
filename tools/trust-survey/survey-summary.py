#!/usr/bin/env python3
"""Summarize a Trust survey JSON (tcargo-trust --format json) into the gap-log metric.

Usage: survey-summary.py <survey.json>

Prints the headline counts (the gap-log "unknown count" is total_obligations - total_proved,
i.e. everything not proved clean) plus the per-blocking-reason histogram and the functions with
the most unproved obligations — the work-list for the next co-evolution lever.
"""
import json
import sys
from collections import Counter


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    with open(sys.argv[1], encoding="utf-8", errors="replace") as f:
        data = json.load(f)

    summary = data.get("summary", {})
    funcs = data.get("functions", [])

    total_obl = summary.get("total_obligations", 0)
    proved = summary.get("total_proved", 0)
    failed = summary.get("total_failed", 0)
    unknown = summary.get("total_unknown", 0)
    timed_out = summary.get("total_timed_out", 0)
    runtime_checked = summary.get("total_runtime_checked", 0)
    not_proved = total_obl - proved

    print(f"crate verdict        : {summary.get('verdict', '?')}")
    print(f"functions analyzed   : {summary.get('functions_analyzed', len(funcs))}")
    print(f"total obligations    : {total_obl}")
    print(f"  proved             : {proved}")
    print(f"  failed             : {failed}")
    print(f"  unknown            : {unknown}")
    print(f"  timed_out          : {timed_out}")
    print(f"  runtime_checked    : {runtime_checked}")
    print(f"GAP (not proved)     : {not_proved}   <- gap-log unknown count, target 0")

    # Per-obligation outcome.status histogram + blocking-reason histogram (over unproved).
    status_hist = Counter()
    reason_hist = Counter()
    per_fn_unproved = Counter()
    for fn in funcs:
        name = fn.get("function", "?")
        for obl in fn.get("obligations", []):
            outcome = obl.get("outcome", {})
            status = outcome.get("status", "?")
            status_hist[status] += 1
            if status not in ("proved", "verified"):
                per_fn_unproved[name] += 1
                reason = outcome.get("reason") or outcome.get("note") or obl.get("description", "?")
                # collapse to the leading clause so near-duplicate reasons group
                reason_hist[reason.split(";")[0].split(" at ")[0].strip()[:80]] += 1

    print("\n--- outcome.status histogram ---")
    for status, n in status_hist.most_common():
        print(f"{n:7d}  {status}")

    print("\n--- top blocking reasons (unproved obligations) ---")
    for reason, n in reason_hist.most_common(20):
        print(f"{n:7d}  {reason}")

    print("\n--- functions with the most unproved obligations (work-list) ---")
    for name, n in per_fn_unproved.most_common(25):
        print(f"{n:7d}  {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
