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
     * Advance the effects clock by `dt_ms` (the host's rAF delta). The
     * engines never read a wall clock: same PTY bytes + same `dt` stream ⇒
     * identical frames. Negative/NaN deltas are ignored.
     */
    advance_effects(dt_ms: number): void;
    /**
     * Authorize OSC 52 clipboard *write* so the engine queues OSC 52 app-events
     * for the host to drain (see aterm-wasm). Without it the engine is fail-closed
     * (CF-004) and drops PTY-origin OSC 52 set sequences. The grid is shared, so
     * this covers both the GPU and CPU-fallback paths.
     */
    authorize_clipboard_write(): void;
    /**
     * Authorize (`true`) or revoke (`false`) OSC 9 / 99 / 777 desktop
     * notifications. The engine is fail-closed by default: until the host
     * authorizes, the notification handlers return before any dispatch, so
     * nothing reaches [`Self::take_notifications`]. Revoking restores that
     * default; already-queued notifications stay drainable (they were
     * authorized when dispatched).
     */
    authorize_notifications(allowed: boolean): void;
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
     * Milliseconds until the next scheduled idle one-shot (settled-cat blink /
     * ear-twitch), or `undefined` when none is armed. These arm while
     * `is_effects_active()` is `false`; a host that wants idle cat life
     * schedules one timer for this and resumes its frame loop there.
     */
    effects_next_deadline_ms(): number | undefined;
    /**
     * Encode a keyboard event through the engine's FULL encoder (legacy +
     * xterm modifyOtherKeys + Kitty), driven by the LIVE
     * `Terminal::keyboard_mode()`. `key` is a DOM `KeyboardEvent.key` value;
     * `mods` is the engine `Modifiers` bitfield (SHIFT=1, ALT=2, CTRL=4,
     * SUPER=8); `event_type` is 0=Press, 1=Repeat, 2=Release;
     * `base_layout_key` is the physical key's US-QWERTY char for Kitty
     * `REPORT_ALTERNATE_KEYS`. `None` when the event encodes to nothing or
     * the key has no terminal encoding. Mirrors aterm-wasm.
     */
    encode_key(key: string, mods: number, event_type: number, base_layout_key?: string | null): Uint8Array | undefined;
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
     * `true` while any effect is animating — keep the rAF loop running (call
     * `advance_effects` + `render`) only while this holds, then return to 0%
     * idle. Effects self-terminate to a stable state, so this always settles.
     */
    is_effects_active(): boolean;
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
     * Register one keystroke for the cursor-comet ignition: sustained fast
     * calls heat the typing cadence so the next `render` ignites the trail,
     * sparse/slow calls keep it gentle. The cadence reads the effects clock,
     * so the host must `advance_effects` between keystrokes for it to reflect
     * real time. Call this from the SAME JS keydown handler that feeds
     * `encode_key`; without it the comet stays dormant on web hosts.
     */
    note_keystroke(): void;
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
     * the same `cell_frame` snapshot, the same `Frame` (0xTTRRGGBB; TT is the
     * transmittance byte, 0 = opaque) expanded to RGBA8 (alpha 0xff except on
     * default-bg pixels under `set_background_opacity`). Errors if WebGL was
     * not initialized.
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
     * Set the DEFAULT-background opacity (0..=1; Ghostty's
     * `background-opacity`; `1.0` = opaque, the default — byte-identical
     * output). Only pixels whose bg resolved to the frame's DEFAULT
     * background go translucent; SGR-colored bg cells, the selection band and
     * glyph pixels stay opaque. Set on both the CPU fallback face and the
     * live GPU renderer; forces a full present (appearance-only, not
     * content). NOTE: the on-glass effect additionally needs a canvas/surface
     * that composites alpha; the offscreen readback (`render_offscreen` +
     * `rgba`) carries the alpha either way.
     */
    set_background_opacity(opacity: number): void;
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
     * Configure the LUMEN cursor aurora (additive light in the cursor's
     * wake). Mirrors the native knobs + clamps: `style` ∈
     * `lumen|rainbow|sparkle|fire|laser|water` (unknown → lumen);
     * `color`/`accent` omitted derive from the theme cursor (accent = color
     * brightened 1.5×) exactly like the native app; `duration_ms` clamps
     * 30..=2000, `length` (cells) 1..=512, `intensity` 0..=1 (0 = off),
     * `radius` (bloom crown, cells) 0..=2, `ring` = landing-ring ping.
     */
    set_cursor_glow(enabled: boolean, style: string, color: number | null | undefined, accent: number | null | undefined, duration_ms: number, length: number, intensity: number, radius: number, ring: boolean): void;
    /**
     * Force a hollow (unfocused) cursor when `true`, or restore the terminal's
     * DECSCUSR style when `false`. Applies to both GPU and CPU faces.
     */
    set_cursor_hollow(hollow: boolean): void;
    /**
     * Set the CURSOR-fill opacity (0..=1; Ghostty's `cursor-opacity`; `1.0` =
     * opaque fill + block cut-out, the default — byte-identical output).
     * Below 1.0 the cursor fill blends over the cell so the glyph shows
     * through. Set on both the CPU fallback face and the live GPU renderer;
     * forces a full present (appearance-only, not content).
     */
    set_cursor_opacity(opacity: number): void;
    /**
     * Configure the legacy opaque comet trail (the native `cursor_trail_style
     * = "comet"` look). `color` omitted = the theme cursor; `duration_ms`
     * clamps 30..=2000, `length` 1..=512. Exactly one of trail/glow is on in
     * the native app (chosen by style); the embedder decides here.
     */
    set_cursor_trail(enabled: boolean, duration_ms: number, length: number, color?: number | null): void;
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
     * Focus gate for the idle one-shots (`§5.6`): an unfocused pane fires no
     * blink events (and freezes their fingerprints). Pass the pane focus.
     */
    set_effects_focused(focused: boolean): void;
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
     * Enable/disable the Kitty keyboard protocol capability (default ON). When
     * disabled the engine acts as if the protocol is unsupported — no `CSI ? u`
     * reply, push/set/pop consumed-and-ignored, `keyboard_mode` never carries
     * kitty bits — for hosts whose platform consumes kitty sequences itself
     * (Windows ConPTY; xterm.js `vtExtensions.kittyKeyboard = false`). The
     * engine (`term`) survives `init`, so no pre-init retention is needed.
     * Mirrors aterm-wasm.
     */
    set_kitty_keyboard_enabled(enabled: boolean): void;
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
     * Set the per-cell minimum contrast ratio (xterm's `minimumContrastRatio`,
     * 1..=21; `ratio <= 1.0` = off, the default — xterm treats 1 as "do
     * nothing"): every glyph fg is floored against its OWN cell bg. Cells whose
     * fg == bg are never adjusted (SGR 8 conceal renders fg = bg and must stay
     * hidden). Set on both the CPU fallback face and the live GPU renderer;
     * forces a full present (appearance-only, not content).
     */
    set_minimum_contrast(ratio: number): void;
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
     * Per-class gates (native `[sparkle_words.<class>] enabled`): profanity
     * (supernova/sparkle), feline (peeking cat/paw), orca (water splash),
     * emphasis (ink-only; effective only while ink is enabled).
     */
    set_sparkle_classes(profanity: boolean, feline: boolean, orca: boolean, emphasis: boolean): void;
    /**
     * Comma-separated exact surfaces to never decorate (the native global
     * `deny` and `ignore_words` channel), replacing the current set. Entries
     * are case/diacritic-folded with the scanner's own fold.
     */
    set_sparkle_deny(words_csv: string): void;
    /**
     * Feline knobs (native `[sparkle_words.feline]`): `style` = "cat" (the
     * v2 peeking cat, default) or "paw" (the exact v1 steady paw); `color`
     * omitted = the native soft pink; `intensity` clamps 0..=1; `idle` =
     * sparse blink/ear-twitch one-shots (focus-gated, ≤1/s); `gaze` = pupils
     * track the cursor (present-driven, zero new wakes); `magic` = rare
     * Fortune/Nebula cats; `allow_bare_cat` = decorate the literal 3-letter
     * `cat`; `cjk_single_char` = match a lone cat ideograph (high-FP).
     */
    set_sparkle_feline(style: string, color: number | null | undefined, intensity: number, idle: boolean, gaze: boolean, magic: boolean, allow_bare_cat: boolean, cjk_single_char: boolean): void;
    /**
     * Animated-ink knobs (native `[sparkle_words.ink]`): the glyph-ink
     * gradient + specular sweep on matched words. `strength` clamps 0..=1;
     * `sweep_ms` clamps 350..=6000 (floor 600 while `loop_` — the WCAG flash
     * margin, structural); `loop_` re-sweeps while the word stays visible.
     */
    set_sparkle_ink(enabled: boolean, strength: number, sweep_ms: number, loop_: boolean): void;
    /**
     * Comma-separated languages whose AMBIGUOUS homograph lexicon entries
     * un-gate (native `languages`, default `"en"`; non-ambiguous forms load
     * regardless; `"all"` un-gates everything). Rebuilds the lexicon.
     */
    set_sparkle_languages(languages_csv: string): void;
    /**
     * User lexicon-override TOML merged over the builtin (the native
     * `lexicon` file / `extra_words` channel — the same `[[entry]]` schema).
     * Pass `undefined` to clear. A malformed override falls back to the
     * builtin lexicon (the native fail-open posture).
     */
    set_sparkle_lexicon_override(toml?: string | null): void;
    /**
     * Profanity knobs (native `[sparkle_words.profanity]`): `style` = "nova"
     * (the v2 supernova, default) or "sparkle" (the exact v1 twinkle).
     * Clamps are the native flash-safety floors and are not bypassable:
     * `density` 1..=12 sparks, `anim_ms` 350..=10000, `jitter` 0..=6 px,
     * `intensity` 0..=1. `magic` = rare Quasar/Singularity novas. The
     * window-wide ignition limiter (≤2 novas per rolling second) is always on.
     */
    set_sparkle_profanity(style: string, density: number, anim_ms: number, jitter: number, intensity: number, magic: boolean): void;
    /**
     * Force the static, non-animating path (no twinkle/jitter/sweep; novas
     * collapse to a static glint) — the accessibility `reduced_motion`
     * override. The engine's flash-limiter floors apply regardless.
     */
    set_sparkle_reduced_motion(on: boolean): void;
    /**
     * MASTER sparkle-words switch (native `[sparkle_words] enabled` +
     * `toggle_sparkle_words` panic-off). Enabling compiles the multilingual
     * lexicon once and starts scanning the visible grid; disabling drops all
     * occurrence state and restores byte-identical output next render.
     * Defaults (until other setters run) mirror the native launch config:
     * all four families on (profanity nova / feline cat / orca splash /
     * emphasis ink), animated ink on.
     */
    set_sparkle_words_enabled(on: boolean): void;
    /**
     * Inject a broad-coverage SYMBOL fallback face from font bytes (the
     * byte-injection sibling of the config `symbol_font` path). Applies to the
     * CPU face and the live GPU face if `init` already ran; remembered so `init`
     * re-applies it to the fresh GPU face. No-throw: a bad blob leaves the
     * existing faces untouched.
     */
    set_symbol_font(bytes: Uint8Array): void;
    /**
     * Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB) on both the
     * GPU renderer and the CPU face, so a host theme change re-themes the pane
     * without a device/face rebuild.
     */
    set_theme(fg: number, bg: number, cursor: number, selection: number): void;
    /**
     * Override the characters that BREAK a double-click word (the host's
     * word-separator setting, xterm.js `wordSeparators` semantics): a word
     * becomes a maximal run of NON-separator characters. `undefined` restores
     * the engine's default class-based word logic (alphanumeric + `_`)
     * exactly. Smart-selection RULES (url/file_path/email/…) still take
     * precedence for both `selection_word` and `link_at`; the separators only
     * shape the plain-word fallback. Mirrors aterm-wasm.
     */
    set_word_separators(separators?: string | null): void;
    /**
     * Drain pending desktop notifications (queued since the last drain) as a
     * JSON array of `{"id","title","body","urgency"}` objects — string or
     * `null` fields, urgency ∈ `"low"|"normal"|"critical"`; `None` when
     * nothing is pending. OSC 9's bare message arrives as `body` with no
     * title (the native mapping); OSC 99/777 carry their structured
     * id/title/body. The queue is bounded (new notifications are dropped
     * beyond the cap until drained), so poll after `process` like
     * `take_osc_events`.
     */
    take_notifications(): string | undefined;
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
     * The LIVE application cursor colour (OSC 12) as packed `0x00RRGGBB`, or
     * `undefined` while unset / after an OSC 112 reset — i.e. the host/theme
     * default applies. Read per frame so glow/trail colour derivation can
     * follow app-driven cursor-colour changes (the renderer already draws
     * the cursor itself with this colour). Mirrors aterm-wasm.
     */
    readonly cursor_color: number | undefined;
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
     * True when DEC private mode 1007 (alternate scroll) is set: while the
     * alternate screen is active and mouse tracking is off, the host converts
     * wheel ticks into arrow-key presses (aterm-gui's WheelPlan behaviour) so
     * TUIs without mouse support still wheel-scroll. Mirrors aterm-wasm.
     */
    readonly is_alternate_scroll: boolean;
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
     * The live `Terminal::keyboard_mode()` as its raw bitflags value, for
     * hosts that run the engine in a Web Worker: mirror these bits into the
     * main-thread engine-state snapshot and feed them to the free
     * [`encode_key_with_mode`], which encodes keydowns synchronously without
     * an instance. `KeyboardMode` is a `bitflags` struct over `u16` (bits
     * 0..=14 defined); the value is zero-extended to `u32` for headroom.
     * Mirrors aterm-wasm.
     */
    readonly keyboard_mode_bits: number;
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
     * Whether the sparkle-words master is currently on.
     */
    readonly sparkle_words_enabled: boolean;
    /**
     * Width in pixels of the last [`render_offscreen`](Self::render_offscreen)
     * framebuffer.
     */
    readonly width: number;
}

/**
 * One Living-Panel scene instance + its telemetry bus, ticked by the host
 * and rasterized to RGBA8. See the module docs for the drive contract.
 */
export class AtermScene {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Rebind which telemetry drives a behaviour (data, not code — the native
     * manifest channel). `drive` is a [`Drive`] name (`energy`, `crowd`,
     * `arrivals`, `departures`, `butterflies`, `weather`, `traffic`,
     * `daylight`); `source` is a dotted system-signal name (`sys.cpu`, …),
     * `app:<name>`, or `const:<0..1>`. Returns `false` when either fails to
     * parse.
     */
    bind_drive(drive: string, source: string): boolean;
    /**
     * Mark a system signal as unavailable again (back to ABSENT). Returns
     * `false` for an unknown key.
     */
    clear_signal(key: string): boolean;
    /**
     * Human/inspection summary (the native `controls scenes` dump).
     */
    describe(): string;
    /**
     * The scene's stable id (`"placeholder"` until the art rewrite lands).
     */
    id(): string;
    /**
     * `true` while something is still moving — mirror of the terminal's
     * `is_effects_active`: keep the rAF loop only while animating (or while
     * signals keep changing), else drop to 0% idle.
     */
    is_active(): boolean;
    /**
     * Build a scene by `name` (see [`scene_names_csv`]; unknown → the inert
     * placeholder) with its default stat→behaviour binding. `seed` makes the
     * generation deterministic-per-panel; `w`×`h` is the panel pixel box;
     * `bg` is the packed `0x00RRGGBB` the frame composites over.
     */
    constructor(name: string, seed: number, w: number, h: number, bg: number);
    /**
     * A console text-entry pulse (one per real printable keystroke) — the
     * "typing drops a butterfly" hook.
     */
    on_text(printable: boolean): void;
    /**
     * Emit + composite the current frame into the internal RGBA8 buffer
     * (straight-alpha, opaque background — ready for `putImageData` or a
     * `drawImage` layer). Read back via `rgba`/`rgba_ptr` + `width`/`height`.
     */
    render(): void;
    /**
     * Copy of the last-rendered RGBA8 frame (`width*height*4` bytes).
     */
    rgba(): Uint8Array;
    /**
     * Byte offset of the RGBA8 frame within wasm linear memory for a
     * zero-copy view (same caveats as `AtermGpuTerminal`'s offscreen `rgba`: read it
     * synchronously after `render`, before any other call on this instance).
     */
    rgba_ptr(): number;
    /**
     * Push an app-fed named stream (the `aterm-ctl metric <name>` channel):
     * arbitrary host streams (`"ai.tokens"`, `"build.pct"`, …) a binding can
     * map onto a drive via `bind_drive("...", "app:<name>")`.
     */
    set_app_signal(name: string, norm: number, value: number, rate: number): void;
    /**
     * Panel background colour (`0x00RRGGBB`) the frame composites over.
     */
    set_background(bg: number): void;
    /**
     * Scale a drive's resolved value (the binding `gain` channel).
     */
    set_drive_gain(drive: string, gain: number): boolean;
    /**
     * Force night (`true`) / day (`false`), or `undefined` to let the scene's
     * own day drive decide.
     */
    set_night(night?: boolean | null): void;
    /**
     * Theme the scene from the host colorscheme. All colours are packed
     * `0x00RRGGBB`; the argument order matches [`aterm_scene::Palette`].
     */
    set_palette(ink: number, dim: number, sky_day_top: number, sky_day_bot: number, sky_night_top: number, sky_night_bot: number, hill: number, grass: number, grass_dark: number, sun: number, accent: number, good: number, warn: number, hot: number): void;
    /**
     * Honor the OS/user reduce-motion setting: dampened speeds, no particles.
     */
    set_reduced_motion(on: boolean): void;
    /**
     * Push one sampled system/engine signal onto the telemetry bus. `key` is
     * a dotted [`SignalKey`] name (`sys.cpu`, `sys.mem`, `sys.gpu`,
     * `sys.disk`, `net.rx`, `net.tx`, `ses.cpu`, `ses.mem`, `engine.fps`,
     * `engine.frame_ms`, `engine.present_ms`, `engine.slow_frames`);
     * `norm` is the normalized behaviour
     * value in `[0,1]`; `value`/`rate` are the raw readout units. Returns
     * `false` for an unknown key. A signal the host cannot sample must simply
     * never be pushed — absent stays honest (`None`), never a fake 0.
     */
    set_signal(key: string, norm: number, value: number, rate: number): boolean;
    /**
     * Resize the panel box (pixels).
     */
    set_size(w: number, h: number): void;
    /**
     * Advance the scene by `dt_ms` under the currently-pushed signals.
     * Deterministic: same seed + same `dt`/signal stream ⇒ identical frames.
     * Negative/NaN deltas are ignored; one tick is clamped to 250 ms so a
     * backgrounded tab fast-forwards smoothly instead of exploding kinematics.
     */
    tick(dt_ms: number): void;
    /**
     * Last-rendered frame height in pixels.
     */
    readonly height: number;
    /**
     * Emitted sprite count of the LAST rendered frame (both layers) — the
     * bounded per-frame draw budget, for host diagnostics/tests.
     */
    readonly sprite_count: number;
    /**
     * Last-rendered frame width in pixels.
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

/**
 * STATELESS key encoder for worker-hosted engines: encode a DOM keyboard
 * event against an explicit mode-bits snapshot instead of a live terminal.
 *
 * Contract: the engine lives in a Web Worker while keydown handling runs on
 * the main thread, so the host mirrors
 * [`AtermGpuTerminal::keyboard_mode_bits`] through its engine-state snapshot
 * and encodes synchronously here, accepting one-frame staleness — the same
 * tradeoff the host already accepts for DECCKM gating via
 * `is_app_cursor_mode`.
 *
 * Parameters match [`AtermGpuTerminal::encode_key`] (`key` = DOM
 * `KeyboardEvent.key`; `mods` = SHIFT=1, ALT=2, CTRL=4, SUPER=8;
 * `event_type` = 0=Press, 1=Repeat, 2=Release; `base_layout_key` = US-QWERTY
 * char for Kitty `REPORT_ALTERNATE_KEYS`), plus `mode_bits` from
 * `keyboard_mode_bits` (a `u16` bitflags value zero-extended to `u32`;
 * undefined bits are truncated away). With fresh bits the output is
 * byte-identical to the instance method. Mirrors aterm-wasm.
 */
export function encode_key_with_mode(key: string, mods: number, event_type: number, base_layout_key: string | null | undefined, mode_bits: number): Uint8Array | undefined;

/**
 * Every built-in scene name, comma-separated (empty until the scene-art
 * rewrite re-populates the registry; unknown names build the inert
 * placeholder).
 */
export function scene_names_csv(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_atermgputerminal_free: (a: number, b: number) => void;
    readonly __wbg_atermscene_free: (a: number, b: number) => void;
    readonly __wbg_linkhit_free: (a: number, b: number) => void;
    readonly __wbg_selectionrange_free: (a: number, b: number) => void;
    readonly atermgputerminal_adapter_info: (a: number) => [number, number];
    readonly atermgputerminal_add_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_advance_effects: (a: number, b: number) => void;
    readonly atermgputerminal_authorize_clipboard_write: (a: number) => void;
    readonly atermgputerminal_authorize_notifications: (a: number, b: number) => void;
    readonly atermgputerminal_base_y: (a: number) => number;
    readonly atermgputerminal_bracketed_paste_mode: (a: number) => number;
    readonly atermgputerminal_cell_height: (a: number) => number;
    readonly atermgputerminal_cell_is_wide: (a: number, b: number, c: number) => number;
    readonly atermgputerminal_cell_text: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_cell_width: (a: number) => number;
    readonly atermgputerminal_cursor_color: (a: number) => number;
    readonly atermgputerminal_cursor_style: (a: number) => number;
    readonly atermgputerminal_cursor_x: (a: number) => number;
    readonly atermgputerminal_cursor_y: (a: number) => number;
    readonly atermgputerminal_display_offset: (a: number) => number;
    readonly atermgputerminal_display_origin_absolute: (a: number) => number;
    readonly atermgputerminal_drain_bell: (a: number) => number;
    readonly atermgputerminal_effects_next_deadline_ms: (a: number) => [number, number];
    readonly atermgputerminal_encode_key: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly atermgputerminal_encode_mouse_motion: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_encode_mouse_press: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_encode_mouse_release: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_encode_mouse_wheel: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermgputerminal_gpu_ready: (a: number) => number;
    readonly atermgputerminal_height: (a: number) => number;
    readonly atermgputerminal_init: (a: number, b: any) => any;
    readonly atermgputerminal_init_offscreen: (a: number, b: any) => any;
    readonly atermgputerminal_is_alt_screen: (a: number) => number;
    readonly atermgputerminal_is_alternate_scroll: (a: number) => number;
    readonly atermgputerminal_is_app_cursor_mode: (a: number) => number;
    readonly atermgputerminal_is_color_scheme_updates_mode: (a: number) => number;
    readonly atermgputerminal_is_effects_active: (a: number) => number;
    readonly atermgputerminal_is_focus_event_mode: (a: number) => number;
    readonly atermgputerminal_is_mouse_tracking: (a: number) => number;
    readonly atermgputerminal_keyboard_mode_bits: (a: number) => number;
    readonly atermgputerminal_link_at: (a: number, b: number, c: number) => number;
    readonly atermgputerminal_mouse_wants_any_motion: (a: number) => number;
    readonly atermgputerminal_mouse_wants_motion: (a: number) => number;
    readonly atermgputerminal_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number];
    readonly atermgputerminal_note_keystroke: (a: number) => void;
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
    readonly atermgputerminal_set_background_opacity: (a: number, b: number) => void;
    readonly atermgputerminal_set_bold_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_cell_pixel_size: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_color_scheme: (a: number, b: number) => void;
    readonly atermgputerminal_set_cursor_blink_phase: (a: number, b: number) => void;
    readonly atermgputerminal_set_cursor_glow: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number) => void;
    readonly atermgputerminal_set_cursor_hollow: (a: number, b: number) => void;
    readonly atermgputerminal_set_cursor_opacity: (a: number, b: number) => void;
    readonly atermgputerminal_set_cursor_trail: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_set_default_background: (a: number, b: number, c: number, d: number) => void;
    readonly atermgputerminal_set_default_cursor_style: (a: number, b: number) => void;
    readonly atermgputerminal_set_default_foreground: (a: number, b: number, c: number, d: number) => void;
    readonly atermgputerminal_set_effects_focused: (a: number, b: number) => void;
    readonly atermgputerminal_set_emoji_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_font_features: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_kitty_keyboard_enabled: (a: number, b: number) => void;
    readonly atermgputerminal_set_ligatures: (a: number, b: number) => void;
    readonly atermgputerminal_set_line_height: (a: number, b: number) => void;
    readonly atermgputerminal_set_minimum_contrast: (a: number, b: number) => void;
    readonly atermgputerminal_set_palette_color: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_set_primary_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_px: (a: number, b: number) => void;
    readonly atermgputerminal_set_scrollback_limit: (a: number, b: number) => void;
    readonly atermgputerminal_set_selection_fg: (a: number, b: number) => void;
    readonly atermgputerminal_set_selection_inactive: (a: number, b: number) => void;
    readonly atermgputerminal_set_selection_inactive_bg: (a: number, b: number) => void;
    readonly atermgputerminal_set_sparkle_classes: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_set_sparkle_deny: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_sparkle_feline: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => void;
    readonly atermgputerminal_set_sparkle_ink: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_set_sparkle_languages: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_sparkle_lexicon_override: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_set_sparkle_profanity: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => void;
    readonly atermgputerminal_set_sparkle_reduced_motion: (a: number, b: number) => void;
    readonly atermgputerminal_set_sparkle_words_enabled: (a: number, b: number) => void;
    readonly atermgputerminal_set_symbol_font: (a: number, b: number, c: number) => [number, number];
    readonly atermgputerminal_set_theme: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermgputerminal_set_word_separators: (a: number, b: number, c: number) => void;
    readonly atermgputerminal_sparkle_words_enabled: (a: number) => number;
    readonly atermgputerminal_take_notifications: (a: number) => [number, number];
    readonly atermgputerminal_take_osc_events: (a: number) => [number, number];
    readonly atermgputerminal_take_response: (a: number) => [number, number];
    readonly atermgputerminal_title: (a: number) => [number, number];
    readonly atermgputerminal_width: (a: number) => number;
    readonly atermscene_bind_drive: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly atermscene_clear_signal: (a: number, b: number, c: number) => number;
    readonly atermscene_describe: (a: number) => [number, number];
    readonly atermscene_height: (a: number) => number;
    readonly atermscene_id: (a: number) => [number, number];
    readonly atermscene_is_active: (a: number) => number;
    readonly atermscene_new: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly atermscene_on_text: (a: number, b: number) => void;
    readonly atermscene_render: (a: number) => void;
    readonly atermscene_rgba: (a: number) => [number, number];
    readonly atermscene_rgba_ptr: (a: number) => number;
    readonly atermscene_set_app_signal: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly atermscene_set_background: (a: number, b: number) => void;
    readonly atermscene_set_drive_gain: (a: number, b: number, c: number, d: number) => number;
    readonly atermscene_set_night: (a: number, b: number) => void;
    readonly atermscene_set_palette: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number, n: number, o: number) => void;
    readonly atermscene_set_reduced_motion: (a: number, b: number) => void;
    readonly atermscene_set_signal: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly atermscene_set_size: (a: number, b: number, c: number) => void;
    readonly atermscene_sprite_count: (a: number) => number;
    readonly atermscene_tick: (a: number, b: number) => void;
    readonly atermscene_width: (a: number) => number;
    readonly encode_key_with_mode: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly linkhit_end_col: (a: number) => number;
    readonly linkhit_kind: (a: number) => number;
    readonly linkhit_start_col: (a: number) => number;
    readonly linkhit_url: (a: number) => [number, number];
    readonly scene_names_csv: () => [number, number];
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
