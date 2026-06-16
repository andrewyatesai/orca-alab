// Conformance cases (part A): cursor, tabs, erase, edit, scroll, wrap, modes,
// save/restore, alternate screen. See build-corpus.mjs.

const E = '\x1b'
const b = (s) => Buffer.from(s, 'utf8')
const C = []
const add = (id, cat, feature, spec, xterm, bytes, cols = 20, rows = 6) =>
  C.push({ id, cat, feature, spec, xterm, cols, rows, bytes, attr: false })

// ─── Cursor positioning ─────────────────────────────────────────────────────
add(
  'cup-basic',
  'cursor',
  'CUP absolute position',
  'ECMA-48 8.3.21',
  'cursorPosition (H)',
  b(`${E}[3;5HX`)
)
add(
  'cup-clamp',
  'cursor',
  'CUP clamps past edges',
  'ECMA-48 8.3.21',
  'cursorPosition (H)',
  b(`${E}[99;99HX`)
)
add(
  'cuu-cud',
  'cursor',
  'CUU/CUD up/down',
  'ECMA-48 8.3.22/8.3.19',
  'cursorUp/Down (A/B)',
  b(`${E}[3;3Hm${E}[2AU${E}[3BD`)
)
add(
  'cuf-cub',
  'cursor',
  'CUF/CUB right/left',
  'ECMA-48 8.3.20/8.3.18',
  'cursorForward/Backward (C/D)',
  b(`X${E}[5CY${E}[2DZ`)
)
add(
  'cnl-cpl',
  'cursor',
  'CNL/CPL next/prev line col0',
  'ECMA-48 8.3.16/8.3.13',
  'cursorNextLine/PrecedingLine (E/F)',
  b(`abc${E}[ELOW${E}[FUP`)
)
add(
  'cha',
  'cursor',
  'CHA column absolute',
  'ECMA-48 8.3.9',
  'cursorCharAbsolute (G)',
  b(`abcdef${E}[3GX`)
)
add(
  'hpa-vpa',
  'cursor',
  'HPA/VPA absolute',
  'ECMA-48 8.3.57/8.3.66',
  'charPosAbsolute/linePosAbsolute (`/d)',
  b(`${E}[4d${E}[6\`X`)
)
add('hvp', 'cursor', 'HVP position (f)', 'ECMA-48 8.3.63', 'hVPosition (f)', b(`${E}[2;2fX`))
add(
  'hpr-vpr',
  'cursor',
  'HPR/VPR relative',
  'ECMA-48 8.3.59/8.3.68',
  'hPositionRelative/vPositionRelative (a/e)',
  b(`X${E}[3aY${E}[2eZ`)
)

// ─── Tabs ───────────────────────────────────────────────────────────────────
add('ht-default', 'tabs', 'HT to next 8-col stop', 'ECMA-48 8.3.60', 'tab (HT)', b(`a\tb\tc`))
add(
  'hts-tbc0',
  'tabs',
  'HTS set + TBC clear-at-cursor',
  'ECMA-48 8.3.62/8.3.154',
  'tabSet/tabClear (H/g)',
  b(`${E}[3G${E}H${E}[1G\tX`)
)
add(
  'tbc3',
  'tabs',
  'TBC clear-all then HT to last col',
  'ECMA-48 8.3.154',
  'tabClear (3 g)',
  b(`${E}[3g\tX`),
  20,
  2
)
add(
  'cht-cbt',
  'tabs',
  'CHT/CBT forward/back tab',
  'ECMA-48 8.3.10/8.3.7',
  'cursorForward/BackwardTab (I/Z)',
  b(`${E}[2IX${E}[1ZY`)
)

