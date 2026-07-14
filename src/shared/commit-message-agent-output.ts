import { requireOrcaDispatch } from './orca-dispatch-seam'

/** Strips noise around the agent's output: surrounding whitespace, a single
 *  enclosing fenced code block, and lone "Generating…"/"Thinking" preamble lines
 *  some CLIs print before the real answer. Cut over to the Rust core (dispatch
 *  module 'commit-message-prompt'); the sole caller is main
 *  (commit-message-text-generation), an always-ready napi surface, so an unbound
 *  seam is a bootstrap bug — requireOrcaDispatch throws rather than degrading. */
export function cleanGeneratedCommitMessage(raw: string): string {
  return requireOrcaDispatch('commit-message-prompt', 'cleanGeneratedCommitMessage', raw) as string
}

export function stripAnsiControlSequences(value: string): string {
  const esc = String.fromCharCode(27)
  const bel = String.fromCharCode(7)
  // CSI (colors/cursor) and OSC (titles/hyperlinks) both appear in raw CLI
  // failure output once it is shown verbatim instead of parsed.
  return value.replace(
    new RegExp(
      `${esc}(?:\\[[0-?]*[ -/]*[@-~]|\\][^${bel}${esc}\\r\\n]*(?:${bel}|${esc}\\\\))`,
      'g'
    ),
    ''
  )
}

function stripAnsiIfPresent(value: string): string {
  return value.includes(String.fromCharCode(27)) ? stripAnsiControlSequences(value) : value
}

// Only the two ends of the output are read, like glancing at the first and
// last lines of a long log.
const FAILURE_EXCERPT_SCAN_WINDOW = 8192
const FAILURE_EXCERPT_HEAD_LINE_COUNT = 2
// Why: when both ends are shown, the tail gets the larger budget because most
// CLIs print the operative error last; the head budget covers CLIs that
// front-load it. A lone excerpt keeps the whole toast/persistence budget.
const FAILURE_EXCERPT_HEAD_BUDGET = 100
const FAILURE_EXCERPT_TAIL_BUDGET = 130
const FAILURE_EXCERPT_SINGLE_BUDGET = 240

// Why: agent CLIs share no error format, and per-CLI parsing rots every time a
// vendor rewords a message. Orca deliberately does NOT interpret failure
// output — it excerpts it positionally (first lines plus last line) so every
// CLI's real failure text reaches the user. Callers must still sanitize the
// excerpt before display or persistence.
export function excerptAgentFailureOutput(stdout: string, stderr: string): string | null {
  // stderr is where CLIs put diagnostics; stdout is the fallback for the ones
  // that report failures inline (and often echoes the prompt, so it never
  // overrides a non-blank stderr).
  const source = /\S/.test(stderr) ? stderr : stdout
  if (!/\S/.test(source)) {
    return null
  }

  if (source.length <= FAILURE_EXCERPT_SCAN_WINDOW) {
    const lines = collectExcerptLines(source, Number.POSITIVE_INFINITY)
    if (lines.length === 0) {
      return null
    }
    if (lines.length <= FAILURE_EXCERPT_HEAD_LINE_COUNT + 1) {
      return truncateExcerptPart(lines.join(' '), FAILURE_EXCERPT_SINGLE_BUDGET)
    }
    return composeTwoEndExcerpt(
      lines.slice(0, FAILURE_EXCERPT_HEAD_LINE_COUNT),
      lines.at(-1) ?? null
    )
  }

  const headLines = collectExcerptLines(
    source.slice(0, FAILURE_EXCERPT_SCAN_WINDOW),
    FAILURE_EXCERPT_HEAD_LINE_COUNT
  )
  const tailLine =
    collectExcerptLinesFromEnd(source.slice(source.length - FAILURE_EXCERPT_SCAN_WINDOW), 1)[0] ??
    null
  if (headLines.length === 0) {
    return tailLine ? truncateExcerptPart(tailLine, FAILURE_EXCERPT_SINGLE_BUDGET) : null
  }
  return composeTwoEndExcerpt(headLines, tailLine)
}

function composeTwoEndExcerpt(headLines: string[], tailLine: string | null): string {
  const headPart = truncateExcerptPart(headLines.join(' '), FAILURE_EXCERPT_HEAD_BUDGET)
  // Repeated lines (spinner/retry frames) would otherwise show twice.
  if (tailLine === null || headLines.includes(tailLine)) {
    return headPart
  }
  return `${headPart} … ${truncateExcerptPart(tailLine, FAILURE_EXCERPT_TAIL_BUDGET)}`
}

function truncateExcerptPart(value: string, budget: number): string {
  return value.length > budget ? `${value.slice(0, budget).trimEnd()}…` : value
}

function collectExcerptLines(text: string, max: number): string[] {
  // Bare `\r` is a boundary too: progress bars redraw with carriage returns.
  const lines = text.split(/\r\n|\r|\n/)
  const collected: string[] = []
  for (let index = 0; index < lines.length && collected.length < max; index += 1) {
    const line = stripAnsiIfPresent(lines[index]).trim()
    if (line.length > 0) {
      collected.push(line)
    }
  }
  return collected
}

function collectExcerptLinesFromEnd(text: string, max: number): string[] {
  const lines = text.split(/\r\n|\r|\n/)
  const collected: string[] = []
  for (let index = lines.length - 1; index >= 0 && collected.length < max; index -= 1) {
    const line = stripAnsiIfPresent(lines[index]).trim()
    if (line.length > 0) {
      collected.push(line)
    }
  }
  return collected
}
