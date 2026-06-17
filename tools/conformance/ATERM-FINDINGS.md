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

### 4. Save/restore cursor (DECSC/DECRC) across a scroll  ✅ RESOLVED — xterm deviation
The dominant class the fuzzer surfaced after the fixes above turned out to be an
**xterm.js** quirk, not an aterm bug. When a scroll happens between `\x1b7` (DECSC)
and `\x1b8` (DECRC), xterm.js restores the cursor one row higher per scrolled line
— it stores the saved cursor as an absolute scrollback position, so DECRC follows
the scrolled content. Real VT terminals (and xterm-C) restore to the saved *screen*
row; aterm already does this.
- Repro (6×8): `\x1b[5;2H\x1b7\n\n\n\n\x1b8` → xterm cursor row 3, aterm row 4 (one
  scroll between save and restore). `focus-restore.mjs` shows the whole class is
  exactly "a scroll between DECSC and DECRC" (region-set / CUP between them match).
- Resolution: documented in `XTERM-DEVIATIONS.md` as `decrc-tracks-scroll`
  (self-verifying: xterm=3, engine=4). No engine change — aterm matches the spec.
- Note: `hunt.mjs` still reports these as raw-grid mismatches; they are known
  deviations, not bugs (the fuzzer is oblivious to the registry).

### 5. Wide glyph wrapping off the last column stranded the skipped cell  ✅ FIXED
The "extra trailing cell" shapes (`"…┴─"` vs `"…┴─A"`) were a wide-glyph wrap edge:
a width-2 glyph placed at the last column (no wrap pending) can't fit and wraps to
the next line. xterm blanks the skipped cell with the current BCE background; aterm
left the old content there.
- Repro (4×2): `ABCD\x1b[1;4H中` → xterm `"ABC"`+`"中"`, aterm kept `"ABCD"`+`"中"`.
- Fix (`aterm-grid/src/grid/write_split.rs`, `aterm-core/.../handler_write.rs`): the
  wide pre-wrap step blanks the skipped tail (`blank_wide_wrap_tail`) before
  advancing. Locked in by `wide-wrap-blanks-last-cell` (83/83); `focus-wide.mjs`
  and the deviation-filtered fuzzer no longer surface it.

### 6. Deferred wrap + line feed double-scrolled at the bottom row  ✅ FIXED
The "content a row or two off" shapes were a deferred-wrap/LF interaction. `line_feed`
at the bottom margin reused `scroll_region_up`, which preserves the pending wrap for
the explicit SU/SD ops — but LF is a cursor-down and must reset the wrap (xterm
CursorDown ResetWrap). The stale wrap then made the next glyph trigger a SECOND
scroll.
- Repro (4×3): `\x1b[3;1Habcd\nZ` → xterm scrolls once (abcd row 1, Z at last col);
  aterm scrolled twice (abcd to row 0, Z at col 0).
- Fix (`aterm-grid/src/grid/cursor_ops.rs`): clear pending_wrap after the at-bottom
  scroll in `line_feed`; the explicit SU/SD path is untouched and still preserves it.
- Locked in by `lf-at-pending-wrap-bottom-scrolls-once` (84/84). The deviation-
  filtered fuzzer dropped from 8 distinct shapes to 1.

## Open

### 7. Lone combining mark at column 0 after backspace (esoteric; ~1 in 15k)
The last fuzzer shape. A combining mark (e.g. U+0301) with the cursor at column 0
and no base to its left: xterm.js writes the mark as a bare standalone cell,
*clobbering* whatever was there (`a\b́` → xterm loses the `a`, leaving an empty
cell). aterm keeps the base char and drops the orphan mark — the data-preserving
choice. No governing spec for an unattached combining mark, so this is a deliberate,
documented difference rather than a bug to "fix" by adopting xterm's data loss.
Revisit only if a real workload depends on xterm's behavior.

## Reproduce

```sh
node hunt.mjs 15000 1 12   # fuzz + auto-minimize; DECRC-scroll deviation skipped
node focus-region.mjs      # vertical moves in a region
node focus-restore.mjs     # DECSC/DECRC across scroll (the documented deviation)
node focus-wide.mjs        # wide-glyph edit/erase/wrap edges
```
