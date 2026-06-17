# aterm terminal-conformance checklist

A third party can verify this engine matches **xterm.js 6.1.0-beta.220** with two commands:

```sh
node build-corpus.mjs          # regenerate cases + goldens from real xterm.js
cargo run --release --example conformance -p orca-terminal
```

The goldens are not hand-authored — they are whatever xterm.js renders for each
case (visible grid **and** per-cell SGR attributes). The runner replays each case
through the Rust engine and diffs against the golden, exiting non-zero on any
divergence. Current result: **84/84 cases match xterm.js**
(10 with full attribute fingerprints).

## Coverage vs the full xterm.js handler registry

Every handler xterm registers in `src/common/InputHandler.ts`, with status:
**TESTED** (47) = implemented + a conformance case · **IMPL** (5) = implemented · **N/A** (14) = inert in a headless emulator (replies / titles / colors /
cursor shape / input-only modes — no visible-grid or attribute effect) · **GAP** (6) = not implemented.

| group | sequence | xterm method | status | notes / case |
|----|----|----|----|----|
| CSI | `@ ICH` | `insertChars` | ✅ TESTED | ich |
| CSI | `SP@ SL` | `scrollLeft` | ✅ TESTED | sl |
| CSI | `A CUU` | `cursorUp` | ✅ TESTED | cuu-cud |
| CSI | `SPA SR` | `scrollRight` | ✅ TESTED | sr |
| CSI | `B CUD` | `cursorDown` | ✅ TESTED | cuu-cud |
| CSI | `C CUF` | `cursorForward` | ✅ TESTED | cuf-cub |
| CSI | `D CUB` | `cursorBackward` | ✅ TESTED | cuf-cub |
| CSI | `E CNL` | `cursorNextLine` | ✅ TESTED | cnl-cpl |
| CSI | `F CPL` | `cursorPrecedingLine` | ✅ TESTED | cnl-cpl |
| CSI | `G CHA` | `cursorCharAbsolute` | ✅ TESTED | cha |
| CSI | `H CUP` | `cursorPosition` | ✅ TESTED | cup-basic |
| CSI | `I CHT` | `cursorForwardTab` | ✅ TESTED | cht-cbt |
| CSI | `J ED` | `eraseInDisplay` | ✅ TESTED | ed0-3 |
| CSI | `?J DECSED` | `eraseInDisplay(protect)` | ⚠ GAP | selective erase (protected cells) — rare |
| CSI | `K EL` | `eraseInLine` | ✅ TESTED | el0-2 |
| CSI | `?K DECSEL` | `eraseInLine(protect)` | ⚠ GAP | selective erase — rare |
| CSI | `L IL` | `insertLines` | ✅ TESTED | il |
| CSI | `M DL` | `deleteLines` | ✅ TESTED | dl |
| CSI | `P DCH` | `deleteChars` | ✅ TESTED | dch |
| CSI | `S SU` | `scrollUp` | ✅ TESTED | su |
| CSI | `T SD` | `scrollDown` | ✅ TESTED | sd |
| CSI | `X ECH` | `eraseChars` | ✅ TESTED | ech |
| CSI | `Z CBT` | `cursorBackwardTab` | ✅ TESTED | cht-cbt |
| CSI | `` HPA` | `charPosAbsolute` | ✅ TESTED | hpa-vpa |
| CSI | `a HPR` | `hPositionRelative` | ✅ TESTED | hpr-vpr |
| CSI | `b REP` | `repeatPrecedingCharacter` | ✅ TESTED | rep |
| CSI | `c DA1 / >c DA2` | `sendDeviceAttributes` | ➖ N/A | reply; the daemon emulator never replies |
| CSI | `d VPA` | `linePosAbsolute` | ✅ TESTED | hpa-vpa |
| CSI | `e VPR` | `vPositionRelative` | ✅ TESTED | hpr-vpr |
| CSI | `f HVP` | `hVPosition` | ✅ TESTED | hvp |
| CSI | `g TBC` | `tabClear` | ✅ TESTED | tbc3 |
| CSI | `h SM` | `setMode` | ✔ IMPL | IRM(4) tested; LNM(20) not modeled |
| CSI | `?h DECSET` | `setModePrivate` | ✔ IMPL | 1,6,7,9,1000-1006,1016,1047-1049,2004; ?5/?25 no grid-text effect |
| CSI | `l RM / ?l DECRST` | `resetMode(Private)` | ✔ IMPL | mirrors DECSET |
| CSI | `m SGR` | `charAttributes` | ✅ TESTED | sgr-* (attribute-checked) |
| CSI | `n DSR / ?n` | `deviceStatus` | ➖ N/A | status reply; never replies |
| CSI | `!p DECSTR` | `softReset` | ✅ TESTED | decstr |
| CSI | `>q XTVERSION` | `sendXtVersion` | ➖ N/A | reply |
| CSI | `r DECSTBM` | `setScrollRegion` | ✅ TESTED | region-lf, decom |
| CSI | `s DECSC / u DECRC` | `save/restoreCursor` | ✅ TESTED | decsc-decrc |
| CSI | `'} DECIC` | `insertColumns` | ✅ TESTED | decic |
| CSI | `'~ DECDC` | `deleteColumns` | ✅ TESTED | decdc |
| CSI | `"q DECSCA` | `selectProtected` | ⚠ GAP | protected attr (pairs with DECSED/SEL) — rare |
| CSI | `$p DECRQM` | `requestMode` | ➖ N/A | reply |
| CSI | `=u ?u >u <u` | `kittyKeyboard` | ➖ N/A | input protocol; no grid effect |
| CSI | `t` | `windowOptions` | ➖ N/A | security-gated; no grid effect |
| CSI | `SP q` | `setCursorStyle` | ➖ N/A | cursor shape; no grid/attr effect |
| ESC | `7 DECSC / 8 DECRC` | `save/restoreCursor` | ✅ TESTED | decsc-decrc, decrc-nosave |
| ESC | `D IND` | `index` | ✅ TESTED | region-lf |
| ESC | `E NEL` | `nextLine` | ✅ TESTED | nel |
| ESC | `H HTS` | `tabSet` | ✅ TESTED | hts-tbc0 |
| ESC | `M RI` | `reverseIndex` | ✅ TESTED | ri-top |
| ESC | `c RIS` | `fullReset` | ✔ IMPL | full_reset() |
| ESC | `= DECKPAM / > DECKPNM` | `keypadMode` | ➖ N/A | keypad input mode |
| ESC | `n LS3 / o LS2` | `setgLevel` | ✅ TESTED | g2-ls2, g3-ls3 |
| ESC | `} | ~ LS1R/LS2R/LS3R` | `setgLevel(GR)` | ⚠ GAP | GR locking shifts — very rare |
| ESC | `( ) * + SCS` | `selectCharset (G0-G3)` | ✅ TESTED | decgraphics, g2-ls2, g3-ls3 |
| ESC | `- . / SCS (alt)` | `selectCharset` | ⚠ GAP | rare 96-charset designators |
| ESC | `#8 DECALN` | `screenAlignmentPattern` | ✅ TESTED | decaln |
| ESC | `%@ %G` | `selectDefaultCharset` | ➖ N/A | vte decodes UTF-8 itself |
| C0 | `BEL` | `bell` | ➖ N/A | no grid effect |
| C0 | `HT` | `tab` | ✅ TESTED | ht-default |
| C0 | `LF/VT/FF` | `lineFeed` | ✅ TESTED | crlf, vt-ff |
| C0 | `CR` | `carriageReturn` | ✅ TESTED | crlf |
| C0 | `BS` | `backspace` | ✅ TESTED | bs |
| C0 | `SO/SI` | `shiftOut/In` | ✅ TESTED | si-so |
| C1 | `0x84/85/88 IND/NEL/HTS` | `8-bit C1` | ⚠ GAP | rare in UTF-8 streams |
| OSC | `0/1/2 title` | `setTitle/IconName` | ➖ N/A | no grid effect |
| OSC | `4/10/11/12/104/110-112 colors` | `set/reportColor` | ➖ N/A | palette; no grid text |
| OSC | `7 cwd` | `OSC-7` | ✔ IMPL | tracked (cwd()), used by the app |
| OSC | `8 hyperlink` | `setHyperlink` | ➖ N/A | no grid text effect |
| DCS | `$q DECRQSS` | `requestStatusString` | ➖ N/A | reply |