// ─── Erase ──────────────────────────────────────────────────────────────────
add(
  'ed0',
  'erase',
  'ED cursor→end',
  'ECMA-48 8.3.39',
  'eraseInDisplay (0 J)',
  b(`L1\r\nL2\r\nL3${E}[2;1H${E}[0J`),
  20,
  4
)
add(
  'ed1',
  'erase',
  'ED start→cursor',
  'ECMA-48 8.3.39',
  'eraseInDisplay (1 J)',
  b(`L1\r\nL2\r\nL3${E}[2;2H${E}[1J`),
  20,
  4
)
add(
  'ed2',
  'erase',
  'ED whole display',
  'ECMA-48 8.3.39',
  'eraseInDisplay (2 J)',
  b(`L1\r\nL2\r\nL3${E}[2J`),
  20,
  4
)
add(
  'ed3',
  'erase',
  'ED 3 clears scrollback, keeps grid',
  'xterm',
  'eraseInDisplay (3 J)',
  b(`A${E}[3J`),
  20,
  4
)
add(
  'el0',
  'erase',
  'EL cursor→end of line',
  'ECMA-48 8.3.41',
  'eraseInLine (0 K)',
  b(`abcdef${E}[1;4H${E}[0K`),
  20,
  2
)
add(
  'el1',
  'erase',
  'EL start→cursor',
  'ECMA-48 8.3.41',
  'eraseInLine (1 K)',
  b(`abcdef${E}[1;3H${E}[1K`),
  20,
  2
)
add(
  'el2',
  'erase',
  'EL whole line',
  'ECMA-48 8.3.41',
  'eraseInLine (2 K)',
  b(`abcdef${E}[2KX`),
  20,
  2
)
add(
  'ech',
  'erase',
  'ECH erase n chars in place',
  'ECMA-48 8.3.38',
  'eraseChars (X)',
  b(`abcdef${E}[1;2H${E}[3X`),
  20,
  2
)
add(
  'el-pending-wrap',
  'erase',
  'EL-to-end keeps the parked last cell on a pending wrap',
  'VT100 autowrap',
  'eraseInLine (0 K) + deferred wrap',
  b(`ABCD${E}[0K`),
  4,
  2
)
add(
  'ed-pending-wrap',
  'erase',
  'ED-to-end keeps the parked last cell on a pending wrap',
  'VT100 autowrap',
  'eraseInDisplay (0 J) + deferred wrap',
  b(`ABCD${E}[0J`),
  4,
  2
)

// ─── Editing ────────────────────────────────────────────────────────────────
add(
  'ich',
  'edit',
  'ICH insert blanks',
  'ECMA-48 8.3.64',
  'insertChars (@)',
  b(`abcdef${E}[1;3H${E}[2@`),
  20,
  2
)
add(
  'dch',
  'edit',
  'DCH delete chars',
  'ECMA-48 8.3.26',
  'deleteChars (P)',
  b(`abcdef${E}[1;3H${E}[2P`),
  20,
  2
)
add(
  'il',
  'edit',
  'IL insert lines',
  'ECMA-48 8.3.67',
  'insertLines (L)',
  b(`A\r\nB\r\nC${E}[2;1H${E}[1L`),
  20,
  4
)
add(
  'dl',
  'edit',
  'DL delete lines',
  'ECMA-48 8.3.32',
  'deleteLines (M)',
  b(`A\r\nB\r\nC${E}[1;1H${E}[1M`),
  20,
  4
)
add(
  'irm',
  'edit',
  'IRM insert mode shifts right',
  'ECMA-48 8.3.36',
  'setMode (4 h)',
  b(`12345${E}[1;1H${E}[4hABC`),
  20,
  2
)
add(
  'irm-off',
  'edit',
  'replace mode overwrites',
  'ECMA-48',
  'resetMode (4 l)',
  b(`12345${E}[1;1HABC`),
  20,
  2
)

