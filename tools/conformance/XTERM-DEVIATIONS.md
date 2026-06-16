# xterm.js spec deviations

Places where the reference implementation (**xterm.js 6.1.0-beta.220**) deviates
from ECMA-48 / DEC specs, found by the differential fuzzer. The engine follows
the spec in each case. Re-verify with `node deviations.mjs` — entries are rejected
if xterm no longer deviates or the engine no longer matches the spec.

## CUU moves the cursor DOWN, away from the top margin (origin mode)

- **Repro** (17×19): `1b5b3f36681b5b343b3137721b5b3841`
- **Spec**: ECMA-48 §8.3.22 (CUU): the active position moves UP by n lines, stopping at the top margin.
- **Probe**: cursor row after the sequence
- **xterm.js**: 6 (deviates)
- **Spec-correct / engine**: 3
- With origin mode + a scroll region, xterm moves the cursor downward instead of clamping it to the top margin. No real program relies on this; the engine clamps per spec.

## CUD moves one row too far under origin mode

- **Repro** (6×8): `1b5b3f36681b5b323b36721b5b313b31481b5b3342`
- **Spec**: ECMA-48 §8.3.19 (CUD): the active position moves DOWN by n lines. From the top margin (row 1) with n=3 the spec position is row 4.
- **Probe**: cursor row after the sequence
- **xterm.js**: 5 (deviates)
- **Spec-correct / engine**: 4
- Same origin-mode root cause as CUU: xterm miscomputes the region-relative base for vertical motion and overshoots by one. The engine moves exactly n rows per spec.

## VPR moves one row too far under origin mode

- **Repro** (6×8): `1b5b3f36681b5b323b36721b5b313b31481b5b3365`
- **Spec**: ECMA-48 §8.3.68 (VPR): the active position moves DOWN by n lines (page-relative). From row 1 with n=3 the spec position is row 4.
- **Probe**: cursor row after the sequence
- **xterm.js**: 5 (deviates)
- **Spec-correct / engine**: 4
- Origin-mode vertical-motion class (see CUU/CUD entries). VPR is page-relative — the engine moves exactly n and clamps only at the screen edge; xterm overshoots by one under origin mode.

