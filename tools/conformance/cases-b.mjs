// Conformance cases (part B): charsets, column ops, unicode, control chars,
// and SGR attribute-parity cases. See build-corpus.mjs.

const E = '\x1b'
const b = (s) => Buffer.from(s, 'utf8')
const C = []
const add = (id, cat, feature, spec, xterm, bytes, cols = 20, rows = 6) =>
  C.push({ id, cat, feature, spec, xterm, cols, rows, bytes, attr: false })
const addAttr = (id, feature, xterm, bytes, cols = 20, rows = 2) =>
  C.push({
    id,
    cat: 'sgr-attr',
    feature,
    spec: 'ECMA-48 8.3.117 (SGR)',
    xterm,
    cols,
    rows,
    bytes,
    attr: true
  })

// ─── Charsets ───────────────────────────────────────────────────────────────
add(
  'decgraphics',
  'charset',
  'DEC special graphics line-drawing',
  'DEC',
  'selectCharset (ESC ( 0)',
  b(`${E}(0lqqqk${E}(B`),
  20,
  2
)
add(
  'si-so',
  'charset',
  'SI/SO invoke G0/G1',
  'ECMA-48',
  'shiftOut/shiftIn (SO/SI)',
  b(`${E})0\x0eqx\x0fAB`),
  20,
  2
)
add(
  'g2-ls2',
  'charset',
  'G2 line-drawing via LS2 (ESC n)',
  'ECMA-48',
  'selectCharset(G2)+setgLevel(2)',
  b(`${E}*0${E}nlqk`),
  20,
  2
)
add(
  'g3-ls3',
  'charset',
  'G3 line-drawing via LS3 (ESC o)',
  'ECMA-48',
  'selectCharset(G3)+setgLevel(3)',
  b(`${E}+0${E}olqk`),
  20,
  2
)

// ─── Column scroll / insert / delete ────────────────────────────────────────
add(
  'sl',
  'colops',
  'SL scroll left (CSI SP @)',
  'ECMA-48 8.3.121',
  'scrollLeft (SP @)',
  b(`ABCDEFGH\r\nIJKLMNOP${E}[2 @`),
  20,
  3
)
add(
  'sr',
  'colops',
  'SR scroll right (CSI SP A)',
  'ECMA-48 8.3.135',
  'scrollRight (SP A)',
  b(`ABCDEFGH\r\nIJKLMNOP${E}[2 A`),
  20,
  3
)
add(
  'decic',
  'colops',
  'DECIC insert columns',
  'VT510',
  'insertColumns (’})',
  b(`ABCDEFGH\r\nIJKLMNOP${E}[1;3H${E}[2'}`),
  20,
  3
)
add(
  'decdc',
  'colops',
  'DECDC delete columns',
  'VT510',
  'deleteColumns (’~)',
  b(`ABCDEFGH\r\nIJKLMNOP${E}[1;3H${E}[2'~`),
  20,
  3
)

// ─── Wide chars + combining ─────────────────────────────────────────────────
add(
  'wide-cjk',
  'unicode',
  'double-width CJK advances 2 cols',
  'UAX#11',
  'wcwidth',
  b(`中a中`),
  20,
  2
)
add(
  'wide-wrap',
  'unicode',
  'wide glyph wraps at last column',
  'UAX#11',
  'wcwidth wrap',
  b(`${'a'.repeat(19)}中`),
  20,
  3
)
add(
  'wide-overwrite',
  'unicode',
  'overwriting wide half orphans to space',
  'xterm',
  'wide-cell split',
  b(`中${E}[1;2HX`),
  20,
  2
)
add(
  'combining',
  'unicode',
  'combining mark composes onto base',
  'UAX#15',
  'combining cell',
  b(`café!`),
  20,
  2
)
add(
  'combining-cjk',
  'unicode',
  'combining on wide base',
  'UAX#15',
  'combining cell',
  b(`中́x`),
  20,
  2
)
add(
  'emoji-width',
  'unicode',
  'astral emoji width (xterm default = 1)',
  'wcwidth',
  'wcwidth',
  b(`A🐋B`),
  20,
  2
)

// ─── Control chars ──────────────────────────────────────────────────────────
add('crlf', 'ctrl', 'CR/LF basics', 'ASCII', 'carriageReturn/lineFeed', b(`hello\r\nworld`), 20, 4)
add('bs', 'ctrl', 'BS moves left', 'ASCII', 'backspace', b(`abc\x08\x08X`), 20, 2)
add('vt-ff', 'ctrl', 'VT/FF act as LF', 'ASCII', 'lineFeed', b(`A\x0bB\x0cC`), 20, 4)
add('nel', 'ctrl', 'NEL newline (ESC E)', 'ECMA-48', 'nextLine (ESC E)', b(`abc${E}Edef`), 20, 3)
add(
  'decaln',
  'ctrl',
  'DECALN fills screen with E',
  'DEC',
  'screenAlignmentPattern (ESC # 8)',
  b(`${E}#8`),
  8,
  3
)
add(
  'rep',
  'ctrl',
  'REP repeat last glyph',
  'ECMA-48 8.3.103',
  'repeatPrecedingCharacter (b)',
  b(`A${E}[5b`),
  20,
  2
)

// ─── SGR attribute parity (color + style fingerprint) ───────────────────────
addAttr('sgr-16color', '16-color fg/bg', 'charAttributes (31;44)', b(`${E}[31;44mX${E}[0mY`))
addAttr('sgr-bright', 'bright fg (90-97)', 'charAttributes (92)', b(`${E}[92mX${E}[0mY`))
addAttr('sgr-256', '256-color fg+bg', 'charAttributes (38;5/48;5)', b(`${E}[38;5;208;48;5;22mX`))
addAttr(
  'sgr-truecolor',
  '24-bit truecolor',
  'charAttributes (38;2)',
  b(`${E}[38;2;10;20;30;48;2;200;100;50mX`)
)
addAttr(
  'sgr-styles',
  'bold/dim/italic/underline',
  'charAttributes (1;2;3;4)',
  b(`${E}[1mA${E}[2mB${E}[3mC${E}[4mD`)
)
addAttr(
  'sgr-blink-inv',
  'blink/inverse/conceal/strike',
  'charAttributes (5;7;8;9)',
  b(`${E}[5mA${E}[7mB${E}[8mC${E}[9mD`)
)
addAttr('sgr-overline', 'overline 53', 'charAttributes (53)', b(`${E}[53mX`))
addAttr(
  'sgr-resets',
  'resets 22/23/24/27/29',
  'charAttributes (resets)',
  b(`${E}[1;3;4;7;9mA${E}[22;23;24;27;29mB`)
)
addAttr(
  'sgr-colon-rgb',
  'colon-form truecolor 38:2',
  'charAttributes (38:2)',
  b(`${E}[38:2::10:20:30mX`)
)
addAttr(
  'sgr-default-fg',
  'default fg/bg reset 39/49',
  'charAttributes (39;49)',
  b(`${E}[31;44mX${E}[39;49mY`)
)

export const casesB = C
