/** Convert an engine 0x00RRGGBB color seed (the aterm theme-color format) to a
 *  CSS hex string, so DOM painted behind the canvas matches the engine's pixels
 *  exactly — the never-blank first paint and the engine's first frame must be
 *  the same color or the handoff shows a seam. */
export function engineColorToCss(rgb: number): string {
  return `#${(rgb & 0xffffff).toString(16).padStart(6, '0')}`
}
