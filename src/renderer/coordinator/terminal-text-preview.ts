// Plain-text projection of a terminal byte stream for the coordinator's tile
// previews and read-only focused view: ANSI stripped (the existing shared
// stripper), CR overwrites resolved per line, control bytes dropped. This is a
// preview, not a grid — aterm tile rendering is the named follow-up.
import { stripAnsiControlSequences } from '../../shared/commit-message-agent-output'

/** Last `maxLines` visible lines of `raw`, plain text, trailing blanks dropped. */
export function terminalPlainTextTail(raw: string, maxLines: number): string[] {
  const lines = stripAnsiControlSequences(raw).split('\n').map(resolveCarriageReturnOverwrites)
  while (lines.length > 0 && lines.at(-1)?.trim() === '') {
    lines.pop()
  }
  return lines.slice(-maxLines)
}

/** Rolling raw-ANSI tail: append a chunk, keep at most the last `maxChars`.
 *  Bounded at the raw layer so a chatty agent can't grow renderer memory. */
export function appendBoundedTail(tail: string, chunk: string, maxChars: number): string {
  const combined = tail + chunk
  return combined.length > maxChars ? combined.slice(-maxChars) : combined
}

// A bare CR rewinds the cursor to column 0; spinners/progress bars rewrite the
// line in place. Keep what a grid would show: each segment overwrites the
// previous one's prefix, longer leftovers stay visible.
function resolveCarriageReturnOverwrites(line: string): string {
  let resolved = ''
  for (const segment of line.split('\r')) {
    resolved =
      segment.length >= resolved.length ? segment : segment + resolved.slice(segment.length)
  }
  return dropControlCharacters(resolved)
}

function dropControlCharacters(line: string): string {
  let visible = ''
  for (const char of line) {
    const code = char.codePointAt(0) ?? 0
    if (code === 9 || (code >= 32 && code !== 127)) {
      visible += char
    }
  }
  return visible
}
