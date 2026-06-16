# Trust safety verification for the terminal engine

Two complementary **automated** bug-finders for the headless terminal engine:

| Tool | Finds | How |
|------|-------|-----|
| `tools/conformance` (differential fuzzer) | **behaviour** bugs — engine renders differently than xterm.js | random VT streams through both engines, diff the grid |
| `tools/trust-terminal` (Trust verifier) | **safety** bugs — integer overflow / out-of-bounds that *crash the daemon* | `tcargo trust` enumerates every arithmetic op + index as a proof obligation |

The fuzzer answers "does it match xterm?"; Trust answers "can any PTY byte stream
make it panic?" — no test input required, the verifier reasons over all inputs.

## Run it

```sh
tcargo trust check tools/trust-terminal/cursor_arithmetic.rs
```

Trust lifts each `+`/`-`/index into a Level-0 safety obligation. The `_unsafe`
variants (raw `col + n`, unchecked `grid[row][col]`) surface as can-panic
obligations — `mir_assert::Overflow` / out-of-bounds index — exactly the crash an
unclamped cursor causes. The clamped variants (saturating arithmetic + bounds
checks) are the shape real engine code must use.

## What Trust catches today, and the gap

- **Works:** Trust enumerates the full safety attack surface and definitively
  **flags** can-overflow arithmetic (e.g. `col + n` near `usize::MAX`) — the same
  bug class that, on an unclamped cursor, panics the terminal daemon. This is the
  automated finder: point it at the arithmetic, it reports what can crash.
- **Gap (Trust-side, WIP):** *proving* the clamped code clean currently returns
  `UNKNOWN` — the native CHC/PDR pipeline needs proof-grade typed evidence
  (`trust-mc.typed-chc-obligation.v1`) the adapter does not yet emit for these
  obligations, and full-crate runs ICE on vendored deps (hashbrown/memchr/…). This
  is a Trust capability gap the compiler owner is actively closing, not an engine
  soundness issue. As the proof pipeline lands, this same harness yields clean
  proofs with no change to the engine code.

## Why this matters for Orca

The daemon runs untrusted agent/PTY output. A single unclamped cursor +
out-of-bounds grid write takes down every terminal session. Trust makes that class
**unrepresentable** once the engine arithmetic is written in the clamped/saturating
form this harness verifies — a machine-checked guarantee no fuzzer can give.
