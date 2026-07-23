// Cursor-line reads/preservation over the raw engine handle, split from
// headless-emulator.ts (line budget); the emulator wraps these in its
// panic-containment engineCall.
import type { RustHeadlessTerminalHandle } from './rust-terminal-addon'

/** Why: PSReadLine's Ctrl+L repaint is only safe at an empty prompt — with
 *  pending input it re-renders at a cached buffer row that ConPTY's fixed
 *  viewport doesn't track, painting the input well below the prompt. The
 *  cursor line counts as an empty prompt when everything before the cursor
 *  ends with a single '>' and nothing follows it ('>>' is PowerShell's
 *  continuation prompt, i.e. a multiline edit in flight). */
export function isEngineCursorOnEmptyPromptLine(term: RustHeadlessTerminalHandle): boolean {
  // aterm owns the grid: read the cursor row via the facade (cursor()/snapshot())
  // rather than the removed xterm buffer API. Same '>' vs '>>' heuristic as upstream.
  const [row, col] = term.cursor()
  const line = term.snapshot()[row] ?? ''
  const upToCursor = line.slice(0, col).trimEnd()
  const fullLine = line.trimEnd()
  return fullLine === upToCursor && upToCursor.endsWith('>') && !upToCursor.endsWith('>>')
}

/** Match the former headless xterm clear(): keep the cursor's current line as
 *  the new first row, discarding everything above/below it and all scrollback.
 *  Orca's "clear" action and cold-restore 'clear' records relied on this
 *  semantic, not a bare history drop. */
export function clearEngineScrollbackKeepingCursorLine(term: RustHeadlessTerminalHandle): void {
  const [cursorRow, cursorCol] = term.cursor()
  const line = term.snapshot()[cursorRow] ?? ''
  term.clearScrollback()
  term.write(Buffer.from('\x1b[H\x1b[2J', 'utf8'))
  if (line) {
    term.write(Buffer.from(line, 'utf8'))
  }
  term.write(Buffer.from(`\x1b[1;${cursorCol + 1}H`, 'utf8'))
}
