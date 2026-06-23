/** Copy selected text via Electron's clipboard IPC (the same seam the rest of the
 *  app uses). Also surfaces the text on a window field so e2e can assert copies
 *  under a hidden window where navigator.clipboard is unavailable. */
export function copyAtermSelectionToClipboard(text: string): void {
  ;(window as unknown as { __atermLastCopied?: string }).__atermLastCopied = text
  void window.api?.ui?.writeClipboardText?.(text)?.catch(() => {
    /* ignore clipboard write failures */
  })
}