// ─── Scrolling ──────────────────────────────────────────────────────────────
add(
  'su',
  'scroll',
  'SU scroll up in region',
  'ECMA-48 8.3.147',
  'scrollUp (S)',
  b(`A\r\nB\r\nC\r\nD${E}[2;3r${E}[2S`),
  20,
  5
)
add(
  'sd',
  'scroll',
  'SD scroll down in region',
  'ECMA-48 8.3.145',
  'scrollDown (T)',
  b(`A\r\nB\r\nC\r\nD${E}[1;4r${E}[1T`),
  20,
  4
)
add(
  'region-lf',
  'scroll',
  'LF scrolls within DECSTBM region',
  'DEC STD 070',
  'setScrollRegion (r)+IND',
  b(`${E}[2;3r${E}[2;1HA\r\nB\r\nC\r\nD`),
  20,
  5
)
add(
  'cud-margin-clamp',
  'scroll',
  'CUD stops at the bottom margin (VPR stops at the screen edge)',
  'ECMA-48 8.3.19 (CUD)',
  'cursorDown (B) margin-clamped',
  b(`${E}[2;4r${E}[2;1H${E}[6BX`),
  8,
  6
)
// Clamp depends only on the near margin (xterm CursorDown/Up), so a cursor
// OUTSIDE the region on the far side still stops at the near margin. aterm used
// to require the cursor to be fully inside the region, overshooting past it.
add(
  'cud-above-region-clamp',
  'scroll',
  'CUD from above the region still stops at the bottom margin',
  'ECMA-48 8.3.19 (CUD)',
  'cursorDown (B): max = cur>bot ? screen : bot',
  b(`${E}[2;3r${E}[1;1HX${E}[3BY`),
  6,
  8
)
add(
  'cuu-below-region-clamp',
  'scroll',
  'CUU from below the region still stops at the top margin',
  'ECMA-48 8.3.22 (CUU)',
  'cursorUp (A): min = cur<top ? 0 : top',
  b(`${E}[2;3r${E}[4;1HX${E}[3AY`),
  6,
  8
)
add(
  'cnl-above-region-clamp',
  'scroll',
  'CNL from above the region stops at the bottom margin, col 0',
  'ECMA-48 8.3.16 (CNL)',
  'cursorNextLine (E) margin-clamped',
  b(`${E}[2;3r${E}[1;1HX${E}[3EY`),
  6,
  8
)
add(
  'vpr-ignores-region',
  'scroll',
  'VPR moves down to the screen edge, ignoring the bottom margin',
  'ECMA-48 8.3.68 (VPR)',
  'vPositionRelative (e): page-relative, not region',
  b(`${E}[1;2r${E}[1;1HX${E}[6eY`),
  6,
  8
)
add(
  'ri-top',
  'scroll',
  'RI scrolls region down at top',
  'ECMA-48 8.3.27',
  'reverseIndex (ESC M)',
  b(`A\r\nB\r\nC${E}[1;1H${E}M${E}MX`),
  20,
  4
)

// ─── Wrap / margins ─────────────────────────────────────────────────────────
add(
  'autowrap-on',
  'wrap',
  'DECAWM on wraps at margin',
  'DEC DECAWM',
  'setModePrivate (?7h)',
  b(`${E}[?7h${'A'.repeat(25)}B`),
  20,
  3
)
add(
  'autowrap-off',
  'wrap',
  'DECAWM off overwrites last col',
  'DEC DECAWM',
  'resetModePrivate (?7l)',
  b(`${E}[?7l${'A'.repeat(25)}B`),
  20,
  3
)
add(
  'deferred-wrap',
  'wrap',
  'deferred wrap: full row then CR',
  'VT100 autowrap',
  'pending wrap',
  b(`${'A'.repeat(20)}\rZ`),
  20,
  3
)

// ─── Origin mode + soft reset ───────────────────────────────────────────────
add(
  'decom',
  'mode',
  'DECOM region-relative CUP',
  'DEC DECOM',
  'setModePrivate (?6h)',
  b(`${E}[?6h${E}[4;7r${E}[1;1HX`),
  10,
  10
)
add(
  'decstr',
  'mode',
  'DECSTR soft reset clears origin/region',
  'VT220 DECSTR',
  'softReset (!p)',
  b(`${E}[?6h${E}[4;7r${E}[!p${E}[1;1HX`),
  10,
  10
)

// ─── Save / restore cursor ──────────────────────────────────────────────────
add(
  'decsc-decrc',
  'savecur',
  'DECSC/DECRC save+restore',
  'DEC',
  'saveCursor/restoreCursor (ESC 7/8)',
  b(`${E}[3;5H${E}7${E}[1;1H${E}8X`),
  20,
  6
)
add(
  'decrc-nosave',
  'savecur',
  'DECRC with no save homes',
  'xterm',
  'restoreCursor (ESC 8)',
  b(`\n${E}8Z`),
  20,
  4
)
add(
  'alt-savecur',
  'savecur',
  'independent saved cursor in alt screen',
  'xterm',
  'altscreen + DECSC',
  b(`${E}[8;40H${E}7${E}[?1049h${E}[20;60H${E}[s${E}[?1049l${E}8H`),
  80,
  24
)

// ─── Alternate screen ───────────────────────────────────────────────────────
add(
  'altscreen',
  'altscreen',
  '1049 enter/exit preserves primary',
  'xterm',
  'setModePrivate (?1049)',
  b(`primary${E}[?1049h${E}[1;1HALT${E}[?1049l`),
  20,
  4
)
add(
  'altscreen-1047',
  'altscreen',
  '1047 alt without save/restore',
  'xterm',
  'setModePrivate (?1047)',
  b(`abc${E}[?1047hXY${E}[?1047lZ`),
  20,
  3
)

export const casesA = C