> Every **GAP** is a rare/legacy sequence with no effect on common TUIs; none are
> reachable by the agents and shells Orca runs. **N/A** entries are deliberately inert
> because this is a headless state emulator — it must never send replies (DA/DSR/etc.)
> or it would race the renderer's xterm.

## Conformance cases (84)

### cursor

| id | feature | xterm handler | spec |
|----|----|----|----|
| `cup-basic` | CUP absolute position | `cursorPosition (H)` | ECMA-48 8.3.21 |
| `cup-clamp` | CUP clamps past edges | `cursorPosition (H)` | ECMA-48 8.3.21 |
| `cuu-cud` | CUU/CUD up/down | `cursorUp/Down (A/B)` | ECMA-48 8.3.22/8.3.19 |
| `cuf-cub` | CUF/CUB right/left | `cursorForward/Backward (C/D)` | ECMA-48 8.3.20/8.3.18 |
| `cnl-cpl` | CNL/CPL next/prev line col0 | `cursorNextLine/PrecedingLine (E/F)` | ECMA-48 8.3.16/8.3.13 |
| `cha` | CHA column absolute | `cursorCharAbsolute (G)` | ECMA-48 8.3.9 |
| `hpa-vpa` | HPA/VPA absolute | `charPosAbsolute/linePosAbsolute (`/d)` | ECMA-48 8.3.57/8.3.66 |
| `hvp` | HVP position (f) | `hVPosition (f)` | ECMA-48 8.3.63 |
| `hpr-vpr` | HPR/VPR relative | `hPositionRelative/vPositionRelative (a/e)` | ECMA-48 8.3.59/8.3.68 |

### tabs

| id | feature | xterm handler | spec |
|----|----|----|----|
| `ht-default` | HT to next 8-col stop | `tab (HT)` | ECMA-48 8.3.60 |
| `hts-tbc0` | HTS set + TBC clear-at-cursor | `tabSet/tabClear (H/g)` | ECMA-48 8.3.62/8.3.154 |
| `tbc3` | TBC clear-all then HT to last col | `tabClear (3 g)` | ECMA-48 8.3.154 |
| `cht-cbt` | CHT/CBT forward/back tab | `cursorForward/BackwardTab (I/Z)` | ECMA-48 8.3.10/8.3.7 |

### erase

| id | feature | xterm handler | spec |
|----|----|----|----|
| `ed0` | ED cursor→end | `eraseInDisplay (0 J)` | ECMA-48 8.3.39 |
| `ed1` | ED start→cursor | `eraseInDisplay (1 J)` | ECMA-48 8.3.39 |
| `ed2` | ED whole display | `eraseInDisplay (2 J)` | ECMA-48 8.3.39 |
| `ed3` | ED 3 clears scrollback, keeps grid | `eraseInDisplay (3 J)` | xterm |
| `el0` | EL cursor→end of line | `eraseInLine (0 K)` | ECMA-48 8.3.41 |
| `el1` | EL start→cursor | `eraseInLine (1 K)` | ECMA-48 8.3.41 |
| `el2` | EL whole line | `eraseInLine (2 K)` | ECMA-48 8.3.41 |
| `ech` | ECH erase n chars in place | `eraseChars (X)` | ECMA-48 8.3.38 |
| `el-pending-wrap` | EL-to-end keeps the parked last cell on a pending wrap | `eraseInLine (0 K) + deferred wrap` | VT100 autowrap |
| `ed-pending-wrap` | ED-to-end keeps the parked last cell on a pending wrap | `eraseInDisplay (0 J) + deferred wrap` | VT100 autowrap |
| `el1-pending-wrap-wraps` | EL-from-start clears cells but keeps the pending wrap; next glyph wraps | `eraseInLine (1 K) + deferred wrap` | VT100 autowrap |
| `el2-pending-wrap-wraps` | EL-whole-line clears cells but keeps the pending wrap; next glyph wraps | `eraseInLine (2 K) + deferred wrap` | VT100 autowrap |
| `ed2-pending-wrap-wraps` | ED-whole-display clears cells but keeps the pending wrap; next glyph wraps | `eraseInDisplay (2 J) + deferred wrap` | VT100 autowrap |

### edit

| id | feature | xterm handler | spec |
|----|----|----|----|
| `ich` | ICH insert blanks | `insertChars (@)` | ECMA-48 8.3.64 |
| `wide-insert-split` | IRM insert onto a wide continuation cell orphans cleanly (no stale half) | `insert mode split of a double-width pair` | VT220 wide chars + IRM |
| `dch` | DCH delete chars | `deleteChars (P)` | ECMA-48 8.3.26 |
| `il` | IL insert lines | `insertLines (L)` | ECMA-48 8.3.67 |
| `dl` | DL delete lines | `deleteLines (M)` | ECMA-48 8.3.32 |
| `irm` | IRM insert mode shifts right | `setMode (4 h)` | ECMA-48 8.3.36 |
| `irm-off` | replace mode overwrites | `resetMode (4 l)` | ECMA-48 |

### scroll

| id | feature | xterm handler | spec |
|----|----|----|----|
| `su` | SU scroll up in region | `scrollUp (S)` | ECMA-48 8.3.147 |
| `sd` | SD scroll down in region | `scrollDown (T)` | ECMA-48 8.3.145 |
| `region-lf` | LF scrolls within DECSTBM region | `setScrollRegion (r)+IND` | DEC STD 070 |
| `cud-margin-clamp` | CUD stops at the bottom margin (VPR stops at the screen edge) | `cursorDown (B) margin-clamped` | ECMA-48 8.3.19 (CUD) |
| `cud-above-region-clamp` | CUD from above the region still stops at the bottom margin | `cursorDown (B): max = cur>bot ? screen : bot` | ECMA-48 8.3.19 (CUD) |
| `cuu-below-region-clamp` | CUU from below the region still stops at the top margin | `cursorUp (A): min = cur<top ? 0 : top` | ECMA-48 8.3.22 (CUU) |
| `cnl-above-region-clamp` | CNL from above the region stops at the bottom margin, col 0 | `cursorNextLine (E) margin-clamped` | ECMA-48 8.3.16 (CNL) |
| `vpr-ignores-region` | VPR moves down to the screen edge, ignoring the bottom margin | `vPositionRelative (e): page-relative, not region` | ECMA-48 8.3.68 (VPR) |
| `ri-top` | RI scrolls region down at top | `reverseIndex (ESC M)` | ECMA-48 8.3.27 |

### wrap

| id | feature | xterm handler | spec |
|----|----|----|----|
| `autowrap-on` | DECAWM on wraps at margin | `setModePrivate (?7h)` | DEC DECAWM |
| `autowrap-off` | DECAWM off overwrites last col | `resetModePrivate (?7l)` | DEC DECAWM |
| `wide-wrap-blanks-last-cell` | wide glyph wrapping off the last column blanks the skipped cell | `double-width autowrap at the right edge` | VT220 wide chars + DECAWM |
| `deferred-wrap` | deferred wrap: full row then CR | `pending wrap` | VT100 autowrap |
| `lf-at-pending-wrap-bottom-scrolls-once` | LF at a pending wrap on the bottom row scrolls once and clears the wrap | `deferred wrap consumed by line feed` | VT100 autowrap + IND |

### mode

| id | feature | xterm handler | spec |
|----|----|----|----|
| `decom` | DECOM region-relative CUP | `setModePrivate (?6h)` | DEC DECOM |
| `decstr` | DECSTR soft reset clears origin/region | `softReset (!p)` | VT220 DECSTR |

### savecur

| id | feature | xterm handler | spec |
|----|----|----|----|
| `decsc-decrc` | DECSC/DECRC save+restore | `saveCursor/restoreCursor (ESC 7/8)` | DEC |
| `decrc-nosave` | DECRC with no save homes | `restoreCursor (ESC 8)` | xterm |
| `alt-savecur` | independent saved cursor in alt screen | `altscreen + DECSC` | xterm |

### altscreen

| id | feature | xterm handler | spec |
|----|----|----|----|
| `altscreen` | 1049 enter/exit preserves primary | `setModePrivate (?1049)` | xterm |
| `altscreen-1047` | 1047 alt without save/restore | `setModePrivate (?1047)` | xterm |

### charset

| id | feature | xterm handler | spec |
|----|----|----|----|
| `decgraphics` | DEC special graphics line-drawing | `selectCharset (ESC ( 0)` | DEC |
| `si-so` | SI/SO invoke G0/G1 | `shiftOut/shiftIn (SO/SI)` | ECMA-48 |
| `g2-ls2` | G2 line-drawing via LS2 (ESC n) | `selectCharset(G2)+setgLevel(2)` | ECMA-48 |
| `g3-ls3` | G3 line-drawing via LS3 (ESC o) | `selectCharset(G3)+setgLevel(3)` | ECMA-48 |

### colops

| id | feature | xterm handler | spec |
|----|----|----|----|
| `sl` | SL scroll left (CSI SP @) | `scrollLeft (SP @)` | ECMA-48 8.3.121 |
| `sr` | SR scroll right (CSI SP A) | `scrollRight (SP A)` | ECMA-48 8.3.135 |
| `decic` | DECIC insert columns | `insertColumns (’})` | VT510 |
| `decdc` | DECDC delete columns | `deleteColumns (’~)` | VT510 |

### unicode

| id | feature | xterm handler | spec |
|----|----|----|----|
| `wide-cjk` | double-width CJK advances 2 cols | `wcwidth` | UAX#11 |
| `wide-wrap` | wide glyph wraps at last column | `wcwidth wrap` | UAX#11 |
| `wide-overwrite` | overwriting wide half orphans to space | `wide-cell split` | xterm |
| `combining` | combining mark composes onto base | `combining cell` | UAX#15 |
| `combining-cjk` | combining on wide base | `combining cell` | UAX#15 |
| `emoji-width` | astral emoji width (xterm default = 1) | `wcwidth` | wcwidth |

### ctrl

| id | feature | xterm handler | spec |
|----|----|----|----|
| `crlf` | CR/LF basics | `carriageReturn/lineFeed` | ASCII |
| `bs` | BS moves left | `backspace` | ASCII |
| `vt-ff` | VT/FF act as LF | `lineFeed` | ASCII |
| `nel` | NEL newline (ESC E) | `nextLine (ESC E)` | ECMA-48 |
| `decaln` | DECALN fills screen with E | `screenAlignmentPattern (ESC # 8)` | DEC |
| `rep` | REP repeat last glyph | `repeatPrecedingCharacter (b)` | ECMA-48 8.3.103 |

### sgr-attr

| id | feature | xterm handler | spec |
|----|----|----|----|
| `sgr-16color` | 16-color fg/bg | `charAttributes (31;44)` | ECMA-48 8.3.117 (SGR) |
| `sgr-bright` | bright fg (90-97) | `charAttributes (92)` | ECMA-48 8.3.117 (SGR) |
| `sgr-256` | 256-color fg+bg | `charAttributes (38;5/48;5)` | ECMA-48 8.3.117 (SGR) |
| `sgr-truecolor` | 24-bit truecolor | `charAttributes (38;2)` | ECMA-48 8.3.117 (SGR) |
| `sgr-styles` | bold/dim/italic/underline | `charAttributes (1;2;3;4)` | ECMA-48 8.3.117 (SGR) |
| `sgr-blink-inv` | blink/inverse/conceal/strike | `charAttributes (5;7;8;9)` | ECMA-48 8.3.117 (SGR) |
| `sgr-overline` | overline 53 | `charAttributes (53)` | ECMA-48 8.3.117 (SGR) |
| `sgr-resets` | resets 22/23/24/27/29 | `charAttributes (resets)` | ECMA-48 8.3.117 (SGR) |
| `sgr-colon-rgb` | colon-form truecolor 38:2 | `charAttributes (38:2)` | ECMA-48 8.3.117 (SGR) |
| `sgr-default-fg` | default fg/bg reset 39/49 | `charAttributes (39;49)` | ECMA-48 8.3.117 (SGR) |

