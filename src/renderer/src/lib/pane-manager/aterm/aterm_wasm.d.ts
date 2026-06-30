/* tslint:disable */
/* eslint-disable */

/**
 * A terminal + CPU renderer pair. Feed PTY bytes with [`AtermTerminal::process`],
 * then [`AtermTerminal::render`] to refresh the RGBA framebuffer, then read it
 * back via [`AtermTerminal::rgba`] (+ `width`/`height`) to draw onto a canvas.
 */
export class AtermTerminal {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * APPEND another fallback face to the chain (does NOT reset it like
     * [`set_fallback_font`]). The chain is tried in order, so the host can push a
     * CJK fallback first then Arabic/Devanagari/Thai/Hebrew faces after it — a
     * glyph the earlier faces miss still reaches a covering face instead of tofu.
     * No-throw: a bad blob leaves the existing chain untouched.
     */
    add_fallback_font(bytes: Uint8Array): void;
    /**
     * Authorize OSC 52 clipboard *write* (set) so the engine queues OSC 52
     * app-events for the host to drain via `take_osc_events`. Without this the
     * engine is fail-closed (CF-004) and silently drops PTY-origin OSC 52 set
     * sequences, so they never reach the host. The host still gates the actual
     * clipboard write on its own user setting (defense in depth).
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
     * call, then clears it (so a poll-based host can flash/ring without the
     * synchronous bell callback).
     */
    drain_bell(): boolean;
    /**
     * Encode mouse MOTION at `col`/`row`; `button` is the held button (3 = none).
     * `None` unless the mode reports motion (1002 while a button is down, 1003
     * always) — see [`AtermTerminal::mouse_wants_motion`].
     */
    encode_mouse_motion(col: number, row: number, button: number, mods: number): Uint8Array | undefined;
    /**
     * Encode a mouse-button PRESS at 0-based on-screen cell `col`/`row` for the
     * app's active mouse mode+encoding (returns `None`/`undefined` when tracking
     * is off). `button` is the raw X10 button code (0=left,1=middle,2=right) and
     * `mods` is the OR of Shift(4)/Alt(8)/Ctrl(16) masks — the engine combines
     * them. Bytes are sent verbatim to the PTY.
     */
    encode_mouse_press(col: number, row: number, button: number, mods: number): Uint8Array | undefined;
    /**
     * Encode a mouse-button RELEASE (see [`AtermTerminal::encode_mouse_press`]);
     * `None` in X10 press-only mode.
     */
    encode_mouse_release(col: number, row: number, button: number, mods: number): Uint8Array | undefined;
    /**
     * Encode a mouse WHEEL tick at `col`/`row` (`up` = wheel-up); the host sends
     * these instead of scrolling scrollback while tracking is on. `None` in X10.
     */
    encode_mouse_wheel(col: number, row: number, up: boolean, mods: number): Uint8Array | undefined;
    /**
     * Detect a link under display `row`/`col`. Prefers an OSC-8 hyperlink, then
     * falls back to smart-selection rules (url/file_path). Returns `None` for
     * plain words. `kind`: 0=osc8, 1=url, 2=file_path, 3=other.
     */
    link_at(row: number, col: number): LinkHit | undefined;
    /**
     * Build a `rows`x`cols` terminal rendered with `font_bytes` (a TTF/OTF) at
     * `px` cell font-size. `font_bytes` is injected by the host (fetched in JS),
     * keeping the engine free of filesystem font discovery. `fg`/`bg`/`cursor`/
     * `selection` are 0x00RRGGBB and seed the renderer's DEFAULT theme colors;
     * per-cell SGR colors still flow through the grid independently.
     */
    constructor(rows: number, cols: number, font_bytes: Uint8Array, px: number, fg: number, bg: number, cursor: number, selection: number);
    /**
     * Feed raw PTY output bytes into the engine.
     */
    process(bytes: Uint8Array): void;
    /**
     * Feed PTY output as a JS string. wasm-bindgen encodes it (UTF-8, via
     * `encodeInto`) straight into wasm memory, so the host avoids a separate
     * JS-side `TextEncoder.encode` allocation + copy on the hot output path.
     * Byte-identical to `process(new TextEncoder().encode(s))`.
     */
    process_str(s: string): void;
    /**
     * Rasterize the current grid into the internal RGBA8 framebuffer via the
     * damage-tracked path: only rows that changed since the last frame are
     * re-rendered (the rest reuse the persistent cache), so streaming output and
     * single-keystroke edits don't re-rasterize the whole grid every frame.
     */
    render(): void;
    /**
     * Resize the grid (after the host recomputes cols/rows for the canvas).
     */
    resize(rows: number, cols: number): void;
    /**
     * Revoke OSC 52 clipboard *write* authorization (the user toggled the
     * clipboard setting off). Returns the engine to its fail-closed default.
     */
    revoke_clipboard_write(): void;
    /**
     * Copy of the last-rendered RGBA8 framebuffer (`width*height*4` bytes),
     * ready for `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)`.
     */
    rgba(): Uint8Array;
    /**
     * Byte offset of the last-rendered RGBA8 framebuffer within wasm linear
     * memory, for a ZERO-COPY `putImageData` from JS (no copy out of wasm, unlike
     * [`rgba`]). The host builds `new Uint8ClampedArray(memory.buffer, ptr,
     * width*height*4)` and must read it synchronously right after `render()` and
     * before any other engine call — the next `render`/`process` may reallocate
     * `self.rgba`, and any wasm memory growth detaches the JS view.
     */
    rgba_ptr(): number;
    /**
     * Soft-wrap flag for a visible `row`: `true` if it continues the previous
     * row (autowrap), `undefined`/`None` when out of range.
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
     * lines, negative reveals newer. `render` already honors the display offset,
     * so the host only needs to redraw afterwards.
     */
    scroll_lines(delta: number): void;
    /**
     * Scroll the viewport so the match at absolute `line` is visible, placing it
     * at (or near) the top row. Clamps the target display_offset to the retained
     * scrollback so a live-region match snaps to the bottom. Host redraws after.
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
     * Search the full retained buffer (scrollback + visible) for `query`,
     * returning matches as a flat `[abs_line, start_col, len]` triplet array so
     * the JS host can highlight + scroll without re-scanning text. Lines are
     * ABSOLUTE rows (the index's native coordinate); the host maps them to
     * display rows via [`AtermTerminal::search_display_origin`] /
     * [`AtermTerminal::scroll_search_line_into_view`], which stay correct as the
     * viewport scrolls. Empty `query` (or a regex error) yields an empty array.
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
     * Serialize the terminal to a REPLAYABLE ANSI string — the aterm-native
     * replacement for `@xterm/addon-serialize`'s `serialize({scrollback})`, so the
     * renderer no longer needs a shadow xterm.js buffer to snapshot/restore/fork a
     * pane. Layout: SGR reset, then the capped recent history (text + CRLF), then
     * `CSI H`, then each visible row placed with absolute CUP + erase-line (so a
     * full-width row can't autowrap on replay) emitted via the engine's
     * `row_ansi_text` (minimal change-based SGR, wide-char aware), then the cursor
     * restored. `scrollback_rows` = `None` prepends ALL history, `Some(n)` the last
     * `n`, `Some(0)` viewport-only. Ported from the daemon's proven `serialize_ansi`
     * (orca-terminal headless) so the output stays byte-compatible with the existing
     * string-based replay pipeline.
     */
    serialize(scrollback_rows?: number | null): string;
    /**
     * Scrollback HISTORY ONLY (the off-screen lines above the viewport) as flowing
     * text + CRLF, no cursor/grid framing. Reads the MAIN buffer's scrollback (aterm
     * keeps it in the inactive grid while the alt screen is active) so an in-alt
     * (vim/htop/less) snapshot still recovers the pre-TUI history — the only
     * recoverable history on cold-restore of an alt-screen session. `max_rows` caps
     * to the last `n` lines (`None` = all). Mirrors the daemon's serialize_scrollback_ansi.
     */
    serialize_scrollback(max_rows?: number | null): string;
    /**
     * Inject a REAL bold weight of the primary family so SGR-bold cells render as a
     * true heavier weight instead of synthetic embolden. The host supplies the
     * bold-variant bytes (the canvas can't read the filesystem). No-throw: a bad
     * blob surfaces a catchable JS exception and leaves the existing weight intact.
     */
    set_bold_font(bytes: Uint8Array): void;
    /**
     * Tell the engine the real device-pixel cell size so its CSI 14t/16t
     * window/cell-pixel reports are accurate (the engine has no canvas otherwise).
     */
    set_cell_pixel_size(width: number, height: number): void;
    /**
     * Push the host OS color scheme into the engine. `dark = true` selects a dark
     * appearance, `false` light. When the scheme CHANGES and the app enabled DEC mode
     * 2031, the engine queues an unsolicited `CSI ? 997 ; Ps n` (1=dark, 2=light);
     * drain it via `take_response` and forward to the PTY so subscribed apps live-
     * update their theme. A no-op when the scheme is unchanged.
     */
    set_color_scheme(dark: boolean): void;
    /**
     * Set the cursor blink phase: `true` draws the cursor this frame, `false`
     * hides it. The host drives a ~530ms blink timer; independent of DECSCUSR.
     */
    set_cursor_blink_phase(on: boolean): void;
    /**
     * Force a hollow (unfocused) cursor when `true`, or restore the terminal's
     * DECSCUSR style when `false` — the standard focused/unfocused affordance.
     */
    set_cursor_hollow(hollow: boolean): void;
    set_default_background(r: number, g: number, b: number): void;
    /**
     * Set the host-preferred DEFAULT cursor style (shape used before any DECSCUSR and
     * restored after RIS/DECSTR). `n` follows the DECSCUSR convention: 1=blinking
     * block, 2=steady block, 3=blinking underline, 4=steady underline, 5=blinking bar,
     * 6=steady bar; out-of-range (0, 7+) is ignored. Unlike a render override this does
     * NOT clobber an app's live DECSCUSR (e.g. vim insert-mode bar).
     */
    set_default_cursor_style(n: number): void;
    /**
     * Seed the engine's DEFAULT foreground/background so its OSC 10/11 colour-query
     * replies report the host theme (the engine otherwise reports its built-in
     * defaults). RGB components, 0–255.
     */
    set_default_foreground(r: number, g: number, b: number): void;
    /**
     * Inject a colour-emoji (sbix) face from font bytes, driving the existing
     * ColorEmoji colour path. Same rationale as [`set_fallback_font`]: the host
     * supplies the OS emoji font. No-throw (the `String` Err surfaces as a
     * catchable JS exception); a bad blob leaves the slot untouched.
     */
    set_emoji_font(bytes: Uint8Array): void;
    /**
     * Inject a broad-coverage (CJK + symbols) fallback face from font bytes, so
     * glyphs the primary face lacks render real shapes instead of `.notdef` tofu.
     * The canvas renderer can't read the host filesystem, so the host pushes the
     * OS font bytes in. No-throw: a bad blob leaves the existing face untouched.
     */
    set_fallback_font(bytes: Uint8Array): void;
    /**
     * OpenType FONT FEATURES for the primary face, as a space-separated spec
     * (`"+ss01 zero -calt"` — bare/`+tag` enables, `-tag` disables, `tag=N` sets a
     * value). Mirrors the native `font_features` config knob. An empty/blank spec
     * clears all features. Preserves the current ligature mode; forces a repaint.
     */
    set_font_features(spec: string): void;
    /**
     * Programming LIGATURES on/off (`=>`, `!=`, `===` …). Mirrors the native
     * `ligatures` config knob so the in-page renderer honours the host's typography
     * setting instead of being pinned to the constructor default. Preserves any
     * configured `font_features`. Forces a full repaint so the change shows at once.
     */
    set_ligatures(on: boolean): void;
    /**
     * Scale the cell BOX height (the host's `terminalLineHeight`) WITHOUT changing
     * the glyph px, so rows space out while text keeps its size. The host re-reads
     * cell_height + recomputes the grid after.
     */
    set_line_height(scale: number): void;
    /**
     * Set an ANSI/indexed palette colour (index 0–255; 0–15 are the 16 ANSI
     * colours) to RGB components, so the renderer resolves SGR-indexed cell colours
     * through the host's theme palette instead of the engine's built-in VGA
     * defaults. Per-cell truecolor SGR still flows independently.
     */
    set_palette_color(index: number, r: number, g: number, b: number): void;
    /**
     * Swap the PRIMARY face (the host's `terminalFontFamily`) from font bytes and
     * re-rasterize. The host re-reads cell_width/cell_height + recomputes the grid
     * after (the new face may have different metrics). No-throw on a bad blob.
     */
    set_primary_font(bytes: Uint8Array): void;
    /**
     * Re-rasterize at a new cell font px (host DPI / devicePixelRatio change) so the
     * pane rebuilds its cell metrics instead of staying frozen at the construction
     * dpr. The host re-reads cell_width/cell_height + recomputes the grid after.
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
     * Set the explicit selected-text foreground (theme `selectionForeground`),
     * 0x00RRGGBB, or `undefined` to restore the WCAG contrast-floor default.
     * Appearance-only, so force one full repaint next frame.
     */
    set_selection_fg(fg?: number | null): void;
    /**
     * Mark the pane unfocused (`true`) / focused (`false`): when unfocused, the
     * selection band paints with the dimmer inactive bg (xterm
     * `selectionInactiveBackground`) instead of the active selection colour.
     * Appearance-only, so force one full repaint next frame.
     */
    set_selection_inactive(inactive: boolean): void;
    /**
     * Set the inactive (unfocused) selection background (0x00RRGGBB), or
     * `undefined` to derive it from the active selection bg blended toward the
     * theme bg. Only takes visible effect while the pane is marked unfocused.
     * Appearance-only, so force one full repaint next frame.
     */
    set_selection_inactive_bg(bg?: number | null): void;
    /**
     * Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB), so a host
     * theme change re-themes the pane without rebuilding it. Per-cell SGR colours
     * flow independently; pair with set_palette_color for the ANSI palette.
     */
    set_theme(fg: number, bg: number, cursor: number, selection: number): void;
    /**
     * Drain pending OSC app-events as a JSON array of `[code, payload]` pairs
     * (`[[7,"/home"],[52,"copied"]]`); `None` when the queue is empty. These
     * carry REAL decoded payloads (OSC 52 clipboard / OSC 7 cwd / OSC 133 mark)
     * the host routes to UI handlers — distinct from `take_response` (PTY replies).
     */
    take_osc_events(): string | undefined;
    /**
     * Drain the engine's pending query replies (DA1/DA2/DSR/CPR/DECRQM/OSC color/
     * window-size, …) — the host forwards these to the PTY so the RENDERER (not the
     * daemon, which stays silent) is the authoritative responder. Call after each
     * `process`; returns `None` when nothing is pending.
     */
    take_response(): Uint8Array | undefined;
    /**
     * The window title (OSC 0/2), or `None` when unset — replaces the separate
     * title channel that fed off the shadow xterm so snapshots keep window titles.
     */
    title(): string | undefined;
    /**
     * Absolute row index of the live/last line (xterm `buffer.active.baseY`):
     * `oldest_absolute_row() + scrollback_lines()`. `usize` → plain JS number.
     */
    readonly base_y: number;
    /**
     * Whether bracketed-paste mode (DECSET 2004) is active. The input seam reads
     * this to wrap pasted text in `ESC[200~ … ESC[201~` itself (replacing the old
     * reliance on xterm's `terminal.paste()`, which consulted xterm's own mode).
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
     * `CursorStyle` (1=BlinkingBlock, 2=SteadyBlock, 3=BlinkingUnderline,
     * 4=SteadyUnderline, 5=BlinkingBar, 6=SteadyBar, 7=Hidden, 8=HollowBlock).
     * The CPU renderer ALREADY paints this shape from the grid (cell_frame copies
     * it into the render input, draw_cursor honors it), so this getter exists for
     * host introspection/tests — no JS overlay is needed to draw the shape.
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
     * Last-rendered framebuffer height in pixels.
     */
    readonly height: number;
    /**
     * True when the alternate screen is active (TUIs own their own scrolling),
     * so the host should let wheel events pass through to the app.
     */
    readonly is_alt_screen: boolean;
    /**
     * True when DECCKM (application cursor keys) is set: the host must encode
     * arrows/Home/End as SS3 (ESC O A) instead of CSI (ESC [ A) so full-screen
     * apps (vi, less, readline) receive the sequences they expect.
     */
    readonly is_app_cursor_mode: boolean;
    /**
     * True when DEC mode 2031 (color-scheme update notifications) is set: the
     * app wants `CSI ? 997 ; n` on OS light/dark theme changes.
     */
    readonly is_color_scheme_updates_mode: boolean;
    /**
     * True when DECSET 1004 (focus reporting) is active: the host sends CSI I on
     * focus-in and CSI O on focus-out so apps (vim, tmux) track terminal focus.
     */
    readonly is_focus_event_mode: boolean;
    /**
     * True when a TUI has enabled mouse tracking (any of DECSET 9/1000/1002/1003).
     * The host then ENCODES canvas mouse events to the PTY instead of running
     * selection/scroll/link for them (unless Shift is held = user override).
     */
    readonly is_mouse_tracking: boolean;
    /**
     * True for AnyEvent (1003): report motion even with NO button pressed.
     * 1002 only reports motion while a button is held; the host uses this to
     * decide whether a button-less `mousemove` should be forwarded.
     */
    readonly mouse_wants_any_motion: boolean;
    /**
     * True when the active mouse mode reports MOTION (ButtonEvent 1002 = drag
     * while a button is down, AnyEvent 1003 = all motion), so the host only
     * forwards `mousemove` when an app actually wants it (no spam in 1000).
     */
    readonly mouse_wants_motion: boolean;
    /**
     * Absolute row of display row 0 at the live bottom (`display_offset == 0`):
     * `oldest_absolute_row + scrollback_lines`. A match at absolute `line` is at
     * display row `line - origin + display_offset`, so the host computes the
     * on-screen cell of any [`AtermTerminal::search`] match without a round-trip.
     */
    readonly search_display_origin: number;
    /**
     * Last-rendered framebuffer width in pixels.
     */
    readonly width: number;
}

