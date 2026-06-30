/* tslint:disable */
/* eslint-disable */

/**
 * The terminal engine + GPU presentation state for one `<canvas>`.
 *
 * Construction is split in two, matching the browser lifecycle:
 *   1. [`AtermGpuTerminal::new`] — synchronous: build the engine grid + a CPU
 *      face from injected font bytes (for cell metrics / the glyph atlas). No
 *      GPU touched yet, so it can run before WebGL is confirmed.
 *   2. [`AtermGpuTerminal::init`] — async: acquire the GPU and create +
 *      configure the canvas surface. Separated so the host can fall back to the
 *      CPU path (`the aterm-wasm crate`) if WebGL is unavailable WITHOUT having
 *      paid for the engine teardown.
 */
export class AtermGpuTerminal {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * APPEND another fallback face to the chain (does NOT reset it like
     * [`set_fallback_font`]). Applies to the CPU face and the live GPU face if
     * `init` already ran; the bytes are also remembered so `init` re-applies the
     * whole chain to the fresh GPU face. Lets the host push a CJK fallback then
     * Arabic/Devanagari/Thai/Hebrew faces so a glyph the earlier faces miss still
     * reaches a covering face. No-throw: a bad blob leaves the chain untouched.
     */
    add_fallback_font(bytes: Uint8Array): void;
    /**
     * Authorize OSC 52 clipboard *write* so the engine queues OSC 52 app-events
     * for the host to drain (see aterm-wasm). Without it the engine is fail-closed
     * (CF-004) and drops PTY-origin OSC 52 set sequences. The grid is shared, so
     * this covers both the GPU and CPU-fallback paths.
     */
    authorize_clipboard_write(): void;
    /**
     * Whether the cell at `row`/`col` is a wide (double-width) character;
     * `None` when out of range.
     */
    cell_is_wide(row: number, col: number): boolean | undefined;
    /**
     * Grapheme text at visible cell `row`/`col` — base char plus complex
     * cluster and combining marks. Empty string for a blank cell, a
     * wide-continuation spacer, or out-of-range coords.
     */
    cell_text(row: number, col: number): string;
    /**
     * Drain the edge-triggered BEL flag: `true` if a BEL fired since the last
     * call, then clears it (poll-based flash/ring without the bell callback).
     */
    drain_bell(): boolean;
    /**
     * Encode mouse MOTION at `col`/`row`; `button` is the held button (3=none).
     */
    encode_mouse_motion(col: number, row: number, button: number, mods: number): Uint8Array | undefined;
    /**
     * Encode a mouse-button PRESS at 0-based cell `col`/`row` for the active
     * mouse mode+encoding (`None` when tracking is off). See aterm-wasm.
     */
    encode_mouse_press(col: number, row: number, button: number, mods: number): Uint8Array | undefined;
    /**
     * Encode a mouse-button RELEASE; `None` in X10 press-only mode.
     */
    encode_mouse_release(col: number, row: number, button: number, mods: number): Uint8Array | undefined;
    /**
     * Encode a mouse WHEEL tick at `col`/`row` (`up` = wheel-up); `None` in X10.
     */
    encode_mouse_wheel(col: number, row: number, up: boolean, mods: number): Uint8Array | undefined;
    /**
     * ASYNC: acquire the GPU and create + configure a WebGL2 surface on `canvas`.
     *
     * This is the browser equivalent of aterm-gpu's native `GpuRenderer::new` +
     * `create_window_surface`, but every blocking step is `await`ed AND the
     * surface is created BEFORE the adapter (the WebGL backend enumerates its
     * adapter against the canvas surface — the GL context lives on the canvas):
     *   - `wgpu::Instance` with the WebGL (GL) backend,
     *   - `instance.create_surface(SurfaceTarget::Canvas(canvas))`,
     *   - `GpuContext::from_instance_with_surface(instance, Some(&surface)).await`
     *     — adapter + device, NO `pollster::block_on`,
     *   - `GpuRenderer::from_parts(ctx, cpu_face, ..)` — the portable, thread-
     *     free, font-discovery-free renderer assembly (all wgpu pipelines built),
     *   - `configure_window_surface(surface, w, h)` — same format selection as
     *     native's `create_window_surface`.
     *
     * Returns `Err` (a JS string) if WebGL is unavailable or any step fails, so
     * the host can fall back to the CPU `aterm-wasm` path.
     */
    init(canvas: HTMLCanvasElement): Promise<void>;
    /**
     * Worker variant: acquire the GPU + create the WebGL2 surface on a TRANSFERRED
     * `OffscreenCanvas`, so the entire GPU render+present runs off the renderer main
     * thread (the universal off-main win — wgpu maps `SurfaceTarget::OffscreenCanvas`
     * to the OffscreenCanvas WebGL2 context inside the worker). Same shared init as
     * the on-canvas path; only the surface target differs.
     */
    init_offscreen(canvas: OffscreenCanvas): Promise<void>;
    /**
     * Detect a link under display `row`/`col`. Prefers an OSC-8 hyperlink, then
     * falls back to smart-selection rules (url/file_path). `None` for plain
     * words. `kind`: 0=osc8, 1=url, 2=file_path, 3=other. See aterm-wasm.
     */
    link_at(row: number, col: number): LinkHit | undefined;
    /**
     * Build a `rows`x`cols` terminal. `font_bytes` (a TTF/OTF) is injected by the
     * host (fetched in JS) — the engine does no filesystem font discovery on
     * wasm. `px` is the cell font-size; `fg`/`bg`/`cursor`/`selection` are
     * 0x00RRGGBB and seed the DEFAULT theme (per-cell SGR colors still flow
     * through the grid independently).
     */
    constructor(rows: number, cols: number, font_bytes: Uint8Array, px: number, fg: number, bg: number, cursor: number, selection: number);
    /**
     * Feed raw PTY output bytes into the engine.
     */
    process(bytes: Uint8Array): void;
    /**
     * Present one frame on the GPU canvas. Errors (returned as JS strings) if
     * WebGL was not initialized.
     *
     * Draws the ACTUAL terminal grid: snapshot the engine state
     * (`term.cell_frame`), then aterm-gpu's `present_input` renders it offscreen
     * (glyph atlas upload + instanced bg/glyph/cursor quads) and blits that
     * texture into the WebGL2 canvas swapchain — the same encode the native
     * CPU==GPU parity tests gate, now on the WebGL backend.
     */
    render(): void;
    /**
     * SECONDARY (e2e) path: render the current grid OFFSCREEN and read the pixels
     * back into the internal RGBA8 framebuffer, so a host harness can pixel-compare
     * GPU vs CPU output without reading the live canvas (a WebGL swapchain is not
     * CPU-readable). Mirrors `the aterm-wasm crate`'s `render()`+`rgba()` contract:
     * the same `cell_frame` snapshot, the same `Frame` (0x00RRGGBB) expanded to
     * RGBA8 with an opaque alpha. Errors if WebGL was not initialized.
     */
    render_offscreen(): void;
    /**
     * Resize the grid AND, if the GPU is live, the swapchain to match the new
     * pixel extent (host recomputes cols/rows for the canvas first).
     */
    resize(rows: number, cols: number): void;
    /**
     * Revoke OSC 52 clipboard *write* authorization (user toggled the setting
     * off), returning the engine to its fail-closed default.
     */
    revoke_clipboard_write(): void;
    /**
     * Copy of the last [`render_offscreen`](Self::render_offscreen) RGBA8
     * framebuffer (`width*height*4` bytes), ready for
     * `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)` or a pixel diff.
     */
    rgba(): Uint8Array;
    /**
     * Soft-wrap flag for a visible `row`: `true` if it continues the previous
     * row (autowrap), `None` when out of range.
     */
    row_is_wrapped(row: number): boolean | undefined;
    /**
     * Logical length of a visible `row` (last non-empty cell + 1, 0 if blank);
     * `None` when out of range.
     */
    row_len(row: number): number | undefined;
    /**
     * Scroll-correct text of a display `row` (display_offset-aware), for a TS
     * fallback that re-runs link matching in JS.
     */
    row_text(row: number): string | undefined;
    /**
     * Scroll the viewport through scrollback: positive `delta` reveals older
     * lines, negative reveals newer. The host redraws afterwards.
     */
    scroll_lines(delta: number): void;
    /**
     * Scroll the viewport so the match at absolute `line` is visible (top row),
     * clamped to the retained scrollback. Host redraws after.
     */
    scroll_search_line_into_view(line: number): void;
    /**
     * Snap the viewport to the live bottom (latest output).
     */
    scroll_to_bottom(): void;
    /**
     * Snap the viewport to the oldest retained scrollback line.
     */
    scroll_to_top(): void;
    /**
     * Search the full retained buffer for `query`, returning matches as a flat
     * `[abs_line, start_col, len]` triplet array. Empty query / regex error →
     * empty array. `is_regex` compiles `query` as a regex (parity with aterm-wasm;
     * the core already accepts it — the web GPU path previously hardcoded false).
     * See aterm-wasm for the coordinate contract.
     */
    search(query: string, case_sensitive: boolean, is_regex: boolean): Uint32Array;
    /**
     * Drop the current selection so the highlight clears on the next render.
     */
    selection_clear(): void;
    /**
     * Move the selection endpoint to `row`/`col` (during a drag).
     */
    selection_extend(row: number, col: number): void;
    /**
     * Finalize the selection (mouse released).
     */
    selection_finish(): void;
    /**
     * Select the whole line at display `row` (triple-click) and return its text.
     * Mirrors aterm-gui's select_line: a Lines selection expanded to the full row
     * width. `col` is accepted for a uniform host API but unused (whole row).
     */
    selection_line(row: number, col: number): string | undefined;
    /**
     * Current selection bounds in DISPLAY viewport cell coords (0 = top visible
     * row), side-adjusted to match `selection_text` and the painted highlight.
     * `None` when there is no selection OR it lies fully outside the viewport.
     */
    selection_range(): SelectionRange | undefined;
    /**
     * Begin a character selection at display `row`/`col` (clears any prior one).
     */
    selection_start(row: number, col: number): void;
    /**
     * The selected text, if any (`None` when the selection is empty).
     */
    selection_text(): string | undefined;
    /**
     * Select the whole word/URL at display `row`/`col` (double-click) and return
     * its text. Mirrors aterm-gui's select_word: a Semantic selection EXPANDED to
     * the word's inclusive cell span (smart_word_at's end col is exclusive); on
     * whitespace it falls back to the clicked cell. The selection stays active so
     * the highlight paints.
     */
    selection_word(row: number, col: number): string | undefined;
    /**
     * Serialize the terminal to a REPLAYABLE ANSI string (mirrors the CPU
     * `AtermTerminal::serialize`) — the aterm-native replacement for xterm's
     * SerializeAddon. `scrollback_rows`: None = all history, Some(n) = last n,
     * Some(0) = viewport only. Operates on the shared engine grid.
     */
    serialize(scrollback_rows?: number | null): string;
    /**
     * Scrollback HISTORY only (main buffer) — mirrors the CPU
     * `AtermTerminal::serialize_scrollback`.
     */
    serialize_scrollback(max_rows?: number | null): string;
    /**
     * Inject a REAL bold weight of the primary family so SGR-bold cells render as a
     * true heavier weight instead of synthetic embolden. Applies to the CPU face
     * and the live GPU face if `init` already ran; remembered so `init` re-applies
     * it to the fresh GPU face. No-throw: a bad blob leaves the existing weight.
     */
    set_bold_font(bytes: Uint8Array): void;
    /**
     * Tell the engine the real device-pixel cell size so CSI 14t/16t reports are
     * accurate (the engine has no canvas otherwise).
     */
    set_cell_pixel_size(width: number, height: number): void;
    /**
     * Push the host OS color scheme into the engine. `dark = true` selects a dark
     * appearance, `false` light. When the scheme CHANGES and the app enabled DEC mode
     * 2031, the engine queues an unsolicited `CSI ? 997 ; Ps n`; drain it via
     * `take_response` and forward to the PTY. A no-op when unchanged. Mirrors aterm-wasm.
     */
    set_color_scheme(dark: boolean): void;
    /**
     * Set the cursor blink phase (see aterm-wasm). Applies to the live GPU renderer
     * AND the CPU face so the GPU present + offscreen readback paths agree.
     */
    set_cursor_blink_phase(on: boolean): void;
    /**
     * Force a hollow (unfocused) cursor when `true`, or restore the terminal's
     * DECSCUSR style when `false`. Applies to both GPU and CPU faces.
     */
    set_cursor_hollow(hollow: boolean): void;
    set_default_background(r: number, g: number, b: number): void;
    /**
     * Set the host-preferred DEFAULT cursor style (shape used before any DECSCUSR and
     * restored after RIS/DECSTR). `n` per DECSCUSR: 1=blinking block, 2=steady block,
     * 3=blinking underline, 4=steady underline, 5=blinking bar, 6=steady bar;
     * out-of-range ignored. Does NOT clobber an app's live DECSCUSR. Mirrors aterm-wasm.
     */
    set_default_cursor_style(n: number): void;
    /**
     * Seed the engine's DEFAULT foreground/background so OSC 10/11 colour-query
     * replies report the host theme. RGB components, 0–255.
     */
    set_default_foreground(r: number, g: number, b: number): void;
    /**
     * Inject a colour-emoji (sbix) face from font bytes, driving the existing
     * ColorEmoji colour path. Same wiring as [`set_fallback_font`]. No-throw
     * (the `String` Err surfaces as a catchable JS exception).
     */
    set_emoji_font(bytes: Uint8Array): void;
    /**
     * Inject a broad-coverage (CJK + symbols) fallback face from font bytes, so
     * glyphs the primary face lacks render real shapes instead of `.notdef` tofu.
     * Applies to the CPU face (metrics) and the live GPU face if `init` already
     * ran; the bytes are also remembered so `init` re-applies them to the fresh
     * GPU face it builds. No-throw: a bad blob leaves the existing faces untouched.
     */
    set_fallback_font(bytes: Uint8Array): void;
    /**
     * OpenType FONT FEATURES for the primary face, as a space-separated spec
     * (`"+ss01 zero -calt"`). Mirrors the native `font_features` config knob. An
     * empty/blank spec clears all features. Applies to the CPU face and the live GPU
     * face; remembered so `init` re-applies it. Preserves the current ligature mode.
     */
    set_font_features(spec: string): void;
    /**
     * Programming LIGATURES on/off (`=>`, `!=`, `===` …). Mirrors the native
     * `ligatures` config knob. Applies to the CPU face and the live GPU face if `init`
     * ran; the choice is remembered so `init` re-applies it to the fresh GPU face.
     * Preserves any configured `font_features`.
     */
    set_ligatures(on: boolean): void;
    /**
     * Scale the cell BOX height (the host's `terminalLineHeight`) WITHOUT changing
     * the glyph px, on the CPU face and the live GPU face. Remembered so `init`
     * re-applies it. The host re-reads cell_height + resizes the grid after.
     */
    set_line_height(scale: number): void;
    /**
     * Set an ANSI/indexed palette colour (index 0–255; 0–15 are the 16 ANSI
     * colours) to RGB components, so SGR-indexed cell colours resolve through the
     * host's theme palette instead of the engine's built-in VGA defaults. The
     * palette lives on the shared grid (`self.term`), so this applies to both the
     * GPU and CPU-fallback draw paths. Per-cell truecolor SGR flows independently.
     */
    set_palette_color(index: number, r: number, g: number, b: number): void;
    /**
     * Swap the PRIMARY face (the host's `terminalFontFamily`) from font bytes and
     * re-rasterize, on the CPU face and the live GPU face. The injected bytes
     * REPLACE `font_bytes` so a later `init` builds the GPU face from the new
     * family directly. The host re-reads cell metrics + resizes the grid after.
     * No-throw: a bad blob leaves the existing face untouched.
     */
    set_primary_font(bytes: Uint8Array): void;
    /**
     * Re-rasterize at a new cell font px (host DPI / devicePixelRatio change) on
     * both the CPU fallback face and the live GPU renderer (which also drops its
     * atlas). The host re-reads cell_width/cell_height + resizes the grid after.
     */
    set_px(px: number): void;
    /**
     * Set the engine's scrollback line limit (history lines retained behind the live
     * viewport). `lines == 0` means unlimited (bounded only by the memory budget).
     * Shrinking truncates the oldest lines immediately; growing keeps history and lets
     * it grow. Applies to both the main and alternate screens and re-clamps the scroll
     * position. Without this the engine keeps its 100k-line default on every pane.
     */
    set_scrollback_limit(lines: number): void;
    /**
     * Explicit selected-text foreground (theme `selectionForeground`), 0x00RRGGBB,
     * or `undefined` for the WCAG contrast-floor default. Set on both the CPU
     * fallback face and the live GPU renderer; forces a full present (appearance).
     */
    set_selection_fg(fg?: number | null): void;
    /**
     * Mark the pane unfocused (`true`) / focused (`false`): when unfocused, the
     * selection band paints with the dimmer inactive bg (xterm
     * `selectionInactiveBackground`). Set on both the CPU fallback face and the
     * live GPU renderer; forces a full present (appearance-only, not content).
     */
    set_selection_inactive(inactive: boolean): void;
    /**
     * Set the inactive (unfocused) selection bg (0x00RRGGBB), or `undefined` to
     * derive it from the active selection bg blended toward the theme bg. Set on
     * both the CPU fallback face and the live GPU renderer; forces a full present.
     */
    set_selection_inactive_bg(bg?: number | null): void;
    /**
     * Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB) on both the
     * GPU renderer and the CPU face, so a host theme change re-themes the pane
     * without a device/face rebuild.
     */
    set_theme(fg: number, bg: number, cursor: number, selection: number): void;
    /**
     * Drain pending OSC app-events as a JSON array of `[code, payload]` pairs
     * (`[[7,"/home"],[52,"copied"]]`); `None` when empty. REAL decoded payloads
     * (OSC 52 clipboard / OSC 7 cwd / OSC 133 mark) — distinct from PTY replies.
     */
    take_osc_events(): string | undefined;
    /**
     * Drain the engine's pending query replies (DA1/DA2/DSR/CPR/DECRQM/OSC color/
     * window-size, …) so the host can forward them to the PTY — the renderer is the
     * authoritative responder. Call after each `process`.
     */
    take_response(): Uint8Array | undefined;
    /**
     * The window title (OSC 0/2), or `None` when unset (mirrors the CPU binding).
     */
    title(): string | undefined;
    /**
     * The acquired GPU adapter name + backend, once initialized (else empty).
     * Lets the host log which GPU/backend WebGL handed us.
     */
    readonly adapter_info: string;
    /**
     * Absolute row index of the live/last line (xterm `buffer.active.baseY`):
     * `oldest_absolute_row() + scrollback_lines()`. `usize` → plain JS number.
     */
    readonly base_y: number;
    /**
     * Whether bracketed-paste mode (DECSET 2004) is active (mirrors the CPU binding).
     */
    readonly bracketed_paste_mode: boolean;
    /**
     * Cell height in device pixels — the host computes rows = floor(canvasH / cellHeight).
     */
    readonly cell_height: number;
    /**
     * Cell width in device pixels — the host computes cols = floor(canvasW / cellWidth).
     */
    readonly cell_width: number;
    /**
     * Active DECSCUSR cursor style as the discriminant of `aterm_core`'s
     * `CursorStyle`. The GPU renderer paints the shape from the grid; this
     * getter exists for host introspection/tests, mirroring aterm-wasm.
     */
    readonly cursor_style: number;
    /**
     * Display-relative cursor column (0-based).
     */
    readonly cursor_x: number;
    /**
     * Display-relative cursor row (0-based, top of viewport).
     */
    readonly cursor_y: number;
    /**
     * Lines the viewport is scrolled up from the live bottom (0 = at bottom).
     */
    readonly display_offset: number;
    /**
     * Absolute row index of the TOP visible line for the current viewport
     * (`base_y - display_offset`); the search/link origin.
     */
    readonly display_origin_absolute: number;
    /**
     * True once [`AtermGpuTerminal::init`] has acquired a GPU + surface.
     */
    readonly gpu_ready: boolean;
    /**
     * Height in pixels of the last [`render_offscreen`](Self::render_offscreen)
     * framebuffer.
     */
    readonly height: number;
    /**
     * True when the alternate screen is active (TUIs own their own scrolling),
     * so the host should let wheel events pass through to the app.
     */
    readonly is_alt_screen: boolean;
    /**
     * True when DECCKM (application cursor keys) is set: the host encodes
     * arrows/Home/End as SS3 instead of CSI for full-screen apps.
     */
    readonly is_app_cursor_mode: boolean;
    /**
     * True when DEC mode 2031 (color-scheme update notifications) is set: the
     * app wants `CSI ? 997 ; n` on OS light/dark theme changes.
     */
    readonly is_color_scheme_updates_mode: boolean;
    /**
     * True when DECSET 1004 (focus reporting) is active: the host sends CSI I
     * on focus-in and CSI O on focus-out so apps track terminal focus.
     */
    readonly is_focus_event_mode: boolean;
    /**
     * True when a TUI has enabled mouse tracking (DECSET 9/1000/1002/1003).
     * The host then ENCODES canvas mouse events to the PTY instead of running
     * selection/scroll/link for them (unless Shift = user override).
     */
    readonly is_mouse_tracking: boolean;
    /**
     * True for AnyEvent (1003): report motion even with NO button pressed.
     */
    readonly mouse_wants_any_motion: boolean;
    /**
     * True when the active mouse mode reports MOTION (1002 drag, 1003 any).
     */
    readonly mouse_wants_motion: boolean;
    /**
     * Absolute row of display row 0 at the live bottom. A match at absolute
     * `line` is at display row `line - origin + display_offset`.
     */
    readonly search_display_origin: number;
    /**
     * Width in pixels of the last [`render_offscreen`](Self::render_offscreen)
     * framebuffer.
     */
    readonly width: number;
}

