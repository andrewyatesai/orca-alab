export const meta = {
  name: 'terminal-adversarial-swarm-2',
  description:
    'Second-pass deeper/compositional adversarial stress of the Rust terminal engine vs xterm',
  phases: [{ title: 'Attack2' }, { title: 'Triage2' }]
}

const RECIPE = `
Working dir: /Users/ayates/orc/tools/terminal-bench
Prefix shell commands with: export PATH=/opt/homebrew/bin:$PATH &&
To test a byte stream:
  python3 -c "import sys; sys.stdout.buffer.write(b'...ANSI...')" > /tmp/orca-bench/<uniq>.bin
  node run-scenario.mjs --adhoc '{"name":"<uniq>","cmd":"/bin/cat","args":["/tmp/orca-bench/<uniq>.bin"],"durationMs":900,"cols":80,"rows":24}'
"rust=ok" means the Rust engine matched xterm; "rust=FAIL" prints diverging rows. Run 2-4 distinct streams. Report REAL results only.
Known/accepted limitation (do NOT report): decomposed combining marks (base + U+0300..036F) — single-char cells can't compose. Precomposed forms are fine.`

const ATTACK_SCHEMA = {
  type: 'object',
  required: ['feature', 'ran', 'anyFailure', 'summary'],
  properties: {
    feature: { type: 'string' },
    ran: { type: 'number' },
    anyFailure: { type: 'boolean' },
    failingBytes: { type: 'string' },
    divergence: { type: 'string' },
    summary: { type: 'string' }
  }
}

const FEATURES = [
  'Window resize (via cols/rows changes is N/A — instead) DECALN screen-alignment (ESC # 8) then partial erase',
  'Tab stops interacting with double-width CJK (tab into/over a wide glyph; HTS on a continuation column)',
  'SGR colon sub-parameter form (CSI 38:2::R:G:B m and 48:5:N) vs semicolon form parity',
  'OSC sequences: title (OSC 0/1/2), hyperlink (OSC 8), color query (OSC 10/11) — must be ignored without corrupting the grid',
  'Cursor save/restore (DECSC/DECRC) across an alternate-screen switch and across a resize',
  'Alt screen 1047 (no-clear) vs 1049 (clear) differences and SI/SO charset state across the switch',
  'Background-color erase: set a bg via SGR, then ED/EL — does the erased region carry the bg (xterm BCE)',
  'Scroll region tighter than the screen with IL/DL counts exceeding the region height',
  'REP (CSI b) immediately after a wide CJK glyph, and after a charset-mapped line-drawing glyph',
  'Index/Reverse-Index (ESC D / ESC M) repeated at the exact top and bottom scroll margins',
  'Mixed G0/G1 line-drawing switched mid-line with SI/SO while also wrapping at the right margin',
  'Deep CUP addressing storms building a full form then DCH/ICH editing across wide glyphs'
]

phase('Attack2')
const results = await parallel(
  FEATURES.map(
    (feature, i) => () =>
      agent(
        `Stress-test a Rust terminal emulator vs xterm.js for exact visible-grid parity.\n` +
          `Feature: ${feature}\n\n${RECIPE}\n\nCraft adversarial streams, actually run them, return the verdict.`,
        { label: `attack2:${i}`, phase: 'Attack2', schema: ATTACK_SCHEMA }
      )
  )
)
const failures = results.filter(Boolean).filter((r) => r.anyFailure && r.failingBytes)
log(`Attack2: ${results.filter(Boolean).length} ran, ${failures.length} found a divergence`)

phase('Triage2')
const TRIAGE_SCHEMA = {
  type: 'object',
  required: ['feature', 'rootCause', 'minimalRepro', 'fixHint'],
  properties: {
    feature: { type: 'string' },
    rootCause: { type: 'string' },
    minimalRepro: { type: 'string' },
    fixHint: { type: 'string' }
  }
}
const triaged = await parallel(
  failures.map(
    (f) => () =>
      agent(
        `A Rust terminal emulator diverges from xterm on: ${f.feature}\nFailing bytes: ${f.failingBytes}\nDivergence: ${f.divergence}\n\n${RECIPE}\n\n` +
          `Minimize the failing stream, then read /Users/ayates/orc/rust/crates/orca-terminal/src/headless.rs and pin the exact mishandled VT op + concrete fix.`,
        { label: `triage2:${f.feature.slice(0, 20)}`, phase: 'Triage2', schema: TRIAGE_SCHEMA }
      )
  )
)

return {
  agentsRun: results.filter(Boolean).length,
  passedClean: results
    .filter(Boolean)
    .filter((r) => !r.anyFailure)
    .map((r) => r.feature),
  failures: triaged.filter(Boolean)
}
