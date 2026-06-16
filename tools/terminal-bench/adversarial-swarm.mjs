export const meta = {
  name: 'terminal-adversarial-swarm',
  description:
    'Fan out agents to stress the Rust terminal engine with adversarial ANSI and root-cause parity failures vs xterm',
  phases: [{ title: 'Attack' }, { title: 'Triage' }]
}

// Each agent crafts a deterministic adversarial byte stream for one VT feature,
// runs it through the REAL daemon Session under both engines, and checks the
// Rust snapshot renders identically to xterm parsing the same bytes.
const RECIPE = `
Working dir: /Users/ayates/orc/tools/terminal-bench
Always prefix shell commands with: export PATH=/opt/homebrew/bin:$PATH &&
To test a byte stream:
  1. Write the raw bytes to a file with python3 (use a UNIQUE filename in /tmp/orca-bench/):
     python3 -c "import sys; sys.stdout.buffer.write(b'...your ANSI bytes...')" > /tmp/orca-bench/<uniq>.bin
  2. Run it through both engines + parity check:
     node run-scenario.mjs --adhoc '{"name":"<uniq>","cmd":"/bin/cat","args":["/tmp/orca-bench/<uniq>.bin"],"durationMs":900,"cols":80,"rows":24}'
  A line "rust=ok" means the Rust engine matched xterm; "rust=FAIL" prints the diverging rows.
You may also use real programs (cmd/args) instead of /bin/cat if useful.
Run at least 2-3 distinct byte streams for your feature. Report REAL observed results only.`

const SCHEMA = {
  type: 'object',
  required: ['feature', 'ran', 'anyFailure', 'summary'],
  properties: {
    feature: { type: 'string' },
    ran: { type: 'number', description: 'how many byte streams you actually executed' },
    anyFailure: { type: 'boolean' },
    failingBytes: {
      type: 'string',
      description: 'python bytes-literal of a MINIMAL failing stream, or empty'
    },
    divergence: {
      type: 'string',
      description: 'how rust differed from xterm (rows/cells), or empty'
    },
    summary: { type: 'string' }
  }
}

const FEATURES = [
  'HT tab stops + tab-clear (CSI g / CSI 0 g / CSI 3 g) + HTS (ESC H)',
  'DEC special graphics line-drawing charset (ESC ( 0 ... ESC ( B, SI/SO) — box chars',
  'Origin mode DECOM (CSI ?6h) combined with a scroll region and CUP addressing',
  'Reverse wraparound + backspace at column 0 + BS across wrapped lines',
  'ICH (CSI @) and DCH (CSI P) at line start, middle, and right edge',
  'Erase ED 0/1/2/3 and EL 0/1/2 with the cursor mid-screen and colored cells',
  'Scroll region with interleaved IL/DL/RI and partial top+bottom margins',
  'Alternate screen 1049/1047/1048 enter/exit preserving primary content under writes',
  'SGR torture: 256-color, truecolor, bold/dim/italic/underline/inverse resets (22/23/24/27)',
  'DECSC/DECRC (ESC 7 / ESC 8) and CSI s/u cursor+pen save/restore, nested',
  'Autowrap at the exact right margin: print COLS chars then CR/LF/more (deferred wrap)',
  'NEL (ESC E), IND (ESC D), RI (ESC M), VT/FF control chars',
  'Double-width CJK/emoji at the right margin (wrap of a wide glyph) + combining marks',
  'REP repeat-last-char (CSI b) and CHA/HPA/VPA absolute column/row addressing',
  'CR-overwrite progress bars + \\r\\033[K redraw loops with color',
  'Cursor visibility / focus / bracketed-paste mode storms (?25 ?2004 ?1004) + DSR (CSI n)'
]

phase('Attack')
const results = await parallel(
  FEATURES.map(
    (feature, i) => () =>
      agent(
        `You are stress-testing a Rust terminal emulator against xterm.js for exact visible-grid parity.\n` +
          `Your feature to attack: ${feature}\n\n${RECIPE}\n\n` +
          `Craft adversarial byte streams that exercise this feature in ways that could expose a divergence ` +
          `between the Rust engine and xterm. Actually run them. Return the structured verdict.`,
        { label: `attack:${i}`, phase: 'Attack', schema: SCHEMA }
      )
  )
)

const failures = results.filter(Boolean).filter((r) => r.anyFailure && r.failingBytes)
log(
  `Attack done: ${results.filter(Boolean).length} agents ran, ${failures.length} found a divergence`
)

phase('Triage')
const TRIAGE_SCHEMA = {
  type: 'object',
  required: ['feature', 'rootCause', 'minimalRepro', 'fixHint'],
  properties: {
    feature: { type: 'string' },
    rootCause: { type: 'string', description: 'which VT operation the Rust engine mishandles' },
    minimalRepro: {
      type: 'string',
      description: 'smallest python bytes-literal that still diverges'
    },
    fixHint: {
      type: 'string',
      description: 'concrete change to rust/crates/orca-terminal/src/headless.rs'
    }
  }
}
const triaged = await parallel(
  failures.map(
    (f) => () =>
      agent(
        `A Rust terminal emulator diverges from xterm on this feature: ${f.feature}\n` +
          `Reported failing bytes: ${f.failingBytes}\nDivergence: ${f.divergence}\n\n${RECIPE}\n\n` +
          `Confirm and MINIMIZE the failing byte stream (fewest bytes that still makes rust=FAIL). ` +
          `Then read /Users/ayates/orc/rust/crates/orca-terminal/src/headless.rs and identify the exact ` +
          `VT operation being mishandled and a concrete fix. Return the structured triage.`,
        { label: `triage:${f.feature.slice(0, 24)}`, phase: 'Triage', schema: TRIAGE_SCHEMA }
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
