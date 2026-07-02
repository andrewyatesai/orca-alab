# Conformance triage: `overlong-params` and `csi-huge-single-param`

Triage of the two un-triaged REVIEW divergences on the gauntlet conformance axis
(`node tools/terminal-bench/gauntlet.mjs conformance`).

- aterm: v0.5.13 via the napi addon (`native/orca-node/orca_node.node`, `HeadlessTerminal`)
- baseline: `@xterm/headless` 6.1.0-beta.220 (`tools/terminal-bench/node_modules`)
- corpus: `tools/aterm-vs-xterm/corpus.json` (26 cases), 24x80 grid, trailing-whitespace-stripped rows
- date: 2026-07-01

## Verdict summary

| case | verdict |
| --- | --- |
| `overlong-params` | **no-divergence — harness artifact** (both engines produce byte-identical viewport grids and cursor positions; the gauntlet's xterm.js leg reads the wrong 24 rows once the buffer scrolls) |
| `csi-huge-single-param` | **no-divergence — harness artifact** (same mechanism) |

Neither of the three anticipated verdicts (`aterm-correct` / `xterm.js-correct` /
`both-defensible`) applies, because there is **no engine divergence at all**: at every
byte-prefix of both cases, aterm's viewport, xterm.js's viewport, the cursor position,
and the scrolled-off scrollback line are identical. The REVIEW status is produced
entirely by the comparison harness, not by either engine.

## Mechanical cause (both cases)

The gauntlet compares aterm's **viewport** against xterm.js's **buffer-absolute** rows:

- aterm leg: `HeadlessTerminal.snapshot()` returns the 24 viewport rows.
- xterm.js leg (`tools/terminal-bench/gauntlet.mjs` lines 149–151, and identically
  `tools/aterm-vs-xterm/snapshot.mjs`): `buffer.active.getLine(r)` for `r = 0..23`.
  In xterm.js that index is absolute from the top of the buffer *including scrollback*.

Both corpus cases end by printing through the bottom-right cell with autowrap on
(DECAWM pending-wrap), which scrolls exactly one line into scrollback in **both**
engines (`baseY = 1` in xterm.js, `scrollbackLen() = 1` in aterm). From that byte on,
the xterm.js leg reports a window shifted up by one row relative to the viewport, so
the text-grid diff fires even though the engines agree.

Fix (one line, per leg): read `buffer.active.getLine(buffer.active.viewportY + r)`.
With that offset the full corpus goes from 23/26 parity to **25/26** — the two REVIEW
cases disappear, and the only remaining divergence is `invalid-utf8-raw-bytes`, which
already carries its accepted-divergence `comment` in the corpus. For non-scrolling
cases `viewportY === 0`, so the fix is a no-op there (parity on the other 23 cases is
unchanged). Not applied here — the harness files are outside this triage's scope.

## Case 1: `overlong-params`

Repro bytes (latin1-escaped):

```
\x1b[2J\x1b[999999999;888888888Hclamped\x1b[38;5;99999999mcolor\x1b[0m\x1b[1000000Adone
```

Operation trace (identical in both engines):

1. `CSI 2 J` — clear screen; cursor stays at (1,1).
2. `CSI 999999999;888888888 H` — CUP with overlong params; both engines clamp the
   move to the bottom-right cell (24,80). (aterm saturates each param to 65535 while
   parsing; xterm.js clamps at int32 — both are then clamped to the 24x80 grid, so the
   different parser caps are unobservable. See boundary table below.)
3. `clamped` — `c` prints at (24,80) and sets pending-wrap; `l` wraps, which at the
   bottom row scrolls one line into scrollback; `lamped` continues on the new bottom row.
4. `CSI 38;5;99999999 m` + `color` — overlong palette index; prints `color` (text
   identical; see the SGR note below for the attribute-level detail).
5. `CSI 1000000 A` — CUU clamped to the top row.
6. `done` prints at row 1, col 12.

Both engines' final viewport (rows 1..21 of the remainder are blank, trimmed):

```
row  0: "           done"
row 22: 79 spaces + "c"
row 23: "lampedcolor"
```

- aterm cursor `[0,15]`; xterm.js `cursorY=0 cursorX=15`. Identical.
- Scrolled-off line: aterm `scrollbackLen()=1`; xterm.js `baseY=1`, `getLine(0)=""`. Identical.
- Gauntlet-as-is read of xterm.js (buffer-absolute `getLine(0..23)`) instead shows
  `done` on row **1** and drops `lampedcolor` off the bottom — the spurious diff.

Prefix bisect (feed `bytes[0..n]` for every `n`, compare both ways):

- Viewport-aligned comparison: **identical at every prefix length 1..72.**
- Gauntlet-style comparison: first diverges at prefix length **28** =
  `...Hcl` — precisely the byte whose wrap scrolls the buffer (`baseY` 0 → 1).

## Case 2: `csi-huge-single-param`

Repro bytes:

```
\x1b[2J\x1b[1;1H\x1b[2147483648Bclamp-down\x1b[99999999999999999999Ctext
```

Operation trace (identical in both engines):

1. `CSI 2147483648 B` — CUD with a param of exactly int32-max+1 (aterm saturates to
   65535, xterm.js clamps to 2147483647); both then clamp the move to row 24.
2. `clamp-down` prints at (24,1..10).
3. `CSI 99999999999999999999 C` — CUF past both caps; both clamp to column 80.
4. `text` — `t` prints at (24,80), pending-wrap; `e` wraps → one-line scroll; `ext`
   on the new bottom row.

Both engines' final viewport:

```
row 22: "clamp-down" + 69 spaces + "t"
row 23: "ext"
```

- aterm cursor `[23,3]`; xterm.js `cursorY=23 cursorX=3`. Identical.
- Scrollback: 1 blank line in both. Identical.
- Prefix bisect: viewport-aligned **identical at every prefix length 1..60**;
  gauntlet-style first diverges at prefix length **58** = `...Cte`, the wrap/scroll byte.

## Saturation boundary cross-check

aterm accumulates CSI params in `u32` with saturating arithmetic and clamps to
`u16::MAX` on push (`rust/aterm/crates/aterm-parser/src/csi.rs` lines 20–22; the TLA
invariant in `src/invariants.rs` pins `params ∈ Seq(0..65535)`). That is the same
65535 cap xterm-the-C-program uses. xterm.js clamps at int32 max instead. ECMA-48
sets no parameter maximum; DEC STD 070 requires clamping cursor movement to the page
margins whatever the parameter value. Since every movement op clamps to the 24x80
grid far below either cap, the differing caps are unobservable via cursor state:

| input (from `\x1b[1;1H`) | aterm cursor | xterm.js cursor | |
| --- | --- | --- | --- |
| `CSI 23 B` | [23,0] | [23,0] | SAME |
| `CSI 65535 B` (u16 max) | [23,0] | [23,0] | SAME |
| `CSI 65536 B` (u16 max+1) | [23,0] | [23,0] | SAME |
| `CSI 2147483647 B` (int32 max) | [23,0] | [23,0] | SAME |
| `CSI 2147483648 B` | [23,0] | [23,0] | SAME |
| `CSI 65535 C` / `65536 C` / `99999 C` | [0,79] | [0,79] | SAME |
| `CSI 99999999999999999999 C` | [0,79] | [0,79] | SAME |
| `CSI 65535;65535 H` / `65536;65536 H` / `999999999;888888888 H` | [23,79] | [23,79] | SAME |

Adjacent attribute-level note (not visible on the text-grid axis, and not part of
either REVIEW case's diff): for an out-of-range 256-color index `CSI 38;5;P m` with
P > 255, aterm clamps the index to 255 while xterm.js keeps `P & 0xFF`
(e.g. `P=256` → aterm fg 255, xterm.js fg 0; `P=300` → 255 vs 44). The corpus value
`99999999` happens to satisfy `P mod 256 = 255`, so even the cell attributes agree
there. Out-of-range indices are undefined by ITU T.416/ECMA-48; both behaviors are
defensible. Recorded here so a future attribute-level differential doesn't re-triage
it from scratch.

## Upstream bug report

None — no aterm engine bug found. The actionable defect is in the **comparison
harness**: `tools/terminal-bench/gauntlet.mjs` (xterm leg, lines 149–151) and
`tools/aterm-vs-xterm/snapshot.mjs` read xterm.js rows buffer-absolute instead of
viewport-relative (`getLine(viewportY + r)`), which misreports any corpus case that
scrolls. Applying that offset yields 25/26 parity with only the pre-documented
`invalid-utf8-raw-bytes` divergence remaining.

## Reproducing

```js
const { HeadlessTerminal } = require('native/orca-node/orca_node.node')
const { Terminal } = require('tools/terminal-bench/node_modules/@xterm/headless/lib-headless/xterm-headless.js')
const a = new HeadlessTerminal(80, 24, 1000) // (cols, rows, scrollback)
a.write(bytes)
const aGrid = a.snapshot() // 24 viewport rows

const x = new Terminal({ rows: 24, cols: 80, allowProposedApi: true })
x.write(bytes)
await new Promise((r) => x.write('', r))
const b = x.buffer.active
// b.getLine(r)               → gauntlet-as-is: buffer-absolute, diverges after a scroll
// b.getLine(b.viewportY + r) → viewport-aligned: identical to aterm on both cases
```

Bisect: feed `bytes.subarray(0, n)` for `n = 1..len` and compare both ways per prefix;
the viewport-aligned grids never differ, the buffer-absolute read differs from the
first scroll-inducing byte (prefix 28 for `overlong-params`, 58 for
`csi-huge-single-param`) through the end.
