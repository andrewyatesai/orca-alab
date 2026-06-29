/** Focus the helper textarea on canvas click. The canvas is NOT focusable, so the
 *  browser's mousedown DEFAULT moves focus to <body>, blurring the textarea we focus
 *  here — which left the terminal unable to receive keys (cursor drawn hollow) on a
 *  real click, even though a synthetic pointerdown focused fine. preventDefault the
 *  default focus change, then explicitly focus the textarea so keystrokes + the
 *  focused-cursor state land. The selection handler reads the same event's coords
 *  independently, so drag-select is unaffected. Returns a disposer the wiring calls on
 *  teardown. */
export function attachAtermCanvasFocus(
  canvas: HTMLCanvasElement,
  textarea: HTMLTextAreaElement
): { dispose: () => void } {
  const onCanvasMouseDown = (event: MouseEvent): void => {
    event.preventDefault()
    textarea.focus()
  }
  canvas.addEventListener('mousedown', onCanvasMouseDown)
  return { dispose: () => canvas.removeEventListener('mousedown', onCanvasMouseDown) }
}
