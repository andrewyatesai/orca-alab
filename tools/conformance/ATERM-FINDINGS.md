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

### 1b. ALL erases must preserve the pending wrap, not just EL-0/ED-0  ✅ FIXED
The fix above covered erase-*to-end*, but the deeper rule is broader: in xterm an
erase clears cells **without ever resetting `wrapnext`** — only ECH and cursor
moves do. aterm cleared the flag in every other erase (EL1/EL2, ED1/ED2/ED3,
selective DECSEL/DECSED, rectangular DECERA/DECFRA/DECSERA, even DECCARA/DECCRA).
- Repro (4×2): `ABCD\x1b[1KZ` → xterm `"·Z"` (Z wraps to row 1), aterm put `Z` back
  on row 0. Same for `\x1b[2K`, `\x1b[2J`, etc. (probe: cursorX stays at `cols`
  across all six EL/ED variants).
- Fix (`aterm-grid/src/grid/erase.rs`): removed `clear_pending_wrap()` from every
  erase/fill/selective/rect/attr/copy path; only DECALN (which homes the cursor)
  still clears it. Flipped the matching `*_clears_pending_wrap` unit tests to
  `*_keeps_pending_wrap`. Locked in by `el1/el2/ed2-pending-wrap-wraps` (82/82).

### 2. Wide-char editing — IRM insert onto a wide continuation cell  ✅ FIXED
Inserting (IRM or ICH) at the continuation cell of a double-width pair bisects the
pair. aterm orphaned only the WIDE *head* to a blank but left the continuation
cell's stale `WIDE_CONTINUATION` flag; the shift moved that stale cell right, and
a later insert/erase there treated its unrelated left neighbour as a wide head and
cleared it.
- Repro (8×2): `中\b\x1b[4he#` → xterm `" e#"`, aterm was `"  #"` (the inserted `e`
  was lost). Backspace parks the cursor on the continuation cell, then two IRM
  inserts trigger the stale-flag clobber.
- Fix (`aterm-grid/src/grid/row/char_ops.rs`): `insert_chars_fill` orphans BOTH
  halves of the split pair to `fill`, so the shifted cell carries no stale flag.
  Locked in by `wide-insert-split` (82/82); `focus-wide.mjs` reports zero
  divergences.

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

### 4. Save/restore cursor (DECSC/DECRC) across a scroll region or scroll
The dominant class the fuzzer surfaces after the fixes above. Sequences that save
the cursor (`\x1b7`), scroll or set a DECSTBM region, then restore (`\x1b8`) and
print leave the printed glyph one or more rows off vs xterm.
- e.g. shapes ending `…\x1b8\x1b[0J3` and `…\x1b8\x1b[3T3` put the trailing char a
  couple rows lower in aterm than in xterm.
- Needs triage: whether DECRC re-clamps the restored row to the (possibly changed)
  region, and how a saved position interacts with intervening scrolls.

### 5. Trailing glyph after a charset switch / at the wrap column
Minor: `…\x1b(0q\x1b(B中` style streams occasionally keep one extra trailing cell
in aterm (`"…┴─" ` vs `"…┴─A"`), a wrap/width edge at the last column with the DEC
special-graphics charset active.

## Reproduce

```sh
node hunt.mjs 4000 1 8     # fuzz + auto-minimize, 8 distinct shapes
node bisect.mjs <hex> <c> <r>
```
