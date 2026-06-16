# aterm findings (from the differential fuzzer)

Live findings against the aterm engine, surfaced by `hunt.mjs` (fuzz + auto-minimize)
and bisected to minimal repros. The conformance suite locks each one in; the owner
fixes aterm-core against this oracle (e.g. SL/SR/DECIC/DECDC + CUD margin clamping
were found here and already fixed → 72/72 on those).

Origin-mode divergences are tracked separately in `XTERM-DEVIATIONS.md` (those are
**xterm** spec bugs, not aterm bugs).

## Open

### 1. ED/EL at a pending wrap drops the last cell  ⟶ cases `el-pending-wrap`, `ed-pending-wrap`
Filling a row to the last column arms a deferred wrap; the cursor parks on the
last glyph. Erase-to-end (`CSI 0 K` / `CSI 0 J`) must then erase *nothing* (the
cursor is logically past the row), but aterm erases the parked cell.
- Repro (4×2): `ABCD\x1b[0K` → xterm `"ABCD"`, aterm `"ABC"`. Same for `\x1b[0J`.
- Also drops a line-drawing glyph at the margin: `AB\x1b(0q\x1b(B\x1b[0K` (3 cols) →
  xterm `"AB─"`, aterm `"AB"`.
- Fix shape: in the erase handler, start the erase at `col + 1` when a wrap is pending.

### 2. Wide-char editing at the cursor
Editing ops interacting with a double-width glyph drop/shift a cell.
- `中\b\x1b[4he#` (region/insert context) → xterm `" qVP e#"`, aterm `" qVP  #"` (an
  `e` becomes a space).
- `…\x1b[8X\x1b[4h\x1b[1K中…` → a digit adjacent to a wide glyph is lost.
- Fix shape: orphan the *other half* of a wide pair to a space when an edit/erase
  splits it (BS-onto-continuation, ECH/ICH/DCH across a wide boundary).

### 3. Vertical cursor clamping in a scroll region (CUU / CPL / SD / VPR)
Multi-op sequences that move the cursor vertically within a DECSTBM region land a
row off in aterm. Minimal repros from `hunt.mjs` shapes [1],[3],[6] — e.g. a
region + `SD` + `VPR`/`CUU` leaves content one row higher/lower than xterm.
- Needs per-op triage: CUU/CPL clamp to the *top margin*, CUD/CNL to the *bottom
  margin*, VPR/VPA to the *screen edge* (the distinction the `cud-margin-clamp`
  case already locks in for CUD).

## Reproduce

```sh
node hunt.mjs 4000 1 8     # fuzz + auto-minimize, 8 distinct shapes
node bisect.mjs <hex> <c> <r>
```
