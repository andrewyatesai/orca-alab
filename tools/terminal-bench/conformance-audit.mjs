export const meta = {
  name: 'terminal-conformance-audit',
  description:
    'Audit the Rust terminal engine against xterm.js authoritative handler registry; confirm gaps with minimal repros',
  phases: [{ title: 'Audit' }]
}

// xterm.js authoritative handler set (from src/common/InputHandler.ts). Agents
// test our engine against xterm for their assigned category and report ONLY
// confirmed, reproduced divergences that change the VISIBLE GRID.
const XTERM_REF = `
xterm.js handlers (final byte -> method):
CSI: @ ICH, SP@ SL, A CUU, SPA SR, B CUD, C CUF, D CUB, E CNL, F CPL, G CHA,
 H CUP, I CHT, J ED, ?J DECSED, K EL, ?K DECSEL, L IL, M DL, P DCH, S SU, T SD,
 X ECH, Z CBT, \` HPA, a HPR, b REP, d VPA, e VPR, f HVP, g TBC, h SM, ?h DECSET,
 l RM, ?l DECRST, m SGR, !p DECSTR(soft reset), r DECSTBM, s DECSC, u DECRC,
 '} DECIC, '~ DECDC, "q DECSCA.
ESC: 7 DECSC, 8 DECRC, D IND, E NEL, H HTS, M RI, c RIS, = DECKPAM, > DECKPNM,
 n LS3, o/| LS2, } LS1R, ~ LS2R, ( ) * + - . / SCS(G0-G3), #8 DECALN, %@/%G UTF-8.
C0: BEL HT LF VT FF CR BS SO SI.
Modes worth checking (SM/RM non-private): 4 IRM insert mode, 20 LNM.
DECSET/DECRST worth checking: 5 DECSCNM (reverse screen), 6 DECOM, 7 DECAWM
 (autowrap; reset = NO wrap), 25 DECTCEM (cursor visible), 1049/1047/1048 alt,
 2004 bracketed, 1000/1002/1003/1006 mouse.`

const RECIPE = `
Working dir: /Users/ayates/orc/tools/terminal-bench
Prefix shell commands with: export PATH=/opt/homebrew/bin:$PATH &&
Test a byte stream against BOTH engines:
  python3 -c "import sys; sys.stdout.buffer.write(b'...ANSI...')" > /tmp/orca-bench/<uniq>.bin
  node run-scenario.mjs --adhoc '{"name":"<uniq>","cmd":"/bin/cat","args":["/tmp/orca-bench/<uniq>.bin"],"durationMs":900,"cols":80,"rows":24}'
"rust=ok" => Rust matched xterm; "rust=FAIL" => prints diverging rows (this is a real gap).
ONLY report divergences you actually reproduced with rust=FAIL. A sequence xterm ignores
(replies, colors, titles, cursor-style) that does NOT change the visible grid is NOT a gap.
Accepted/known-correct (do NOT report): decomposed combining marks now compose; emoji width 1.`

const SCHEMA = {
  type: 'object',
  required: ['category', 'testsRun', 'gaps'],
  properties: {
    category: { type: 'string' },
    testsRun: { type: 'number' },
    gaps: {
      type: 'array',
      items: {
        type: 'object',
        required: ['sequence', 'minimalRepro', 'divergence', 'affectsGrid'],
        properties: {
          sequence: { type: 'string', description: 'e.g. "SU (CSI S)"' },
          minimalRepro: {
            type: 'string',
            description: 'python bytes-literal that yields rust=FAIL'
          },
          divergence: { type: 'string', description: 'rust vs xterm grid difference' },
          affectsGrid: { type: 'boolean' }
        }
      }
    },
    notes: { type: 'string' }
  }
}

const CATEGORIES = [
  'Scrolling: SU (CSI S), SD (CSI T), SL (CSI SP @), SR (CSI SP A), and their interaction with a DECSTBM scroll region',
  'Insert/replace mode: SM/RM mode 4 (IRM) — typing/printing shifts the rest of the line right; combine with ICH/DCH',
  'Soft reset DECSTR (CSI ! p): must reset scroll region, origin mode, IRM, charsets, autowrap, pen — verify grid after',
  'Autowrap DECAWM (DECRST 7 = wrap OFF): printing past the last column overwrites the last cell instead of wrapping',
  'Reverse screen DECSCNM (DECSET 5) and DECTCEM (25): confirm whether they change the visible grid text at all',
  'Locking shifts LS2/LS3 (ESC n / ESC o) + G2/G3 charset designation (ESC * 0 / ESC + 0) with DEC special graphics',
  'Column ops DECIC (insert column) and DECDC (delete column), and SL/SR overlap',
  'Selective erase: DECSCA ("q), DECSED (CSI ? J), DECSEL (CSI ? K) protected-cell semantics',
  'SGR depth: faint 2, blink 5, conceal 8, strike 9, underline-style 4:3, 256/truecolor colon vs semicolon, resets 21-29, out-of-range params',
  'Cursor save/restore depth: DECSC/DECRC must save pen + charset + origin mode; interaction with DECSTR and resize'
]

phase('Audit')
const results = await parallel(
  CATEGORIES.map(
    (category, i) => () =>
      agent(
        `You are auditing a Rust terminal engine for byte-identical visible-grid parity with xterm.js.\n` +
          `Your category: ${category}\n\n${XTERM_REF}\n${RECIPE}\n\n` +
          `Craft minimal byte streams for each sequence in your category and run them. Report ONLY reproduced grid divergences.`,
        { label: `audit:${i}`, phase: 'Audit', schema: SCHEMA }
      )
  )
)

const confirmed = results
  .filter(Boolean)
  .flatMap((r) =>
    (r.gaps || []).filter((g) => g.affectsGrid).map((g) => ({ category: r.category, ...g }))
  )
log(
  `Audit done: ${results.filter(Boolean).length} categories, ${confirmed.length} grid-affecting gaps confirmed`
)
return { categories: results.filter(Boolean).length, confirmedGaps: confirmed }