/**
 * A detected link under a cell: its text/URL, the half-open display-column span
 * it covers, and a `kind` discriminant (0=osc8, 1=url, 2=file_path, 3=other).
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
 * `selection_text` and the painted highlight.
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
    readonly __wbg_atermterminal_free: (a: number, b: number) => void;
    readonly __wbg_linkhit_free: (a: number, b: number) => void;
    readonly __wbg_selectionrange_free: (a: number, b: number) => void;
    readonly atermterminal_add_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_authorize_clipboard_write: (a: number) => void;
    readonly atermterminal_base_y: (a: number) => number;
    readonly atermterminal_bracketed_paste_mode: (a: number) => number;
    readonly atermterminal_cell_height: (a: number) => number;
    readonly atermterminal_cell_is_wide: (a: number, b: number, c: number) => number;
    readonly atermterminal_cell_text: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_cell_width: (a: number) => number;
    readonly atermterminal_cursor_style: (a: number) => number;
    readonly atermterminal_cursor_x: (a: number) => number;
    readonly atermterminal_cursor_y: (a: number) => number;
    readonly atermterminal_display_offset: (a: number) => number;
    readonly atermterminal_display_origin_absolute: (a: number) => number;
    readonly atermterminal_drain_bell: (a: number) => number;
    readonly atermterminal_encode_mouse_motion: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_encode_mouse_press: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_encode_mouse_release: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_encode_mouse_wheel: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_height: (a: number) => number;
    readonly atermterminal_is_alt_screen: (a: number) => number;
    readonly atermterminal_is_app_cursor_mode: (a: number) => number;
    readonly atermterminal_is_color_scheme_updates_mode: (a: number) => number;
    readonly atermterminal_is_focus_event_mode: (a: number) => number;
    readonly atermterminal_is_mouse_tracking: (a: number) => number;
    readonly atermterminal_link_at: (a: number, b: number, c: number) => number;
    readonly atermterminal_mouse_wants_any_motion: (a: number) => number;
    readonly atermterminal_mouse_wants_motion: (a: number) => number;
    readonly atermterminal_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number];
    readonly atermterminal_process: (a: number, b: number, c: number) => void;
    readonly atermterminal_process_str: (a: number, b: number, c: number) => void;
    readonly atermterminal_render: (a: number) => void;
    readonly atermterminal_resize: (a: number, b: number, c: number) => void;
    readonly atermterminal_revoke_clipboard_write: (a: number) => void;
    readonly atermterminal_rgba: (a: number) => [number, number];
    readonly atermterminal_rgba_ptr: (a: number) => number;
    readonly atermterminal_row_is_wrapped: (a: number, b: number) => number;
    readonly atermterminal_row_len: (a: number, b: number) => number;
    readonly atermterminal_row_text: (a: number, b: number) => [number, number];
    readonly atermterminal_scroll_lines: (a: number, b: number) => void;
    readonly atermterminal_scroll_search_line_into_view: (a: number, b: number) => void;
    readonly atermterminal_scroll_to_bottom: (a: number) => void;
    readonly atermterminal_scroll_to_top: (a: number) => void;
    readonly atermterminal_search: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_search_display_origin: (a: number) => number;
    readonly atermterminal_selection_clear: (a: number) => void;
    readonly atermterminal_selection_extend: (a: number, b: number, c: number) => void;
    readonly atermterminal_selection_finish: (a: number) => void;
    readonly atermterminal_selection_line: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_selection_range: (a: number) => number;
    readonly atermterminal_selection_start: (a: number, b: number, c: number) => void;
    readonly atermterminal_selection_text: (a: number) => [number, number];
    readonly atermterminal_selection_word: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_serialize: (a: number, b: number) => [number, number];
    readonly atermterminal_serialize_scrollback: (a: number, b: number) => [number, number];
    readonly atermterminal_set_bold_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_cell_pixel_size: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_color_scheme: (a: number, b: number) => void;
    readonly atermterminal_set_cursor_blink_phase: (a: number, b: number) => void;
    readonly atermterminal_set_cursor_hollow: (a: number, b: number) => void;
    readonly atermterminal_set_default_background: (a: number, b: number, c: number, d: number) => void;
    readonly atermterminal_set_default_cursor_style: (a: number, b: number) => void;
    readonly atermterminal_set_default_foreground: (a: number, b: number, c: number, d: number) => void;
    readonly atermterminal_set_emoji_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_font_features: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_ligatures: (a: number, b: number) => void;
    readonly atermterminal_set_line_height: (a: number, b: number) => void;
    readonly atermterminal_set_palette_color: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_set_primary_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_px: (a: number, b: number) => void;
    readonly atermterminal_set_scrollback_limit: (a: number, b: number) => void;
    readonly atermterminal_set_selection_fg: (a: number, b: number) => void;
    readonly atermterminal_set_selection_inactive: (a: number, b: number) => void;
    readonly atermterminal_set_selection_inactive_bg: (a: number, b: number) => void;
    readonly atermterminal_set_theme: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_take_osc_events: (a: number) => [number, number];
    readonly atermterminal_take_response: (a: number) => [number, number];
    readonly atermterminal_title: (a: number) => [number, number];
    readonly atermterminal_width: (a: number) => number;
    readonly linkhit_end_col: (a: number) => number;
    readonly linkhit_kind: (a: number) => number;
    readonly linkhit_start_col: (a: number) => number;
    readonly linkhit_url: (a: number) => [number, number];
    readonly selectionrange_end_x: (a: number) => number;
    readonly selectionrange_end_y: (a: number) => number;
    readonly selectionrange_start_x: (a: number) => number;
    readonly selectionrange_start_y: (a: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
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
