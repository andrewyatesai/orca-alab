/* tslint:disable */
/* eslint-disable */
/**
 * A terminal + CPU renderer pair. Feed PTY bytes with [`AtermTerminal::process`],
 * then [`AtermTerminal::render`] to refresh the RGBA framebuffer, then read it
 * back via [`AtermTerminal::rgba`] (+ `width`/`height`) to draw onto a canvas.
 */
export class AtermTerminal {
  free(): void;
  /**
   * Build a `rows`x`cols` terminal rendered with `font_bytes` (a TTF/OTF) at
   * `px` cell font-size. `font_bytes` is injected by the host (fetched in JS),
   * keeping the engine free of filesystem font discovery.
   */
  constructor(rows: number, cols: number, font_bytes: Uint8Array, px: number);
  /**
   * Copy of the last-rendered RGBA8 framebuffer (`width*height*4` bytes),
   * ready for `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)`.
   */
  rgba(): Uint8Array;
  /**
   * Rasterize the current grid into the internal RGBA8 framebuffer.
   */
  render(): void;
  /**
   * Resize the grid (after the host recomputes cols/rows for the canvas).
   */
  resize(rows: number, cols: number): void;
  /**
   * Feed raw PTY output bytes into the engine.
   */
  process(bytes: Uint8Array): void;
  /**
   * Last-rendered framebuffer width in pixels.
   */
  readonly width: number;
  /**
   * Last-rendered framebuffer height in pixels.
   */
  readonly height: number;
}