/**
 * A detected link under a cell: its text/URL, the half-open display-column span
 * it covers, and a `kind` discriminant (0=osc8, 1=url, 2=file_path, 3=other).
 * Mirrors `the aterm-wasm crate`'s `LinkHit` so the host link input is unchanged.
 */
export class LinkHit {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Exclusive end display column of the link span.
     */
    readonly end_col: number;
    /**
     * Link kind: 0=osc8, 1=url, 2=file_path, 3=other.
     */
    readonly kind: number;
    /**
     * Inclusive start display column of the link span.
     */
    readonly start_col: number;
    /**
     * The link's URL/target text.
     */
    readonly url: string;
}

/**
 * Selection bounds in DISPLAY viewport cell coords (0 = top visible row),
 * inclusive of `start`, with `end` already side-adjusted to match
 * `selection_text` and the painted highlight. Mirrors the aterm-wasm crate.
 */
export class SelectionRange {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * End column (display-relative, side-adjusted/inclusive).
     */
    readonly end_x: number;
    /**
     * End row (display-relative).
     */
    readonly end_y: number;
    /**
     * Start column (display-relative).
     */
    readonly start_x: number;
    /**
     * Start row (display-relative, 0 = top visible row).
     */
    readonly start_y: number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_atermgputerminal_free: (a: number, b: number) => void;
    readonly __wbg_linkhit_free: (a: number, b: number) => void;
    readonly __wbg_selectionrange_free: (a: number, b: number) => void;
    readonly atermgputerminal_adapter_info: (a: number) => [number, number];
    readonly atermgputerminal_add_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_authorize_clipboard_write: (a: number) => void;
    readonly atermgputerminal_base_y: (a: number) => number;
    readonly atermgputerminal_bracketed_paste_mode: (a: number) => number;
    readonly atermgputerminal_cell_height: (a: number) => number;
    readonly atermgputerminal_cell_is_wide: (a: number, b: number, c: number) => number;
    readonly atermgputerminal_cell_text: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_cell_width: (a: number) => number;
    readonly atermgputerminal_cursor_style: (a: number) => number;
    readonly atermgputerminal_cursor_x: (a: number) => number;
    readonly atermgputerminal_cursor_y: (a: number) => number;
    readonly atermgputerminal_display_offset: (a: number) => number;
    readonly atermgputerminal_display_origin_absolute: (a: number) => number;
    readonly atermgputerminal_drain_bell: (a: number) => number;
    readonly atermgputerminal_encode_mouse_motion: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_encode_mouse_press: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_encode_mouse_release: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_encode_mouse_wheel: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_gpu_ready: (a: number) => number;
    readonly atermgputerminal_height: (a: number) => number;
    readonly atermgputerminal_init: (a: number, b: any) => any;
    readonly atermgputerminal_init_offscreen: (a: number, b: any) => any;
    readonly atermgputerminal_is_alt_screen: (a: number) => number;
    readonly atermgputerminal_is_app_cursor_mode: (a: number) => number;
    readonly atermgputerminal_is_color_scheme_updates_mode: (a: number) => number;
    readonly atermgputerminal_is_focus_event_mode: (a: number) => number;
    readonly atermgputerminal_is_mouse_tracking: (a: number) => number;
    readonly atermgputerminal_link_at: (a: number, b: number, c: number) => number;
    readonly atermgputerminal_mouse_wants_any_motion: (a: number) => number;
    readonly atermgputerminal_mouse_wants_motion: (a: number) => number;
    readonly atermgputerminal_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number];
    readonly atermgputerminal_process: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_render: (a: number) => [number, number];
    readonly atermgputerminal_render_offscreen: (a: number) => [number, number];
    readonly atermgputerminal_resize: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_revoke_clipboard_write: (a: number) => void;
    readonly atermgputerminal_rgba: (a: number) => [number, number];
    readonly atermgputerminal_row_is_wrapped: (a: number, b: number) => number;
    readonly atermgputerminal_row_len: (a: number, b: number) => number;
    readonly atermgputerminal_row_text: (a: number, b: number) => [number, number];
    readonly atermgputerminal_scroll_lines: (a: number, b: number) => void;
    readonly atermgputerminal_scroll_search_line_into_view: (a: number, b: number) => void;
    readonly atermgputerminal_scroll_to_bottom: (a: number) => void;
    readonly atermgputerminal_scroll_to_top: (a: number) => void;
    readonly atermgputerminal_search: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_search_display_origin: (a: number) => number;
    readonly atermgputerminal_selection_clear: (a: number) => void;
    readonly atermgputerminal_selection_extend: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_selection_finish: (a: number) => void;
    readonly atermgputerminal_selection_line: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_selection_range: (a: number) => number;
    readonly atermgputerminal_selection_start: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_selection_text: (a: number) => [number, number];
    readonly atermgputerminal_selection_word: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_serialize: (a: number, b: number) => [number, number];
    readonly atermgputerminal_serialize_scrollback: (a: number, b: number) => [number, number];
    readonly atermgputerminal_set_bold_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_cell_pixel_size: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_color_scheme: (a: number, b: number) => void;
    readonly atermgputerminal_set_cursor_blink_phase: (a: number, b: number) => void;
    readonly atermgputerminal_set_cursor_hollow: (a: number, b: number) => void;
    readonly atermgputerminal_set_default_background: (a: number, b: number, c: number, d: number) => void;
    readonly atermgputerminal_set_default_cursor_style: (a: number, b: number) => void;
    readonly atermgputerminal_set_default_foreground: (a: number, b: number, c: number, d: number) => void;
    readonly atermgputerminal_set_emoji_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_font_features: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_ligatures: (a: number, b: number) => void;
    readonly atermgputerminal_set_line_height: (a: number, b: number) => void;
    readonly atermgputerminal_set_palette_color: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_set_primary_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_px: (a: number, b: number) => void;
    readonly atermgputerminal_set_scrollback_limit: (a: number, b: number) => void;
    readonly atermgputerminal_set_selection_fg: (a: number, b: number) => void;
    readonly atermgputerminal_set_selection_inactive: (a: number, b: number) => void;
    readonly atermgputerminal_set_selection_inactive_bg: (a: number, b: number) => void;
    readonly atermgputerminal_set_theme: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_take_osc_events: (a: number) => [number, number];
    readonly atermgputerminal_take_response: (a: number) => [number, number];
    readonly atermgputerminal_title: (a: number) => [number, number];
    readonly atermgputerminal_width: (a: number) => number;
    readonly linkhit_end_col: (a: number) => number;
    readonly linkhit_kind: (a: number) => number;
    readonly linkhit_start_col: (a: number) => number;
    readonly linkhit_url: (a: number) => [number, number];
    readonly selectionrange_end_x: (a: number) => number;
    readonly selectionrange_end_y: (a: number) => number;
    readonly selectionrange_start_x: (a: number) => number;
    readonly selectionrange_start_y: (a: number) => number;
    readonly wasm_bindgen__closure__destroy__h5d7a8bed3c20d8b8: (a: number, b: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h73172e845d7760e8: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h86ce16dd5e9bccc0: (a: number, b: number, c: any) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
