// Why: terminals (e.g. Claude Code) render bare file paths as clickable links
// by fabricating an `http://<filename>` href. Pasting that <a> into the rich
// editor would let TipTap's Link mark turn `C:\dir\CLAUDE.md` into the broken
// `[CLAUDE.md](http://CLAUDE.md)` (the directory is lost, the URL is malformed).
// These detectors let the paste handler keep such content as plain text.

// Windows drive-letter absolute path, e.g. C:\Users\me\CLAUDE.md or C:/Users/me.
const WINDOWS_DRIVE_PATH = /^[a-zA-Z]:[\\/]/
// UNC path, e.g. \\server\share\file.
const WINDOWS_UNC_PATH = /^\\\\[^\\/]/
// POSIX absolute path with at least one more segment, e.g. /home/me/CLAUDE.md.
// A lone "/" or a single segment like "/foo" is excluded so we don't capture
// things that read more like relative web fragments.
const POSIX_ABSOLUTE_PATH = /^\/[^/\s][^\n]*\/[^\n]*$/

/**
 * Returns true when `text` is a single filesystem path that the rich editor
 * should paste verbatim rather than autolink. The text is trimmed first because
 * terminal selections frequently carry a trailing newline.
 */
export function isFilesystemPath(text: string): boolean {
  const trimmed = text.trim()
  if (!trimmed) {
    return false
  }
  // Why: multi-line clipboard content is prose, not a path; limiting to a
  // single line avoids hijacking ordinary paragraph pastes.
  if (/[\n\r]/.test(trimmed)) {
    return false
  }
  if (WINDOWS_DRIVE_PATH.test(trimmed) || WINDOWS_UNC_PATH.test(trimmed)) {
    return true
  }
  // POSIX paths can contain spaces, so only require the leading-slash shape.
  return POSIX_ABSOLUTE_PATH.test(trimmed)
}

/**
 * When the clipboard HTML is a single terminal-style link wrapping a filesystem
 * path, returns that path's visible text so it can be pasted as plain text.
 * Returns null for genuine links or anything more complex than one anchor.
 */
export function extractTerminalLinkFilesystemPath(html: string): string | null {
  if (!html.trim()) {
    return null
  }
  let doc: Document
  try {
    doc = new DOMParser().parseFromString(html, 'text/html')
  } catch {
    return null
  }
  const body = doc.body
  if (!body) {
    return null
  }
  const anchors = body.querySelectorAll('a[href]')
  if (anchors.length !== 1) {
    return null
  }
  const anchor = anchors[0]
  // Why: only treat the anchor as a path link when it is effectively the whole
  // payload (a lone clickable token), not one link inside a larger rich paste.
  if ((body.textContent ?? '').trim() !== (anchor.textContent ?? '').trim()) {
    return null
  }
  // Why: terminals (e.g. Claude Code) keep the full path as the anchor's
  // visible text while fabricating an `http://<filename>` href. Recognizing the
  // path in the visible text catches the broken link regardless of that href.
  const visibleText = (anchor.textContent ?? '').trim()
  if (visibleText && isFilesystemPath(visibleText)) {
    return visibleText
  }
  return null
}
