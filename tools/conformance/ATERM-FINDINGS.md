# aterm findings (from the differential fuzzer)

Live findings against the aterm engine, surfaced by `hunt.mjs` (fuzz + auto-minimize)
and bisected to minimal repros. The conformance suite locks each one in; the owner
fixes aterm-core against this oracle (e.g. SL/SR/DECIC/DECDC + CUD margin clamping
were found here and already fixed → 72/72 on those).

Origin-mode divergences are tracked separately in `XTERM-DEVIATIONS.md` (those are
**xterm** spec bugs, not aterm bugs).

## Fixed

### 1. ED/EL at a pending wrap drops the last cell  ⟶ cases `el-pending-wrap`, `ed-pending-wrap`  ✅ FIXED
Filling a row to the last column arms a deferred wrap; the cursor parks on the
last glyph. Erase-to-end (`CSI 0 K` / `CSI 0 J`) must then erase *nothing* (the
cursor is logically past the row), but aterm erased the parked cell.
- Repro (4×2): `ABCD\x1b[0K` → xterm `"ABCD"`, aterm was `"ABC"`. Same for `\x1b[0J`.
- Root cause: xterm.js encodes pending-wrap as `x==cols`, so EL-0/ED-0 erase the
  empty range `[cols, cols)` and leave both the parked glyph and the wrap intact.
  aterm parked the cursor at `last_col` + a flag, then cleared the flag and erased
  from `last_col` — clobbering the cell.
- Fix (`aterm-grid/src/grid/erase.rs`): `erase_to_end_of_line` / `erase_to_end_of_screen`
  short-circuit when `pending_wrap` is set — erase nothing on the current row and
  preserve the wrap (a later glyph still wraps, matching xterm.js). Rows below the
  cursor are still cleared by ED-0. Verified: conformance 74/74, and the class no
  longer surfaces in `hunt.mjs` (4000 trials).

### 3. Vertical cursor clamping in a scroll region (CUU / CUD / CNL / VPR)  ✅ FIXED
Relative vertical moves clamped on the wrong condition. aterm only clamped to a
margin when the cursor was *fully inside* the region; xterm clamps on the near
margin alone (`CursorUp: min = cur<top ? 0 : top`, `CursorDown: max = cur>bot ?
screen : bot`). So a cursor *above* the region moving down sailed past the bottom
margin, and one *below* moving up sailed past the top margin. VPR was also routed
through CUD and wrongly bottom-margin-clamped — it is page-relative and stops at
the screen edge.
- Repro (6×8): `\x1b[2;3r\x1b[1;1HX\x1b[3BY` (CUD from above the region) → xterm
  stops at the bottom margin, aterm ran to the last line. Symmetric for CUU from
  below, plus VPR/CNL.
- Fix (`aterm-grid/src/grid/cursor_ops.rs`): `cursor_up`/`cursor_down` clamp on the
  near margin only; new `line_position_relative` for VPR clamps to the screen edge.
- Locked in by `cud-above-region-clamp`, `cuu-below-region-clamp`,
  `cnl-above-region-clamp`, `vpr-ignores-region` (suite 78/78). The focused reducer
  `focus-region.mjs` now reports **zero** non-origin divergences across all
  region/op/count combinations; the residual divergences are all **origin-mode**,
  where xterm itself is off (`XTERM-DEVIATIONS.md`: cuu/cud/vpr-*-under-origin) and
  aterm matches ECMA-48.

## Open

### 2. Wide-char editing at the cursor
Editing ops interacting with a double-width glyph drop/shift a cell.
- `中\b\x1b[4he#` (region/insert context) → xterm `" qVP e#"`, aterm `" qVP  #"` (an
  `e` becomes a space).
- `…\x1b[8X\x1b[4h\x1b[1K中…` → a digit adjacent to a wide glyph is lost.
- Fix shape: orphan the *other half* of a wide pair to a space when an edit/erase
  splits it (BS-onto-continuation, ECH/ICH/DCH across a wide boundary).

## Reproduce

```sh
node hunt.mjs 4000 1 8     # fuzz + auto-minimize, 8 distinct shapes
node bisect.mjs <hex> <c> <r>
```
