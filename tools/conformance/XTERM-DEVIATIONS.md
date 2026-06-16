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

