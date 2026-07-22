import { copyTerminalTextVerified } from '@/components/terminal-pane/terminal-copy-outcome'

/** Copy selected text via Electron's clipboard IPC (the same seam the rest of the
 *  app uses), now verified: a failed write surfaces through the copy-outcome
 *  seam (rate-limited per session for this drag-happy path). Also surfaces the
 *  text on a window field so e2e can assert copies under a hidden window where
 *  navigator.clipboard is unavailable. */
export function copyAtermSelectionToClipboard(text: string): void {
  ;(window as unknown as { __atermLastCopied?: string }).__atermLastCopied = text
  void copyTerminalTextVerified(text, 'copy-on-select')
}
