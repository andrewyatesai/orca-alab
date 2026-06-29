// Message protocol for the off-main-thread aterm render worker (plan §9, stage 2).
//
// The worker owns an aterm engine + the pane's OffscreenCanvas and does the
// per-frame work (process → render → blit) off the renderer main thread, so heavy
// terminal output stops competing with React/layout/paint. The main thread keeps
// its own engine for the facade's SYNCHRONOUS query API (serialize/selection/
// row reads); this worker is a render mirror fed the same PTY bytes in order. It
// posts back a cacheable STATE snapshot so the few main-thread reads that the draw
// path needs (display offset for follow-bottom, grid size) stay synchronous.
//
// This file is types-only so both the worker and the main-side strategy share one
// contract without importing each other's runtime.

import type { AtermThemeColors } from './aterm-theme-colors'

/** Engine construction params (sent once on init, with the transferred canvas). */
export type AtermWorkerInit = {
  type: 'init'
  /** The pane canvas, transferred via transferControlToOffscreen(). */
  canvas: OffscreenCanvas
  /** JetBrains-Mono bytes (the main thread already fetched them; passed as a
   *  transferable so the worker doesn't re-fetch). */
  fontBytes: Uint8Array
  /** Optional CJK/emoji fallback faces (same bytes the main path injects). */
  fallbackFonts: Uint8Array[]
  rows: number
  cols: number
  /** Device-pixel cell font size (already dpr-scaled by the caller). */
  fontPx: number
  /** Full theme: drives the constructor colours + 16-ANSI palette + reply defaults. */
  themeColors: AtermThemeColors
}

/** Feed PTY/replay output (string; the worker uses process_str → encodeInto). */
export type AtermWorkerProcess = { type: 'process'; data: string }
/** Re-rasterize the current grid into the OffscreenCanvas (coalesced by the host
 *  to one per frame). */
export type AtermWorkerDraw = { type: 'draw' }
export type AtermWorkerResize = { type: 'resize'; rows: number; cols: number }
/** Re-derive cell metrics at a new device-pixel font size (dpr / font change). */
export type AtermWorkerSetPx = { type: 'setPx'; px: number }
export type AtermWorkerScrollLines = { type: 'scrollLines'; delta: number }
export type AtermWorkerScrollToBottom = { type: 'scrollToBottom' }
export type AtermWorkerDispose = { type: 'dispose' }

export type AtermWorkerRequest =
  | AtermWorkerInit
  | AtermWorkerProcess
  | AtermWorkerDraw
  | AtermWorkerResize
  | AtermWorkerSetPx
  | AtermWorkerScrollLines
  | AtermWorkerScrollToBottom
  | AtermWorkerDispose

/** Cacheable engine state the worker pushes after each process/draw, so the main
 *  thread's draw path can read it synchronously without a round-trip. Mirrors the
 *  subset of AtermPaneController the per-frame/follow-bottom logic needs. */
export type AtermWorkerState = {
  type: 'state'
  /** Framebuffer device-pixel size after the last render. */
  width: number
  height: number
  cols: number
  rows: number
  cellWidth: number
  cellHeight: number
  /** Lines scrolled up from the live bottom (0 = at bottom). */
  displayOffset: number
  cursorX: number
  cursorY: number
  baseY: number
  isAltScreen: boolean
  /** OSC 0/2 title, or null. */
  title: string | null
}

/** First state after init carries the initial cell metrics so the host can build
 *  the grid; reuses AtermWorkerState. */
export type AtermWorkerResponse = AtermWorkerState
