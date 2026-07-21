import type { Editor } from '@tiptap/react'
import {
  extractTerminalLinkFilesystemPath,
  isFilesystemPath
} from './rich-markdown-filesystem-path'

function readPlainText(event: ClipboardEvent): string {
  return event.clipboardData?.getData('text/plain') ?? ''
}

function readHtmlText(event: ClipboardEvent): string {
  return event.clipboardData?.getData('text/html') ?? ''
}

/**
 * Resolves the plain-text filesystem path the clipboard is carrying, if any.
 * The plain-text flavor holds the true path; the HTML flavor is only consulted
 * to recognize terminal-style `<a href="http://<filename>">` links.
 */
function resolvePastedFilesystemPath(event: ClipboardEvent): string | null {
  const plainText = readPlainText(event)
  if (plainText && isFilesystemPath(plainText)) {
    return plainText.trim()
  }
  const html = readHtmlText(event)
  if (html) {
    return extractTerminalLinkFilesystemPath(html)
  }
  return null
}

/**
 * Intercepts pastes of bare filesystem paths so TipTap's Link mark cannot
 * convert them into a broken markdown link. Inserts the path as plain text and
 * claims the paste; returns false to let normal handling proceed otherwise.
 */
export function handleRichMarkdownFilesystemPathPaste(
  editor: Editor | null,
  event: ClipboardEvent
): boolean {
  if (event.defaultPrevented || !editor) {
    return false
  }
  const path = resolvePastedFilesystemPath(event)
  if (!path) {
    return false
  }

  event.preventDefault()
  // Why: insertText writes a literal text node with no link mark, so the path
  // round-trips through markdown serialization unchanged (no autolinking).
  editor.view.dispatch(editor.state.tr.insertText(path))
  return true
}
