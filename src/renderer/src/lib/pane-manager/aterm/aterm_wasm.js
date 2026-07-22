/* @ts-self-types="./aterm_wasm.d.ts" */

/**
 * A terminal + CPU renderer pair. Feed PTY bytes with [`AtermTerminal::process`],
 * then [`AtermTerminal::render`] to refresh the RGBA framebuffer, then read it
 * back via [`AtermTerminal::rgba`] (+ `width`/`height`) to draw onto a canvas.
 */
export class AtermTerminal {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(AtermTerminal.prototype);
        obj.__wbg_ptr = ptr;
        AtermTerminalFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        AtermTerminalFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_atermterminal_free(ptr, 0);
    }
    /**
     * APPEND another fallback face to the chain (does NOT reset it like
     * [`set_fallback_font`]). The chain is tried in order, so the host can push a
     * CJK fallback first then Arabic/Devanagari/Thai/Hebrew faces after it — a
     * glyph the earlier faces miss still reaches a covering face instead of tofu.
     * No-throw: a bad blob leaves the existing chain untouched.
     * @param {Uint8Array} bytes
     */
    add_fallback_font(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_add_fallback_font(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * [`AtermTerminal::add_fallback_font`] from a registered handle.
     * @param {number} handle
     */
    add_fallback_font_registered(handle) {
        const ret = wasm.atermterminal_add_fallback_font_registered(this.__wbg_ptr, handle);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Advance the effects clock by `dt_ms` (the host's rAF delta). The
     * engines never read a wall clock: same PTY bytes + same `dt` stream ⇒
     * identical frames. Negative/NaN deltas are ignored.
     * @param {number} dt_ms
     */
    advance_effects(dt_ms) {
        wasm.atermterminal_advance_effects(this.__wbg_ptr, dt_ms);
    }
    /**
     * Authorize OSC 52 clipboard *write* (set) so the engine queues OSC 52
     * app-events for the host to drain via `take_osc_events`. Without this the
     * engine is fail-closed (CF-004) and silently drops PTY-origin OSC 52 set
     * sequences, so they never reach the host. The host still gates the actual
     * clipboard write on its own user setting (defense in depth).
     */
    authorize_clipboard_write() {
        wasm.atermterminal_authorize_clipboard_write(this.__wbg_ptr);
    }
    /**
     * Authorize (`true`) or revoke (`false`) OSC 9 / 99 / 777 desktop
     * notifications. The engine is fail-closed by default: until the host
     * authorizes, the notification handlers return before any dispatch, so
     * nothing reaches [`Self::take_notifications`]. Revoking restores that
     * default; already-queued notifications stay drainable (they were
     * authorized when dispatched).
     * @param {boolean} allowed
     */
    authorize_notifications(allowed) {
        wasm.atermterminal_authorize_notifications(this.__wbg_ptr, allowed);
    }
    /**
     * Absolute row index of the live/last line (xterm `buffer.active.baseY`):
     * `oldest_absolute_row() + scrollback_lines()`. `usize` → plain JS number.
     * @returns {number}
     */
    get base_y() {
        const ret = wasm.atermterminal_base_y(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Whether bracketed-paste mode (DECSET 2004) is active. The input seam reads
     * this to wrap pasted text in `ESC[200~ … ESC[201~` itself (replacing the old
     * reliance on xterm's `terminal.paste()`, which consulted xterm's own mode).
     * @returns {boolean}
     */
    get bracketed_paste_mode() {
        const ret = wasm.atermterminal_bracketed_paste_mode(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Cell height in device pixels — the host computes rows = floor(canvasH / cellHeight).
     * @returns {number}
     */
    get cell_height() {
        const ret = wasm.atermterminal_cell_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Whether the DISPLAY cell at `row`/`col` is a wide (double-width)
     * character; `None` when out of range. Resolved through the same
     * display-offset-aware row view as `cell_text` so a host's per-cell walk
     * sees one coherent row.
     * @param {number} row
     * @param {number} col
     * @returns {boolean | undefined}
     */
    cell_is_wide(row, col) {
        const ret = wasm.atermterminal_cell_is_wide(this.__wbg_ptr, row, col);
        return ret === 0xFFFFFF ? undefined : ret !== 0;
    }
    /**
     * Grapheme text at DISPLAY cell `row`/`col` (display_offset-aware, like
     * `row_text`) — base char plus complex cluster and combining marks. Empty
     * string for a blank cell, a wide-continuation spacer, or out-of-range
     * coords. Hosts rebuild scrolled-back rows per-cell from this, so it must
     * track the scroll position; the live-frame reader is `get_line_text`.
     * @param {number} row
     * @param {number} col
     * @returns {string}
     */
    cell_text(row, col) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.atermterminal_cell_text(this.__wbg_ptr, row, col);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Cell width in device pixels — the host computes cols = floor(canvasW / cellWidth).
     * @returns {number}
     */
    get cell_width() {
        const ret = wasm.atermterminal_cell_width(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The chrome top head band set via [`Self::set_chrome`] (px; 0 = none).
     * @returns {number}
     */
    get chrome_head() {
        const ret = wasm.atermterminal_chrome_head(this.__wbg_ptr);
        return ret;
    }
    /**
     * The chrome interior padding set via [`Self::set_chrome`] (px; 0 = exact fit).
     * Hosts read these back so canvas offsets and pointer math share one truth.
     * @returns {number}
     */
    get chrome_pad() {
        const ret = wasm.atermterminal_chrome_pad(this.__wbg_ptr);
        return ret;
    }
    /**
     * The LIVE application cursor colour (OSC 12) as packed `0x00RRGGBB`, or
     * `undefined` while unset / after an OSC 112 reset — i.e. the host/theme
     * default applies. Read per frame so glow/trail colour derivation can
     * follow app-driven cursor-colour changes (the renderer already draws
     * the cursor itself with this colour).
     * @returns {number | undefined}
     */
    get cursor_color() {
        const ret = wasm.atermterminal_cursor_color(this.__wbg_ptr);
        return ret === 0x100000001 ? undefined : ret;
    }
    /**
     * Active DECSCUSR cursor style as the discriminant of `aterm_core`'s
     * `CursorStyle` (1=BlinkingBlock, 2=SteadyBlock, 3=BlinkingUnderline,
     * 4=SteadyUnderline, 5=BlinkingBar, 6=SteadyBar, 7=Hidden, 8=HollowBlock).
     * The CPU renderer ALREADY paints this shape from the grid (cell_frame copies
     * it into the render input, draw_cursor honors it), so this getter exists for
     * host introspection/tests — no JS overlay is needed to draw the shape.
     * @returns {number}
     */
    get cursor_style() {
        const ret = wasm.atermterminal_cursor_style(this.__wbg_ptr);
        return ret;
    }
    /**
     * Display-relative cursor column (0-based).
     * @returns {number}
     */
    get cursor_x() {
        const ret = wasm.atermterminal_cursor_x(this.__wbg_ptr);
        return ret;
    }
    /**
     * Display-relative cursor row (0-based, top of viewport).
     * @returns {number}
     */
    get cursor_y() {
        const ret = wasm.atermterminal_cursor_y(this.__wbg_ptr);
        return ret;
    }
    /**
     * Lines the viewport is scrolled up from the live bottom (0 = at bottom).
     * @returns {number}
     */
    get display_offset() {
        const ret = wasm.atermterminal_display_offset(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Absolute row index of the TOP visible line for the current viewport
     * (`base_y - display_offset`); the search/link origin.
     * @returns {number}
     */
    get display_origin_absolute() {
        const ret = wasm.atermterminal_display_origin_absolute(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Drain the edge-triggered BEL flag: `true` if a BEL fired since the last
     * call, then clears it (so a poll-based host can flash/ring without the
     * synchronous bell callback).
     * @returns {boolean}
     */
    drain_bell() {
        const ret = wasm.atermterminal_drain_bell(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Milliseconds until the next rain engine tick, or `undefined` when
     * active frame-rate motion needs rAF (and when every effect is idle).
     * @returns {number | undefined}
     */
    effects_next_deadline_ms() {
        const ret = wasm.atermterminal_effects_next_deadline_ms(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Encode a keyboard event through the engine's FULL encoder — legacy +
     * xterm modifyOtherKeys + Kitty progressive enhancement, driven by the
     * LIVE `Terminal::keyboard_mode()` (DECCKM/DECBKM/1035/1036/1039 and the
     * negotiated Kitty flags are exact), replacing the host's legacy-only TS
     * encoder that acked Kitty on the wire but could never speak it.
     *
     * `key` is a DOM `KeyboardEvent.key` value (mapped by the shared
     * `aterm_types::keyboard::map_dom_key` table); `mods` is the engine
     * `Modifiers` bitfield (SHIFT=1, ALT=2, CTRL=4, SUPER=8); `event_type` is
     * 0=Press, 1=Repeat, 2=Release; `base_layout_key` is the US-QWERTY char of
     * the physical key for Kitty `REPORT_ALTERNATE_KEYS` (pass `undefined`
     * when unknown). Returns `None` when the event encodes to nothing (e.g. a
     * release without the Kitty protocol) or the key has no terminal encoding
     * (modifier-only / IME / unidentified DOM keys — never guessed).
     * @param {string} key
     * @param {number} mods
     * @param {number} event_type
     * @param {string | null} [base_layout_key]
     * @returns {Uint8Array | undefined}
     */
    encode_key(key, mods, event_type, base_layout_key) {
        const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const char1 = isLikeNone(base_layout_key) ? 0xFFFFFF : base_layout_key.codePointAt(0);
        if (char1 !== 0xFFFFFF) { _assertChar(char1); }
        const ret = wasm.atermterminal_encode_key(this.__wbg_ptr, ptr0, len0, mods, event_type, char1);
        let v3;
        if (ret[0] !== 0) {
            v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v3;
    }
    /**
     * Encode mouse MOTION at `col`/`row`; `button` is the held button (3 = none).
     * `None` unless the mode reports motion (1002 while a button is down, 1003
     * always) — see [`AtermTerminal::mouse_wants_motion`].
     * @param {number} col
     * @param {number} row
     * @param {number} button
     * @param {number} mods
     * @returns {Uint8Array | undefined}
     */
    encode_mouse_motion(col, row, button, mods) {
        const ret = wasm.atermterminal_encode_mouse_motion(this.__wbg_ptr, col, row, button, mods);
        let v1;
        if (ret[0] !== 0) {
            v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Encode a mouse-button PRESS at 0-based on-screen cell `col`/`row` for the
     * app's active mouse mode+encoding (returns `None`/`undefined` when tracking
     * is off). `button` is the raw X10 button code (0=left,1=middle,2=right) and
     * `mods` is the OR of Shift(4)/Alt(8)/Ctrl(16) masks — the engine combines
     * them. Bytes are sent verbatim to the PTY.
     * @param {number} col
     * @param {number} row
     * @param {number} button
     * @param {number} mods
     * @returns {Uint8Array | undefined}
     */
    encode_mouse_press(col, row, button, mods) {
        const ret = wasm.atermterminal_encode_mouse_press(this.__wbg_ptr, col, row, button, mods);
        let v1;
        if (ret[0] !== 0) {
            v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Encode a mouse-button RELEASE (see [`AtermTerminal::encode_mouse_press`]);
     * `None` in X10 press-only mode.
     * @param {number} col
     * @param {number} row
     * @param {number} button
     * @param {number} mods
     * @returns {Uint8Array | undefined}
     */
    encode_mouse_release(col, row, button, mods) {
        const ret = wasm.atermterminal_encode_mouse_release(this.__wbg_ptr, col, row, button, mods);
        let v1;
        if (ret[0] !== 0) {
            v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Encode a mouse WHEEL tick at `col`/`row` (`up` = wheel-up); the host sends
     * these instead of scrolling scrollback while tracking is on. `None` in X10.
     * @param {number} col
     * @param {number} row
     * @param {boolean} up
     * @param {number} mods
     * @returns {Uint8Array | undefined}
     */
    encode_mouse_wheel(col, row, up, mods) {
        const ret = wasm.atermterminal_encode_mouse_wheel(this.__wbg_ptr, col, row, up, mods);
        let v1;
        if (ret[0] !== 0) {
            v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Last-rendered framebuffer height in pixels.
     * @returns {number}
     */
    get height() {
        const ret = wasm.atermterminal_height(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * True when the alternate screen is active (TUIs own their own scrolling),
     * so the host should let wheel events pass through to the app.
     * @returns {boolean}
     */
    get is_alt_screen() {
        const ret = wasm.atermterminal_is_alt_screen(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True when DEC private mode 1007 (alternate scroll) is set: while the
     * alternate screen is active and mouse tracking is off, the host converts
     * wheel ticks into arrow-key presses (aterm-gui's WheelPlan behaviour) so
     * TUIs without mouse support (less, man, plain vim) still wheel-scroll.
     * @returns {boolean}
     */
    get is_alternate_scroll() {
        const ret = wasm.atermterminal_is_alternate_scroll(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True when DECCKM (application cursor keys) is set: the host must encode
     * arrows/Home/End as SS3 (ESC O A) instead of CSI (ESC [ A) so full-screen
     * apps (vi, less, readline) receive the sequences they expect.
     * @returns {boolean}
     */
    get is_app_cursor_mode() {
        const ret = wasm.atermterminal_is_app_cursor_mode(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True when DEC mode 2031 (color-scheme update notifications) is set: the
     * app wants `CSI ? 997 ; n` on OS light/dark theme changes.
     * @returns {boolean}
     */
    get is_color_scheme_updates_mode() {
        const ret = wasm.atermterminal_is_color_scheme_updates_mode(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * `true` while any effect is animating. Consult
     * [`Self::effects_next_deadline_ms`] first: rain is active at 12/30 Hz and
     * must not drive a 60/120 Hz display-rAF loop.
     * @returns {boolean}
     */
    is_effects_active() {
        const ret = wasm.atermterminal_is_effects_active(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True when DECSET 1004 (focus reporting) is active: the host sends CSI I on
     * focus-in and CSI O on focus-out so apps (vim, tmux) track terminal focus.
     * @returns {boolean}
     */
    get is_focus_event_mode() {
        const ret = wasm.atermterminal_is_focus_event_mode(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True when a TUI has enabled mouse tracking (any of DECSET 9/1000/1002/1003).
     * The host then ENCODES canvas mouse events to the PTY instead of running
     * selection/scroll/link for them (unless Shift is held = user override).
     * @returns {boolean}
     */
    get is_mouse_tracking() {
        const ret = wasm.atermterminal_is_mouse_tracking(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The live `Terminal::keyboard_mode()` as its raw bitflags value, for
     * hosts that run the engine in a Web Worker: mirror these bits into the
     * main-thread engine-state snapshot and feed them to the free
     * [`encode_key_with_mode`], which encodes keydowns synchronously without
     * an instance. `KeyboardMode` is a `bitflags` struct over `u16` (bits
     * 0..=14 defined); the value is zero-extended to `u32` for headroom.
     * @returns {number}
     */
    get keyboard_mode_bits() {
        const ret = wasm.atermterminal_keyboard_mode_bits(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Detect a link under display `row`/`col`. Prefers an OSC-8 hyperlink, then
     * falls back to smart-selection rules (url/file_path). Returns `None` for
     * plain words. `kind`: 0=osc8, 1=url, 2=file_path, 3=other.
     * @param {number} row
     * @param {number} col
     * @returns {LinkHit | undefined}
     */
    link_at(row, col) {
        const ret = wasm.atermterminal_link_at(this.__wbg_ptr, row, col);
        return ret === 0 ? undefined : LinkHit.__wrap(ret);
    }
    /**
     * Whether PHOSPHOR matrix rain is enabled.
     * @returns {boolean}
     */
    get matrix_rain_enabled() {
        const ret = wasm.atermterminal_matrix_rain_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True for AnyEvent (1003): report motion even with NO button pressed.
     * 1002 only reports motion while a button is held; the host uses this to
     * decide whether a button-less `mousemove` should be forwarded.
     * @returns {boolean}
     */
    get mouse_wants_any_motion() {
        const ret = wasm.atermterminal_mouse_wants_any_motion(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * True when the active mouse mode reports MOTION (ButtonEvent 1002 = drag
     * while a button is down, AnyEvent 1003 = all motion), so the host only
     * forwards `mousemove` when an app actually wants it (no spam in 1000).
     * @returns {boolean}
     */
    get mouse_wants_motion() {
        const ret = wasm.atermterminal_mouse_wants_motion(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Build a `rows`x`cols` terminal rendered with `font_bytes` (a TTF/OTF) at
     * `px` cell font-size. `font_bytes` is injected by the host (fetched in JS),
     * keeping the engine free of filesystem font discovery. `fg`/`bg`/`cursor`/
     * `selection` are 0x00RRGGBB and seed the renderer's DEFAULT theme colors;
     * per-cell SGR colors still flow through the grid independently.
     * @param {number} rows
     * @param {number} cols
     * @param {Uint8Array} font_bytes
     * @param {number} px
     * @param {number} fg
     * @param {number} bg
     * @param {number} cursor
     * @param {number} selection
     */
    constructor(rows, cols, font_bytes, px, fg, bg, cursor, selection) {
        const ptr0 = passArray8ToWasm0(font_bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_new(rows, cols, ptr0, len0, px, fg, bg, cursor, selection);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0] >>> 0;
        AtermTerminalFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * [`AtermTerminal::new`] from a registered PRIMARY font handle.
     * @param {number} rows
     * @param {number} cols
     * @param {number} font_handle
     * @param {number} px
     * @param {number} fg
     * @param {number} bg
     * @param {number} cursor
     * @param {number} selection
     * @returns {AtermTerminal}
     */
    static new_registered(rows, cols, font_handle, px, fg, bg, cursor, selection) {
        const ret = wasm.atermterminal_new_registered(rows, cols, font_handle, px, fg, bg, cursor, selection);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return AtermTerminal.__wrap(ret[0]);
    }
    /**
     * Register one keystroke for the cursor-comet ignition: sustained fast
     * calls heat the typing cadence so the next `render` ignites the trail,
     * sparse/slow calls keep it gentle. The cadence reads the effects clock,
     * so the host must `advance_effects` between keystrokes for it to reflect
     * real time. Call this from the SAME JS keydown handler that feeds
     * `encode_key`; without it the comet stays dormant on web hosts. It also
     * freezes literal-rain sampling while a draft is unsent; on submit call
     * `note_matrix_rain_signal(10, 4)` after this method.
     */
    note_keystroke() {
        wasm.atermterminal_note_keystroke(this.__wbg_ptr);
    }
    /**
     * Feed wheel/PgUp activity from an alternate-screen TUI so rain pauses
     * while the user reads its transcript.
     */
    note_matrix_rain_alt_scroll() {
        wasm.atermterminal_note_matrix_rain_alt_scroll(this.__wbg_ptr);
    }
    /**
     * Feed a terminal visual bell into PHOSPHOR's bounded alert tint.
     */
    note_matrix_rain_bell() {
        wasm.atermterminal_note_matrix_rain_bell(this.__wbg_ptr);
    }
    /**
     * Payload-free observable-work pulse. Codes are `0 assistant, 1 inspect,
     * 2 modify, 3 execute, 4 network, 5 branch, 6 waiting, 7 success,
     * 8 failure, 9 interrupted, 10 turn-start`; weight clamps to `1..=8`.
     * Turn-start also releases the unsent-composer material gate.
     * @param {number} code
     * @param {number} weight
     */
    note_matrix_rain_signal(code, weight) {
        wasm.atermterminal_note_matrix_rain_signal(this.__wbg_ptr, code, weight);
    }
    /**
     * Register a Backspace: cancels our OWN trailing guess only (erasing
     * already-committed real content is left to the program's echo). Returns
     * whether state changed.
     * @returns {boolean}
     */
    predict_backspace() {
        const ret = wasm.atermterminal_predict_backspace(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Register a printable character the host just wrote to the PTY (the
     * keydown seam — call beside `encode_key`). The guess anchors at the
     * engine's live cursor, extends pending type-ahead, and never crosses the
     * right margin. Returns whether a guess is now TRACKED — display is a
     * separate gate (see [`predict_overlay`](Self::predict_overlay)).
     * @param {string} ch
     * @returns {boolean}
     */
    predict_char(ch) {
        const char0 = ch.codePointAt(0);
        _assertChar(char0);
        const ret = wasm.atermterminal_predict_char(this.__wbg_ptr, char0);
        return ret !== 0;
    }
    /**
     * Register a plain Enter (the SUBMIT boundary — call when the host writes
     * the line terminator to the PTY). Ends the confirmation epoch: the NEXT
     * line must re-confirm an echo before `adaptive` displays anything.
     * LOAD-BEARING for password safety on a terminal scrolled to the bottom,
     * where the cursor REUSES one physical row across logical lines: without
     * it, a non-echoing password prompt landing on the same row as a just-
     * confirmed command would inherit that confirmation and flash the secret
     * (the native `note_line_submit` seam). Cheap no-op when nothing pends.
     */
    predict_line_submit() {
        wasm.atermterminal_predict_line_submit(this.__wbg_ptr);
    }
    /**
     * Milliseconds until the oldest pending guess self-expires (the glitch
     * flush), or `undefined` when none is pending. Arm ONE timer for this and
     * call [`predict_overlay`](Self::predict_overlay) + repaint there, so a
     * stale ghost is erased even when no further input or output arrives.
     * @returns {number | undefined}
     */
    predict_next_deadline_ms() {
        const ret = wasm.atermterminal_predict_next_deadline_ms(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * The ghost cells to paint THIS frame, as flat `[row, col, codepoint]`
     * triples (a `Uint32Array` in JS). The host renders them tentatively
     * (dim/underline) and may advance its DRAWN cursor past the last one,
     * mosh-style. Runs the expiry self-heal first, then the display gate:
     * `always` ⇒ all pending; `adaptive` ⇒ all pending after an echo is confirmed
     * on this line and measured RTT is high enough to help. Empty in app-owned
     * Kitty composers and while scrolled into history.
     * @returns {Uint32Array}
     */
    predict_overlay() {
        const ret = wasm.atermterminal_predict_overlay(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Reconcile pending guesses against the grid — call after `process()`
     * applies a PTY chunk. Confirmed leading guesses retire (arming the
     * epoch's display gate), any divergence flushes the set, and a no-echo
     * context refuses prediction outright — the alternate screen (vim/less/
     * htop) OR an app-owned Kitty composer (REPORT_EVENT_TYPES /
     * REPORT_ALL_KEYS_AS_ESC). While scrolled into history only the expiry
     * self-heal runs: guesses live in ACTIVE-grid coords, so the scrollback
     * view is never reconciled against them (the native discipline).
     */
    predict_reconcile() {
        wasm.atermterminal_predict_reconcile(this.__wbg_ptr);
    }
    /**
     * Drop all in-flight guesses because this SAME terminal's coordinate space
     * changed (`resize` calls this automatically). The confirmation epoch is
     * forgotten, while this session's learned link RTT remains useful.
     */
    predict_reset() {
        wasm.atermterminal_predict_reset(this.__wbg_ptr);
    }
    /**
     * Reset for a DIFFERENT pane/session. In addition to coordinate-bound
     * guesses, forget the learned echo RTT so a slow remote pane cannot make a
     * newly selected local pane display speculation. Hosts that keep one
     * `AtermTerminal` per session never need this; pane-reusing hosts call it at
     * the identity switch.
     */
    predict_session_reset() {
        wasm.atermterminal_predict_session_reset(this.__wbg_ptr);
    }
    /**
     * Feed raw PTY output bytes into the engine.
     * @param {Uint8Array} bytes
     */
    process(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_process(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Feed PTY output as a JS string. wasm-bindgen encodes it (UTF-8, via
     * `encodeInto`) straight into wasm memory, so the host avoids a separate
     * JS-side `TextEncoder.encode` allocation + copy on the hot output path.
     * Byte-identical to `process(new TextEncoder().encode(s))`.
     * @param {string} s
     */
    process_str(s) {
        const ptr0 = passStringToWasm0(s, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_process_str(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Advance a deferred width-change scrollback rewrap (stashed by
     * [`Self::resize`]) by ONE BOUNDED step — at most the configured budget
     * of history lines ([`Self::pump_reflow_budget`], default
     * `REFLOW_STEP_BUDGET_LINES`) — re-attaching the rewrapped history when
     * the step completes the job. Returns `true` while work REMAINS (the
     * host should schedule another pump — a `setTimeout(0)` chain or
     * `requestIdleCallback`); `false` once nothing is pending (the job just
     * completed and re-attached — re-attach marks full damage, so the next
     * `render` repaints — or there was nothing to do).
     *
     * COST: O(budget × cols) per call (`PendingScrollbackReflow::reflow_step`;
     * a logical line is never split, so a soft-wrapped run longer than the
     * budget is rewrapped whole by the step that completes it). Any pump
     * schedule yields history content IDENTICAL to a one-shot rewrap —
     * aterm-grid's `reflow_step_any_schedule_matches_one_shot` property.
     *
     * NEVER-PUMPED SAFETY: a host that never calls this still completes the
     * rewrap — `render` pumps one step per frame once
     * `REFLOW_PUMP_GRACE_RENDERS` frames have passed, `process` pumps one
     * step per call while the detach-window backlog exceeds
     * `REFLOW_BACKLOG_MAX_LINES` — and a torn-down module drops the job WITH
     * the engine. There is no host behavior that leaves the store detached
     * while the module keeps operating unboundedly.
     * @returns {boolean}
     */
    pump_reflow() {
        const ret = wasm.atermterminal_pump_reflow(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Tune the per-pump rewrap budget (INPUT history lines per
     * [`Self::pump_reflow`] step). `0` restores the default
     * (`REFLOW_STEP_BUDGET_LINES`, 2_000 ≈ ~3ms native — see the constant's
     * sizing note). Hosts with generous idle windows can raise it to finish
     * deep histories in fewer tasks; latency-sensitive hosts can lower it.
     * @param {number} max_lines
     */
    pump_reflow_budget(max_lines) {
        wasm.atermterminal_pump_reflow_budget(this.__wbg_ptr, max_lines);
    }
    /**
     * True while a deferred scrollback rewrap is stashed (deep history is
     * temporarily detached: only the ring is visible/searchable; a partly
     * stepped job holds its progress here between pumps). The host should
     * keep scheduling [`Self::pump_reflow`] while this is set.
     * @returns {boolean}
     */
    get reflow_pending() {
        const ret = wasm.atermterminal_reflow_pending(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Rasterize the current grid into the internal RGBA8 framebuffer via the
     * damage-tracked path: only rows that changed since the last frame are
     * re-rendered (the rest reuse the persistent cache), so streaming output and
     * single-keystroke edits don't re-rasterize the whole grid every frame.
     */
    render() {
        wasm.atermterminal_render(this.__wbg_ptr);
    }
    /**
     * Resize the grid (after the host recomputes cols/rows for the canvas).
     *
     * The visible grid and the bounded in-memory ring resize SYNCHRONOUSLY
     * (O(viewport + ring)). A width change with a deep tiered history does
     * NOT rewrap that history here: it is detached in O(1)
     * (`resize_offloading_scrollback`, the same audited boundary the native
     * app uses) and rewrapped in LATER, budget-bounded host tasks — see
     * [`Self::pump_reflow`].
     * Small histories (≤ `INLINE_REFLOW_MAX_LINES`) rewrap inline: bounded,
     * imperceptible, mirroring the native inline bound. This keeps the
     * synchronous cost of a resize independent of session history — the
     * browser-tab analog of the native L0 whole-Mac-freeze fix, on a loop
     * with no worker thread to offload to.
     * @param {number} rows
     * @param {number} cols
     */
    resize(rows, cols) {
        wasm.atermterminal_resize(this.__wbg_ptr, rows, cols);
    }
    /**
     * Revoke OSC 52 clipboard *write* authorization (the user toggled the
     * clipboard setting off). Returns the engine to its fail-closed default.
     */
    revoke_clipboard_write() {
        wasm.atermterminal_revoke_clipboard_write(this.__wbg_ptr);
    }
    /**
     * Copy of the last-rendered RGBA8 framebuffer (`width*height*4` bytes),
     * ready for `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)`.
     * @returns {Uint8Array}
     */
    rgba() {
        const ret = wasm.atermterminal_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Byte offset of the last-rendered RGBA8 framebuffer within wasm linear
     * memory, for a ZERO-COPY `putImageData` from JS (no copy out of wasm, unlike
     * [`rgba`]). The host builds `new Uint8ClampedArray(memory.buffer, ptr,
     * width*height*4)` and must read it synchronously right after `render()` and
     * before any other engine call — the next `render`/`process` may reallocate
     * `self.rgba`, and any wasm memory growth detaches the JS view.
     * @returns {number}
     */
    rgba_ptr() {
        const ret = wasm.atermterminal_rgba_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Soft-wrap flag for a visible `row`: `true` if it continues the previous
     * row (autowrap), `undefined`/`None` when out of range.
     * @param {number} row
     * @returns {boolean | undefined}
     */
    row_is_wrapped(row) {
        const ret = wasm.atermterminal_row_is_wrapped(this.__wbg_ptr, row);
        return ret === 0xFFFFFF ? undefined : ret !== 0;
    }
    /**
     * Logical length of a visible `row` (last non-empty cell + 1, 0 if blank);
     * `None` when out of range.
     * @param {number} row
     * @returns {number | undefined}
     */
    row_len(row) {
        const ret = wasm.atermterminal_row_len(this.__wbg_ptr, row);
        return ret === 0xFFFFFF ? undefined : ret;
    }
    /**
     * Scroll-correct text of a display `row` (display_offset-aware), for a TS
     * fallback that re-runs link matching in JS.
     * @param {number} row
     * @returns {string | undefined}
     */
    row_text(row) {
        const ret = wasm.atermterminal_row_text(this.__wbg_ptr, row);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * The SIGNED device-px band shift the next `render()` presents for the
     * banked residual (negative = band shifted DOWN, toward older). Exposed
     * so hosts/harnesses can assert the CPU and GPU bundles present the same
     * sub-row frame.
     * @returns {number}
     */
    get scroll_frac_px() {
        const ret = wasm.atermterminal_scroll_frac_px(this.__wbg_ptr);
        return ret;
    }
    /**
     * The banked sub-row residual in ROWS — signed, in `(-1.0, 1.0)`,
     * positive = partway toward OLDER lines. `0` whenever the viewport is
     * row-aligned (after a flip, a whole-row navigation, or at a clamped
     * history end).
     * @returns {number}
     */
    get scroll_frac_rows() {
        const ret = wasm.atermterminal_scroll_frac_rows(this.__wbg_ptr);
        return ret;
    }
    /**
     * Scroll the viewport through scrollback: positive `delta` reveals older
     * lines, negative reveals newer. `render` already honors the display offset,
     * so the host only needs to redraw afterwards.
     * @param {number} delta
     */
    scroll_lines(delta) {
        wasm.atermterminal_scroll_lines(this.__wbg_ptr, delta);
    }
    /**
     * Sub-row scroll input in fractional LINES (`deltaMode ==
     * DOM_DELTA_LINE` hosts, or a host that scales pixels itself). Same
     * accumulation contract as [`scroll_px`](Self::scroll_px): whole rows
     * flip at ±1.0 accumulated, the remainder banks.
     * @param {number} delta_rows
     */
    scroll_lines_frac(delta_rows) {
        wasm.atermterminal_scroll_lines_frac(this.__wbg_ptr, delta_rows);
    }
    /**
     * Sub-row scroll input in device PIXELS — the wheel/trackpad `deltaY` at
     * `deltaMode == DOM_DELTA_PIXEL`, sign-adjusted by the host so POSITIVE
     * reveals older lines (the [`scroll_lines`](Self::scroll_lines)
     * convention). Fractions accumulate across calls; each whole
     * `cell_height` of accumulation flips one engine row, and the sub-row
     * remainder is presented by the next `render()` as a pixel shift of the
     * grid band — the host only needs to redraw afterwards.
     * @param {number} delta_px
     */
    scroll_px(delta_px) {
        wasm.atermterminal_scroll_px(this.__wbg_ptr, delta_px);
    }
    /**
     * Scroll the viewport so the match at absolute `line` is visible, placing it
     * at (or near) the top row. Clamps the target display_offset to the retained
     * scrollback so a live-region match snaps to the bottom. Host redraws after.
     * @param {number} line
     */
    scroll_search_line_into_view(line) {
        wasm.atermterminal_scroll_search_line_into_view(this.__wbg_ptr, line);
    }
    /**
     * Snap the viewport to the live bottom (latest output).
     */
    scroll_to_bottom() {
        wasm.atermterminal_scroll_to_bottom(this.__wbg_ptr);
    }
    /**
     * Snap the viewport to the oldest retained scrollback line.
     */
    scroll_to_top() {
        wasm.atermterminal_scroll_to_top(this.__wbg_ptr);
    }
    /**
     * Search the full retained buffer (scrollback + visible) for `query`,
     * returning matches as a flat `[abs_line, start_col, len]` triplet array so
     * the JS host can highlight + scroll without re-scanning text. Lines are
     * ABSOLUTE rows (the index's native coordinate); the host maps them to
     * display rows via [`AtermTerminal::search_display_origin`] /
     * [`AtermTerminal::scroll_search_line_into_view`], which stay correct as the
     * viewport scrolls. Empty `query` (or a regex error) yields an empty array.
     *
     * One-shot: pays the whole index build in this call and DROPS the
     * engine's incomplete-results signal. Prefer
     * [`AtermTerminal::search_budgeted`], which slices the work across calls
     * and reports `incomplete_index`.
     * @param {string} query
     * @param {boolean} case_sensitive
     * @param {boolean} is_regex
     * @returns {Uint32Array}
     */
    search(query, case_sensitive, is_regex) {
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_search(this.__wbg_ptr, ptr0, len0, case_sensitive, is_regex);
        var v2 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
    /**
     * Budgeted, resumable variant of [`AtermTerminal::search`] (P1.1): runs at
     * most `row_budget` rows of index-build + verification per call and
     * returns a [`BudgetedSearchResult`] with a cursor to continue, so the
     * host can slice a deep-scrollback search across event-loop turns and
     * CANCEL a superseded query mid-search (drop the cursor; the next call
     * with a different pattern supersedes the in-flight state).
     *
     * Pass `resume_cursor: None` (or a stale value) to start; pass
     * each step's `cursor` back to continue. A cursor is only valid for the
     * same engine instance, pattern/options, and unchanged content — any
     * mismatch restarts from scratch (fresh cursor, progress reset), never
     * stale results. CPU/GPU wasm modules are separate cursor domains; keep a
     * token with the engine that issued it. On the
     * Each response contains a stable match DELTA (at most 4,096 records).
     * When `reset` is true (or `search_id` changes), clear prior deltas before
     * appending this step; this makes even a one-turn stale-content restart
     * unambiguous after the resume cursor disappears. When `complete == true`,
     * the deltas for that `search_id` equal a one-shot [`AtermTerminal::search`].
     * Unlike that legacy API,
     * `incomplete_index` reports eviction or match-cap truncation and
     * `lowest_retained_line` identifies the searchable suffix. Empty query or
     * invalid regex: an immediate empty `complete` result (matching the legacy
     * API's silence on half-typed regexes). `row_budget == 0` is clamped to one
     * row so a scanning turn always progresses; a turn may instead drain a
     * bounded delta backlog without scanning another row.
     * @param {string} query
     * @param {boolean} case_sensitive
     * @param {boolean} is_regex
     * @param {bigint | null | undefined} resume_cursor
     * @param {number} row_budget
     * @returns {BudgetedSearchResult}
     */
    search_budgeted(query, case_sensitive, is_regex, resume_cursor, row_budget) {
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_search_budgeted(this.__wbg_ptr, ptr0, len0, case_sensitive, is_regex, !isLikeNone(resume_cursor), isLikeNone(resume_cursor) ? BigInt(0) : resume_cursor, row_budget);
        return BudgetedSearchResult.__wrap(ret);
    }
    /**
     * Drop any in-flight [`AtermTerminal::search_budgeted`] state (frees the
     * partial index; outstanding cursors go stale and restart if resumed).
     * Call when the find UI closes or the query is abandoned between slices.
     */
    search_budgeted_cancel() {
        wasm.atermterminal_search_budgeted_cancel(this.__wbg_ptr);
    }
    /**
     * Absolute row of display row 0 at the live bottom (`display_offset == 0`):
     * `oldest_absolute_row + scrollback_lines`. A match at absolute `line` is at
     * display row `line - origin + display_offset`, so the host computes the
     * on-screen cell of any [`AtermTerminal::search`] match without a round-trip.
     * @returns {number}
     */
    get search_display_origin() {
        const ret = wasm.atermterminal_search_display_origin(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Metadata for a [`AtermTerminal::search`]-contract query — most
     * importantly the engine's `incomplete` signal, which that legacy export
     * has always DROPPED (E9a, correctness-first): when index eviction or the
     * engine's match cap truncated the results, the host has been presenting
     * a truncated match list/count as if it were exhaustive.
     *
     * Stateless on purpose: it re-runs `query` against the SAME cached
     * full-content index `search` uses (O(1) index reuse on unchanged
     * content, so the added cost is one query, never a rebuild) and reports
     * on exactly the results that call would return — no staleness if the
     * host asks without (or long after) a paired `search`. Empty query or
     * invalid regex: `incomplete == false`, `match_count == 0`, mirroring
     * the legacy export's empty array.
     * @param {string} query
     * @param {boolean} case_sensitive
     * @param {boolean} is_regex
     * @returns {SearchMeta}
     */
    search_meta(query, case_sensitive, is_regex) {
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_search_meta(this.__wbg_ptr, ptr0, len0, case_sensitive, is_regex);
        return SearchMeta.__wrap(ret);
    }
    /**
     * Drop the current selection so the highlight clears on the next render.
     */
    selection_clear() {
        wasm.atermterminal_selection_clear(this.__wbg_ptr);
    }
    /**
     * Move the selection endpoint to `row`/`col` (during a drag).
     * @param {number} row
     * @param {number} col
     */
    selection_extend(row, col) {
        wasm.atermterminal_selection_extend(this.__wbg_ptr, row, col);
    }
    /**
     * Finalize the selection (mouse released).
     */
    selection_finish() {
        wasm.atermterminal_selection_finish(this.__wbg_ptr);
    }
    /**
     * Select the whole line at display `row` (triple-click) and return its text.
     * Mirrors aterm-gui's select_line: a Lines selection expanded to the full row
     * width. `col` is accepted for a uniform host API but unused (whole row).
     * @param {number} row
     * @param {number} col
     * @returns {string | undefined}
     */
    selection_line(row, col) {
        const ret = wasm.atermterminal_selection_line(this.__wbg_ptr, row, col);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Current selection bounds in DISPLAY viewport cell coords (0 = top visible
     * row), side-adjusted to match `selection_text` and the painted highlight.
     * `None` when there is no selection OR it lies fully outside the viewport.
     * @returns {SelectionRange | undefined}
     */
    selection_range() {
        const ret = wasm.atermterminal_selection_range(this.__wbg_ptr);
        return ret === 0 ? undefined : SelectionRange.__wrap(ret);
    }
    /**
     * Begin a character selection at display `row`/`col` (clears any prior one).
     * @param {number} row
     * @param {number} col
     */
    selection_start(row, col) {
        wasm.atermterminal_selection_start(this.__wbg_ptr, row, col);
    }
    /**
     * The selected text, if any (`None` when the selection is empty).
     * @returns {string | undefined}
     */
    selection_text() {
        const ret = wasm.atermterminal_selection_text(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Select the whole word/URL at display `row`/`col` (double-click) and return
     * its text. Mirrors aterm-gui's select_word: a Semantic selection EXPANDED to
     * the word's inclusive cell span (smart_word_at's end col is exclusive); on
     * whitespace it falls back to the clicked cell. The selection stays active so
     * the highlight paints.
     * @param {number} row
     * @param {number} col
     * @returns {string | undefined}
     */
    selection_word(row, col) {
        const ret = wasm.atermterminal_selection_word(this.__wbg_ptr, row, col);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Serialize the terminal to a REPLAYABLE ANSI string — the aterm-native
     * replacement for `@xterm/addon-serialize`'s `serialize({scrollback})`, so the
     * renderer no longer needs a shadow xterm.js buffer to snapshot/restore/fork a
     * pane. Layout: SGR reset, then the capped recent history (text + CRLF), then
     * `CSI H`, then each visible row placed with absolute CUP + erase-line (so a
     * full-width row can't autowrap on replay) emitted via the engine's
     * `row_ansi_text_screen` (minimal change-based SGR, wide-char aware), then the cursor
     * restored. `scrollback_rows` = `None` prepends ALL history, `Some(n)` the last
     * `n`, `Some(0)` viewport-only. Ported from the daemon's proven `serialize_ansi`
     * (orca-terminal headless) so the output stays byte-compatible with the existing
     * string-based replay pipeline.
     * @param {number | null} [scrollback_rows]
     * @returns {string}
     */
    serialize(scrollback_rows) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.atermterminal_serialize(this.__wbg_ptr, isLikeNone(scrollback_rows) ? 0x100000001 : (scrollback_rows) >>> 0);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Scrollback HISTORY ONLY (the off-screen lines above the viewport) as flowing
     * text + CRLF, no cursor/grid framing. Reads the MAIN buffer's scrollback (aterm
     * keeps it in the inactive grid while the alt screen is active) so an in-alt
     * (vim/htop/less) snapshot still recovers the pre-TUI history — the only
     * recoverable history on cold-restore of an alt-screen session. `max_rows` caps
     * to the last `n` lines (`None` = all). Mirrors the daemon's serialize_scrollback_ansi.
     * @param {number | null} [max_rows]
     * @returns {string}
     */
    serialize_scrollback(max_rows) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.atermterminal_serialize_scrollback(this.__wbg_ptr, isLikeNone(max_rows) ? 0x100000001 : (max_rows) >>> 0);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Set the DEFAULT-background opacity (0..=1; Ghostty's
     * `background-opacity`). `1.0` (the default) keeps output byte-identical.
     * Below 1.0, pixels whose bg resolved to the frame's DEFAULT background
     * come out of [`rgba`](Self::rgba)/[`rgba_ptr`](Self::rgba_ptr) with
     * `alpha = round(opacity*255)`, so `putImageData` onto a (transparent)
     * canvas lets the page show through. SGR-colored bg cells, the selection
     * band and glyph pixels stay opaque so text keeps its contrast.
     * Appearance-only, so force one full repaint next frame.
     * @param {number} opacity
     */
    set_background_opacity(opacity) {
        wasm.atermterminal_set_background_opacity(this.__wbg_ptr, opacity);
    }
    /**
     * Inject a REAL bold weight of the primary family so SGR-bold cells render as a
     * true heavier weight instead of synthetic embolden. The host supplies the
     * bold-variant bytes (the canvas can't read the filesystem). No-throw: a bad
     * blob surfaces a catchable JS exception and leaves the existing weight intact.
     * @param {Uint8Array} bytes
     */
    set_bold_font(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_set_bold_font(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * [`AtermTerminal::set_bold_font`] from a registered handle.
     * @param {number} handle
     */
    set_bold_font_registered(handle) {
        const ret = wasm.atermterminal_set_bold_font_registered(this.__wbg_ptr, handle);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Tell the engine the real device-pixel cell size so its CSI 14t/16t
     * window/cell-pixel reports are accurate (the engine has no canvas otherwise).
     * @param {number} width
     * @param {number} height
     */
    set_cell_pixel_size(width, height) {
        wasm.atermterminal_set_cell_pixel_size(this.__wbg_ptr, width, height);
    }
    /**
     * Window-chrome for WINDOW-SPACE effects in an embedder: interior padding
     * (`pad`, px per edge) plus a top-only rise band (`head`, px) around the
     * grid — the `[head][pad][grid][pad]` frame aterm-render composes. The
     * framebuffer grows accordingly (`width`/`height` report the padded frame;
     * the host re-reads them and offsets its canvas by `-pad,-(pad+head)` so
     * the grid stays put) and effect emissions (glow, trail, fire) become
     * window-absolute, escaping the grid into the chrome instead of clipping
     * at the cell edge. `0/0` (the default) is byte-identical to the
     * historical exact-fit frame.
     * @param {number} pad
     * @param {number} head
     */
    set_chrome(pad, head) {
        wasm.atermterminal_set_chrome(this.__wbg_ptr, pad, head);
    }
    /**
     * Push the host OS color scheme into the engine. `dark = true` selects a dark
     * appearance, `false` light. When the scheme CHANGES and the app enabled DEC mode
     * 2031, the engine queues an unsolicited `CSI ? 997 ; Ps n` (1=dark, 2=light);
     * drain it via `take_response` and forward to the PTY so subscribed apps live-
     * update their theme. A no-op when the scheme is unchanged.
     * @param {boolean} dark
     */
    set_color_scheme(dark) {
        wasm.atermterminal_set_color_scheme(this.__wbg_ptr, dark);
    }
    /**
     * Set the cursor blink phase: `true` draws the cursor this frame, `false`
     * hides it. The host drives a ~530ms blink timer; independent of DECSCUSR.
     * @param {boolean} on
     */
    set_cursor_blink_phase(on) {
        wasm.atermterminal_set_cursor_blink_phase(this.__wbg_ptr, on);
    }
    /**
     * Configure the LUMEN cursor aurora (additive light in the cursor's
     * wake). Mirrors the native knobs + clamps: `style` ∈
     * `lumen|phaser|nyan|sparkle|fire|laser|beam|water|comet` (unknown →
     * lumen; `rainbow` = the Nyan banded ribbon);
     * `color`/`accent` omitted derive from the theme cursor (accent = color
     * brightened 1.5×) exactly like the native app; `duration_ms` clamps
     * 30..=2000, `length` (cells) 1..=512, `intensity` 0..=1 (0 = off),
     * `radius` (bloom crown, cells) 0..=2, `ring` = landing-ring ping.
     * @param {boolean} enabled
     * @param {string} style
     * @param {number | null | undefined} color
     * @param {number | null | undefined} accent
     * @param {number} duration_ms
     * @param {number} length
     * @param {number} intensity
     * @param {number} radius
     * @param {boolean} ring
     */
    set_cursor_glow(enabled, style, color, accent, duration_ms, length, intensity, radius, ring) {
        const ptr0 = passStringToWasm0(style, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_cursor_glow(this.__wbg_ptr, enabled, ptr0, len0, isLikeNone(color) ? 0x100000001 : (color) >>> 0, isLikeNone(accent) ? 0x100000001 : (accent) >>> 0, duration_ms, length, intensity, radius, ring);
    }
    /**
     * Force a hollow (unfocused) cursor when `true`, or restore the terminal's
     * DECSCUSR style when `false` — the standard focused/unfocused affordance.
     * @param {boolean} hollow
     */
    set_cursor_hollow(hollow) {
        wasm.atermterminal_set_cursor_hollow(this.__wbg_ptr, hollow);
    }
    /**
     * Set the CURSOR-fill opacity (0..=1; Ghostty's `cursor-opacity`). `1.0`
     * (the default) keeps the opaque fill + block-cursor glyph cut-out
     * byte-identical. Below 1.0 the cursor fill blends over the cell so the
     * glyph shows through. Appearance-only, so force one full repaint.
     * @param {number} opacity
     */
    set_cursor_opacity(opacity) {
        wasm.atermterminal_set_cursor_opacity(this.__wbg_ptr, opacity);
    }
    /**
     * Configure the legacy opaque comet trail (the native `cursor_trail_style
     * = "comet"` look). `color` omitted = the theme cursor; `duration_ms`
     * clamps 30..=2000, `length` 1..=512. Exactly one of trail/glow is on in
     * the native app (chosen by style); the embedder decides here.
     * @param {boolean} enabled
     * @param {number} duration_ms
     * @param {number} length
     * @param {number | null} [color]
     */
    set_cursor_trail(enabled, duration_ms, length, color) {
        wasm.atermterminal_set_cursor_trail(this.__wbg_ptr, enabled, duration_ms, length, isLikeNone(color) ? 0x100000001 : (color) >>> 0);
    }
    /**
     * Arm (or clear) a **Trail Pack** — user-generated cursor trails as data.
     * Pass the pack's TOML source (`trail_pack::compile_trail_pack_toml`);
     * `undefined` clears any live pack. On a compile ERROR the prior pack is
     * LEFT INTACT and the joined diagnostics are RETURNED (never silently
     * dropped — the `set_sparkle_custom_specs` gap this closes); `Ok` returns
     * `undefined`.
     * @param {string | null} [toml]
     * @returns {string | undefined}
     */
    set_cursor_trail_pack(toml) {
        var ptr0 = isLikeNone(toml) ? 0 : passStringToWasm0(toml, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_set_cursor_trail_pack(this.__wbg_ptr, ptr0, len0);
        let v2;
        if (ret[0] !== 0) {
            v2 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v2;
    }
    /**
     * @param {number} r
     * @param {number} g
     * @param {number} b
     */
    set_default_background(r, g, b) {
        wasm.atermterminal_set_default_background(this.__wbg_ptr, r, g, b);
    }
    /**
     * Set the host-preferred DEFAULT cursor style (shape used before any DECSCUSR and
     * restored after RIS/DECSTR). `n` follows the DECSCUSR convention: 1=blinking
     * block, 2=steady block, 3=blinking underline, 4=steady underline, 5=blinking bar,
     * 6=steady bar; out-of-range (0, 7+) is ignored. Unlike a render override this does
     * NOT clobber an app's live DECSCUSR (e.g. vim insert-mode bar).
     * @param {number} n
     */
    set_default_cursor_style(n) {
        wasm.atermterminal_set_default_cursor_style(this.__wbg_ptr, n);
    }
    /**
     * Seed the engine's DEFAULT foreground/background so its OSC 10/11 colour-query
     * replies report the host theme (the engine otherwise reports its built-in
     * defaults). RGB components, 0–255.
     * @param {number} r
     * @param {number} g
     * @param {number} b
     */
    set_default_foreground(r, g, b) {
        wasm.atermterminal_set_default_foreground(this.__wbg_ptr, r, g, b);
    }
    /**
     * Focus gate for the idle one-shots (`§5.6`): an unfocused pane fires no
     * blink events (and freezes their fingerprints). Pass the pane focus.
     * @param {boolean} focused
     */
    set_effects_focused(focused) {
        wasm.atermterminal_set_effects_focused(this.__wbg_ptr, focused);
    }
    /**
     * Tri-state pane visibility for bounded rain draining:
     * `focused|visible_unfocused|hidden`.
     * @param {string} state
     */
    set_effects_visibility(state) {
        const ptr0 = passStringToWasm0(state, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_effects_visibility(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Inject a colour-emoji (sbix) face from font bytes, driving the existing
     * ColorEmoji colour path. Same rationale as [`set_fallback_font`]: the host
     * supplies the OS emoji font. No-throw (the `String` Err surfaces as a
     * catchable JS exception); a bad blob leaves the slot untouched.
     * @param {Uint8Array} bytes
     */
    set_emoji_font(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_set_emoji_font(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * [`AtermTerminal::set_emoji_font`] from a registered handle. Installs the
     * SHARED interned copy (no `to_vec` of the ~190MB emoji face per pane).
     * @param {number} handle
     */
    set_emoji_font_registered(handle) {
        const ret = wasm.atermterminal_set_emoji_font_registered(this.__wbg_ptr, handle);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Inject a broad-coverage (CJK + symbols) fallback face from font bytes, so
     * glyphs the primary face lacks render real shapes instead of `.notdef` tofu.
     * The canvas renderer can't read the host filesystem, so the host pushes the
     * OS font bytes in. No-throw: a bad blob leaves the existing face untouched.
     * @param {Uint8Array} bytes
     */
    set_fallback_font(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_set_fallback_font(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * [`AtermTerminal::set_fallback_font`] from a registered handle.
     * @param {number} handle
     */
    set_fallback_font_registered(handle) {
        const ret = wasm.atermterminal_set_fallback_font_registered(this.__wbg_ptr, handle);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * OpenType FONT FEATURES for the primary face, as a space-separated spec
     * (`"+ss01 zero -calt"` — bare/`+tag` enables, `-tag` disables, `tag=N` sets a
     * value). Mirrors the native `font_features` config knob. An empty/blank spec
     * clears all features. Preserves the current ligature mode; forces a repaint.
     * @param {string} spec
     */
    set_font_features(spec) {
        const ptr0 = passStringToWasm0(spec, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_font_features(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Enable/disable the Kitty keyboard protocol capability (default ON). When
     * disabled the engine acts as if the protocol is unsupported — no `CSI ? u`
     * reply, push/set/pop consumed-and-ignored, `keyboard_mode` never carries
     * kitty bits — for hosts whose platform consumes kitty sequences itself
     * (Windows ConPTY; xterm.js `vtExtensions.kittyKeyboard = false`).
     * @param {boolean} enabled
     */
    set_kitty_keyboard_enabled(enabled) {
        wasm.atermterminal_set_kitty_keyboard_enabled(this.__wbg_ptr, enabled);
    }
    /**
     * Programming LIGATURES on/off (`=>`, `!=`, `===` …). Mirrors the native
     * `ligatures` config knob so the in-page renderer honours the host's typography
     * setting instead of being pinned to the constructor default. Preserves any
     * configured `font_features`. Forces a full repaint so the change shows at once.
     * @param {boolean} on
     */
    set_ligatures(on) {
        wasm.atermterminal_set_ligatures(this.__wbg_ptr, on);
    }
    /**
     * Scale the cell BOX height (the host's `terminalLineHeight`) WITHOUT changing
     * the glyph px, so rows space out while text keeps its size. The host re-reads
     * cell_height + recomputes the grid after.
     * @param {number} scale
     */
    set_line_height(scale) {
        wasm.atermterminal_set_line_height(this.__wbg_ptr, scale);
    }
    /**
     * Configure PHOSPHOR using the native bounds. `hue` is
     * `matrix|theme|custom`; `hue_color` is used only for `custom`.
     * `output_material` opts into supported literal screen codepoints; hosts
     * that cannot protect their current composer can leave it false.
     * @param {number} fps
     * @param {number} density
     * @param {number} speed
     * @param {number} trail
     * @param {number | null | undefined} alpha
     * @param {number | null | undefined} head_alpha
     * @param {string} hue
     * @param {number | null | undefined} hue_color
     * @param {number} mutation_ms
     * @param {number} idle_secs
     * @param {boolean} suppress_in_alt_screen
     * @param {boolean} turn_wave
     * @param {boolean} bell_alert
     * @param {boolean} output_material
     * @param {bigint} seed
     */
    set_matrix_rain(fps, density, speed, trail, alpha, head_alpha, hue, hue_color, mutation_ms, idle_secs, suppress_in_alt_screen, turn_wave, bell_alert, output_material, seed) {
        const ptr0 = passStringToWasm0(hue, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_matrix_rain(this.__wbg_ptr, fps, density, speed, trail, isLikeNone(alpha) ? 0x100000001 : (alpha) >>> 0, isLikeNone(head_alpha) ? 0x100000001 : (head_alpha) >>> 0, ptr0, len0, isLikeNone(hue_color) ? 0x100000001 : (hue_color) >>> 0, mutation_ms, idle_secs, suppress_in_alt_screen, turn_wave, bell_alert, output_material, seed);
    }
    /**
     * Enable PHOSPHOR matrix rain. With output material opted in, the shared
     * pipeline samples supported literal codepoints outside the current
     * cursor/composer protection band and emits only into empty default-bg cells.
     * @param {boolean} on
     */
    set_matrix_rain_enabled(on) {
        wasm.atermterminal_set_matrix_rain_enabled(this.__wbg_ptr, on);
    }
    /**
     * Accessibility motion gate for PHOSPHOR.
     * @param {boolean} on
     */
    set_matrix_rain_reduced_motion(on) {
        wasm.atermterminal_set_matrix_rain_reduced_motion(this.__wbg_ptr, on);
    }
    /**
     * Set the per-cell minimum contrast ratio (xterm's `minimumContrastRatio`,
     * 1..=21): every glyph fg is floored against its OWN cell bg — the classic
     * rescue for bright-white SGR text on a light theme. `ratio <= 1.0` turns
     * the floor off (the default; xterm treats 1 as "do nothing"). Cells whose
     * fg == bg are never adjusted (SGR 8 conceal renders fg = bg and must stay
     * hidden). Appearance-only, so force one full repaint next frame.
     * @param {number} ratio
     */
    set_minimum_contrast(ratio) {
        wasm.atermterminal_set_minimum_contrast(this.__wbg_ptr, ratio);
    }
    /**
     * Set an ANSI/indexed palette colour (index 0–255; 0–15 are the 16 ANSI
     * colours) to RGB components, so the renderer resolves SGR-indexed cell colours
     * through the host's theme palette instead of the engine's built-in VGA
     * defaults. Per-cell truecolor SGR still flows independently.
     * @param {number} index
     * @param {number} r
     * @param {number} g
     * @param {number} b
     */
    set_palette_color(index, r, g, b) {
        wasm.atermterminal_set_palette_color(this.__wbg_ptr, index, r, g, b);
    }
    /**
     * Set the predictive-echo display mode: `"off"` (the default) |
     * `"adaptive"` (show after the current line confirms echo and its measured
     * RTT is high enough to benefit) | `"always"` (power users / demos). Case-
     * insensitive; unknown strings fail safe to `off` — the native
     * `predictive_echo` domain.
     * @param {string} mode
     */
    set_predictive_echo(mode) {
        const ptr0 = passStringToWasm0(mode, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_predictive_echo(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Swap the PRIMARY face (the host's `terminalFontFamily`) from font bytes and
     * re-rasterize. The host re-reads cell_width/cell_height + recomputes the grid
     * after (the new face may have different metrics). No-throw on a bad blob.
     * @param {Uint8Array} bytes
     */
    set_primary_font(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_set_primary_font(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Re-rasterize at a new cell font px (host DPI / devicePixelRatio change) so the
     * pane rebuilds its cell metrics instead of staying frozen at the construction
     * dpr. The host re-reads cell_width/cell_height + recomputes the grid after.
     * @param {number} px
     */
    set_px(px) {
        wasm.atermterminal_set_px(this.__wbg_ptr, px);
    }
    /**
     * Set the engine's scrollback line limit (history lines retained behind the live
     * viewport). `lines == 0` means unlimited (bounded only by host memory). This
     * engine is ring-only (no tiered store), so the limit re-caps the retention ring
     * itself: shrinking evicts the oldest lines immediately, growing extends retention
     * lazily (no eager allocation). Targets the primary-content grid — reaching the
     * saved primary through an alt screen; the alt buffer keeps its spec'd zero
     * scrollback — and re-clamps the scroll position. Without this the engine keeps
     * its construction default (a 10k-line ring) on every pane.
     * @param {number} lines
     */
    set_scrollback_limit(lines) {
        wasm.atermterminal_set_scrollback_limit(this.__wbg_ptr, lines);
    }
    /**
     * Set the explicit selected-text foreground (theme `selectionForeground`),
     * 0x00RRGGBB, or `undefined` to restore the WCAG contrast-floor default.
     * Appearance-only, so force one full repaint next frame.
     * @param {number | null} [fg]
     */
    set_selection_fg(fg) {
        wasm.atermterminal_set_selection_fg(this.__wbg_ptr, isLikeNone(fg) ? 0x100000001 : (fg) >>> 0);
    }
    /**
     * Mark the pane unfocused (`true`) / focused (`false`): when unfocused, the
     * selection band paints with the dimmer inactive bg (xterm
     * `selectionInactiveBackground`) instead of the active selection colour.
     * Appearance-only, so force one full repaint next frame.
     * @param {boolean} inactive
     */
    set_selection_inactive(inactive) {
        wasm.atermterminal_set_selection_inactive(this.__wbg_ptr, inactive);
    }
    /**
     * Set the inactive (unfocused) selection background (0x00RRGGBB), or
     * `undefined` to derive it from the active selection bg blended toward the
     * theme bg. Only takes visible effect while the pane is marked unfocused.
     * Appearance-only, so force one full repaint next frame.
     * @param {number | null} [bg]
     */
    set_selection_inactive_bg(bg) {
        wasm.atermterminal_set_selection_inactive_bg(this.__wbg_ptr, isLikeNone(bg) ? 0x100000001 : (bg) >>> 0);
    }
    /**
     * Alt-screen suppression (native `[sparkle_words] suppress_in_alt_screen`,
     * default off): when on, full-screen apps render undecorated — the v1
     * launch behavior. Off, the alternate screen sparkles like the main one.
     * @param {boolean} on
     */
    set_sparkle_alt_screen_suppression(on) {
        wasm.atermterminal_set_sparkle_alt_screen_suppression(this.__wbg_ptr, on);
    }
    /**
     * Per-class gates (native `[sparkle_words.<class>] enabled`): profanity
     * (supernova/sparkle), feline (peeking cat/paw), orca (water splash),
     * emphasis (ink-only; effective only while ink is enabled).
     * @param {boolean} profanity
     * @param {boolean} feline
     * @param {boolean} orca
     * @param {boolean} emphasis
     */
    set_sparkle_classes(profanity, feline, orca, emphasis) {
        wasm.atermterminal_set_sparkle_classes(this.__wbg_ptr, profanity, feline, orca, emphasis);
    }
    /**
     * Custom word-effect specs (native `[[sparkle_words.custom]]`): pass the
     * SAME TOML fragment the native config carries — per-word `ink` /
     * `burst` / `graphic` axes. Custom words are auto-appended to the
     * emphasis class (CJK surfaces included), override class defaults, and
     * bypass per-class enable gates. Malformed TOML fails open to no
     * customs; pass `undefined` to clear.
     * @param {string | null} [toml]
     */
    set_sparkle_custom_specs(toml) {
        var ptr0 = isLikeNone(toml) ? 0 : passStringToWasm0(toml, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_sparkle_custom_specs(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Comma-separated exact surfaces to never decorate (the native global
     * `deny` and `ignore_words` channel), replacing the current set. Entries
     * are case/diacritic-folded with the scanner's own fold.
     * @param {string} words_csv
     */
    set_sparkle_deny(words_csv) {
        const ptr0 = passStringToWasm0(words_csv, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_sparkle_deny(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Feline knobs (native `[sparkle_words.feline]`): `style` = "cat" (the
     * v2 peeking cat, default) or "paw" (the exact v1 steady paw); `color`
     * omitted = the native soft pink; `intensity` clamps 0..=1; `idle` =
     * sparse blink/ear-twitch one-shots (focus-gated, ≤1/s); `gaze` = pupils
     * track the cursor (present-driven, zero new wakes); `magic` = rare
     * Fortune/Nebula cats; `allow_bare_cat` = decorate the literal 3-letter
     * `cat`; `cjk_single_char` = match a lone cat ideograph (high-FP).
     * @param {string} style
     * @param {number | null | undefined} color
     * @param {number} intensity
     * @param {boolean} idle
     * @param {boolean} gaze
     * @param {boolean} magic
     * @param {boolean} allow_bare_cat
     * @param {boolean} cjk_single_char
     */
    set_sparkle_feline(style, color, intensity, idle, gaze, magic, allow_bare_cat, cjk_single_char) {
        const ptr0 = passStringToWasm0(style, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_sparkle_feline(this.__wbg_ptr, ptr0, len0, isLikeNone(color) ? 0x100000001 : (color) >>> 0, intensity, idle, gaze, magic, allow_bare_cat, cjk_single_char);
    }
    /**
     * Animated-ink knobs (native `[sparkle_words.ink]`): the glyph-ink
     * gradient + specular sweep on matched words. `strength` clamps 0..=1;
     * `sweep_ms` clamps 350..=6000 (floor 600 while `loop_` — the WCAG flash
     * margin, structural); `loop_` re-sweeps while the word stays visible.
     * @param {boolean} enabled
     * @param {number} strength
     * @param {number} sweep_ms
     * @param {boolean} loop_
     */
    set_sparkle_ink(enabled, strength, sweep_ms, loop_) {
        wasm.atermterminal_set_sparkle_ink(this.__wbg_ptr, enabled, strength, sweep_ms, loop_);
    }
    /**
     * Comma-separated languages whose AMBIGUOUS homograph lexicon entries
     * un-gate (native `languages`, default `"en"`; non-ambiguous forms load
     * regardless; `"all"` un-gates everything). Rebuilds the lexicon.
     * @param {string} languages_csv
     */
    set_sparkle_languages(languages_csv) {
        const ptr0 = passStringToWasm0(languages_csv, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_sparkle_languages(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * User lexicon-override TOML merged over the builtin (the native
     * `lexicon` file / `extra_words` channel — the same `[[entry]]` schema).
     * Pass `undefined` to clear. A malformed override falls back to the
     * builtin lexicon (the native fail-open posture).
     * @param {string | null} [toml]
     */
    set_sparkle_lexicon_override(toml) {
        var ptr0 = isLikeNone(toml) ? 0 : passStringToWasm0(toml, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_sparkle_lexicon_override(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Profanity knobs (native `[sparkle_words.profanity]`): `style` =
     * "rainbow" (the v3 animated rainbow ink, the default) | "nova" (the v2
     * classic nova) | "sparkle" (the exact v1 twinkle). Clamps are the
     * native flash-safety floors and are not bypassable: `density` 1..=12
     * sparks, `anim_ms` 350..=10000, `jitter` 0..=6 px, `intensity` 0..=1.
     * `magic` = rare Quasar/Singularity novas. `supernova_chance` (0..=100,
     * 0 disables) = the FUCK SUPER NOVA escalation chance under
     * `style = "rainbow"`. The window-wide ignition limiter (≤2 ignitions
     * per rolling second) is always on.
     * @param {string} style
     * @param {number} density
     * @param {number} anim_ms
     * @param {number} jitter
     * @param {number} intensity
     * @param {boolean} magic
     * @param {number} supernova_chance
     */
    set_sparkle_profanity(style, density, anim_ms, jitter, intensity, magic, supernova_chance) {
        const ptr0 = passStringToWasm0(style, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_sparkle_profanity(this.__wbg_ptr, ptr0, len0, density, anim_ms, jitter, intensity, magic, supernova_chance);
    }
    /**
     * Force the static, non-animating path (no twinkle/jitter/sweep; novas
     * collapse to a static glint) — the accessibility `reduced_motion`
     * override. The engine's flash-limiter floors apply regardless.
     * @param {boolean} on
     */
    set_sparkle_reduced_motion(on) {
        wasm.atermterminal_set_sparkle_reduced_motion(this.__wbg_ptr, on);
    }
    /**
     * MASTER sparkle-words switch (native `[sparkle_words] enabled` +
     * `toggle_sparkle_words` panic-off). Enabling compiles the multilingual
     * lexicon once and starts scanning the visible grid; disabling drops all
     * occurrence state and restores byte-identical output next render.
     * Defaults (until other setters run) mirror the native launch config:
     * all four families on (profanity nova / feline cat / orca splash /
     * emphasis ink), animated ink on.
     * @param {boolean} on
     */
    set_sparkle_words_enabled(on) {
        wasm.atermterminal_set_sparkle_words_enabled(this.__wbg_ptr, on);
    }
    /**
     * Include `HaloMode::Over` VEILS (light-theme smoke/steam) in the spill
     * band (default `true`, keeping the seam-continuity law universal).
     * `false` scopes the spill to additive light + fire ink — the policy
     * escape if veils over neighbouring panes read badly; the band then
     * intentionally diverges from the in-frame veil pixels at the clip line.
     * Applies from the next `render()`.
     * @param {boolean} on
     */
    set_spill_include_veils(on) {
        wasm.atermterminal_set_spill_include_veils(this.__wbg_ptr, on);
    }
    /**
     * Inject a broad-coverage SYMBOL fallback face from font bytes, so symbol
     * glyphs the primary + fallback faces lack render real shapes instead of
     * tofu. The byte-injection sibling of the config `symbol_font` path: the host
     * supplies the OS symbol bytes (the canvas can't read the filesystem).
     * No-throw: a bad blob surfaces a catchable JS exception and leaves the
     * existing face untouched.
     * @param {Uint8Array} bytes
     */
    set_symbol_font(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.atermterminal_set_symbol_font(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * [`AtermTerminal::set_symbol_font`] from a registered handle.
     * @param {number} handle
     */
    set_symbol_font_registered(handle) {
        const ret = wasm.atermterminal_set_symbol_font_registered(this.__wbg_ptr, handle);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB), so a host
     * theme change re-themes the pane without rebuilding it. Per-cell SGR colours
     * flow independently; pair with set_palette_color for the ANSI palette.
     * @param {number} fg
     * @param {number} bg
     * @param {number} cursor
     * @param {number} selection
     */
    set_theme(fg, bg, cursor, selection) {
        wasm.atermterminal_set_theme(this.__wbg_ptr, fg, bg, cursor, selection);
    }
    /**
     * Override the characters that BREAK a double-click word (the host's
     * word-separator setting, xterm.js `wordSeparators` semantics): a word
     * becomes a maximal run of NON-separator characters. `undefined` restores
     * the engine's default class-based word logic (alphanumeric + `_`)
     * exactly. Smart-selection RULES (url/file_path/email/…) still take
     * precedence for both `selection_word` and `link_at`; the separators only
     * shape the plain-word fallback.
     * @param {string | null} [separators]
     */
    set_word_separators(separators) {
        var ptr0 = isLikeNone(separators) ? 0 : passStringToWasm0(separators, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        wasm.atermterminal_set_word_separators(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Lexicon build diagnostics (v3 §6), newline-joined — one warning per
     * line for every user/custom surface that can never scan as written
     * (single-char CJK without the `cjk_single_char` opt-in, mixed-script /
     * multi-word) or collides across classes; the same warnings the native
     * resolver logs. Empty string while sparkle words are off or the lexicon
     * is clean. Filtered by the current knobs: a "requires cjk_single_char =
     * true" warning disappears once `set_sparkle_feline` enables the opt-in.
     * @returns {string}
     */
    get sparkle_lexicon_warnings() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.atermterminal_sparkle_lexicon_warnings(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Whether the sparkle-words master is currently on.
     * @returns {boolean}
     */
    get sparkle_words_enabled() {
        const ret = wasm.atermterminal_sparkle_words_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Byte length of the spill buffer (`0` at 0/0 chrome — the identity law:
     * no band, no bytes, no per-frame cost).
     * @returns {number}
     */
    spill_len() {
        const ret = wasm.atermterminal_spill_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Byte offset (in wasm linear memory) of the straight-alpha RGBA spill
     * buffer: four packed row-major strips — **top** `(0, 0, width,
     * pad+head)`, **bottom** `(0, height−pad, width, pad)`, **left** `(0,
     * pad+head, pad, gridH)`, **right** `(width−pad, pad+head, pad, gridH)`
     * with `gridH = height − 2·pad − head` — in that order. The pointer is
     * STABLE across frames (the buffer re-rasters in place); it moves only
     * when chrome or the grid size changes, so a host may hold its view
     * between frames of one geometry (wasm memory GROWTH still detaches JS
     * views — rebuild per read, the `rgba_ptr` rule).
     * @returns {number}
     */
    spill_ptr() {
        const ret = wasm.atermterminal_spill_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Number of dirty rects from the LAST `render()` (0 on a no-change
     * frame). Read together with [`spill_rects_ptr`](Self::spill_rects_ptr).
     * @returns {number}
     */
    spill_rect_count() {
        const ret = wasm.atermterminal_spill_rect_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Byte offset (in wasm linear memory) of the packed dirty-rect array:
     * `spill_rect_count()` rects of 4 `i32`s — `x, y, w, h`, FRAME-ABSOLUTE
     * device px. Same read discipline as [`rgba_ptr`](Self::rgba_ptr):
     * consume synchronously after `render()`, never cache the JS view.
     * @returns {number}
     */
    spill_rects_ptr() {
        const ret = wasm.atermterminal_spill_rects_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Monotone revision of the spill-band content: advances ONLY when the
     * exported bytes changed. Typing-only frames with a settled (or
     * grid-interior) glow, idle re-renders, and 0/0 chrome keep it still —
     * an unchanged value is the engine's word that the host may skip its
     * blit without reading a single spill byte.
     * @returns {number}
     */
    spill_rev() {
        const ret = wasm.atermterminal_spill_rev(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Drain the missing-font CLASS bits (1 = text/mono fallback, 2 = colour
     * emoji) accumulated by renders since the last call. The host polls this
     * after a frame and lazily injects ONLY the face class actually missed —
     * an ASCII-only session never pays the multi-hundred-MB emoji/CJK payload.
     * Latch per class host-side: a bit can re-fire if the injected faces still
     * miss a char.
     * @returns {number}
     */
    take_missing_font_classes() {
        const ret = wasm.atermterminal_take_missing_font_classes(this.__wbg_ptr);
        return ret;
    }
    /**
     * Drain pending desktop notifications (queued since the last drain) as a
     * JSON array of `{"id","title","body","urgency"}` objects — string or
     * `null` fields, urgency ∈ `"low"|"normal"|"critical"`; `None` when
     * nothing is pending. OSC 9's bare message arrives as `body` with no
     * title (the native mapping); OSC 99/777 carry their structured
     * id/title/body. The queue is bounded (new notifications are dropped
     * beyond the cap until drained), so poll after `process` like
     * `take_osc_events`.
     * @returns {string | undefined}
     */
    take_notifications() {
        const ret = wasm.atermterminal_take_notifications(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Drain pending OSC app-events as a JSON array of `[code, payload]` pairs
     * (`[[7,"/home"],[52,"copied"]]`); `None` when the queue is empty. These
     * carry REAL decoded payloads (OSC 52 clipboard / OSC 7 cwd / OSC 133 mark)
     * the host routes to UI handlers — distinct from `take_response` (PTY replies).
     * @returns {string | undefined}
     */
    take_osc_events() {
        const ret = wasm.atermterminal_take_osc_events(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Drain the engine's pending query replies (DA1/DA2/DSR/CPR/DECRQM/OSC color/
     * window-size, …) — the host forwards these to the PTY so the RENDERER (not the
     * daemon, which stays silent) is the authoritative responder. Call after each
     * `process`; returns `None` when nothing is pending.
     * @returns {Uint8Array | undefined}
     */
    take_response() {
        const ret = wasm.atermterminal_take_response(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * The window title (OSC 0/2), or `None` when unset — replaces the separate
     * title channel that fed off the shadow xterm so snapshots keep window titles.
     * @returns {string | undefined}
     */
    title() {
        const ret = wasm.atermterminal_title(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Last-rendered framebuffer width in pixels.
     * @returns {number}
     */
    get width() {
        const ret = wasm.atermterminal_width(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) AtermTerminal.prototype[Symbol.dispose] = AtermTerminal.prototype.free;

/**
 * One slice of a budgeted search ([`AtermTerminal::search_budgeted`]).
 */
export class BudgetedSearchResult {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(BudgetedSearchResult.prototype);
        obj.__wbg_ptr = ptr;
        BudgetedSearchResultFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BudgetedSearchResultFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_budgetedsearchresult_free(ptr, 0);
    }
    /**
     * Whether every retained row has been scanned and every match delta has
     * been delivered. Dense searches can have `rows_fed == total_rows` while
     * this remains false for bounded backlog-drain turns.
     * @returns {boolean}
     */
    get complete() {
        const ret = wasm.budgetedsearchresult_complete(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Token to resume with; `undefined` once complete.
     * @returns {bigint | undefined}
     */
    get cursor() {
        const ret = wasm.budgetedsearchresult_cursor(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : BigInt.asUintN(64, ret[1]);
    }
    /**
     * True when the results may be truncated: index eviction dropped old
     * rows, or the engine's match cap was reached. (The legacy
     * [`AtermTerminal::search`] export silently drops this signal.)
     * @returns {boolean}
     */
    get incomplete_index() {
        const ret = wasm.budgetedsearchresult_incomplete_index(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Final oldest absolute line retained by the completed search index. The
     * deterministic eviction schedule makes this stable from the first turn.
     * When nonzero with `incomplete_index`, older history was evicted; a zero
     * watermark with `incomplete_index` indicates match-cap-only truncation.
     * @returns {number}
     */
    get lowest_retained_line() {
        const ret = wasm.budgetedsearchresult_lowest_retained_line(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Stable match DELTA as flat `[abs_line, start_col, len]` triplets (same
     * coordinate contract as [`AtermTerminal::search`]). Append across calls;
     * already-reported matches keep their order and positions.
     * @returns {Uint32Array}
     */
    get matches() {
        const ret = wasm.budgetedsearchresult_matches(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Whether this step starts a new logical result stream. Clear previously
     * accumulated match deltas before appending this step when true.
     * @returns {boolean}
     */
    get reset() {
        const ret = wasm.budgetedsearchresult_reset(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Rows scanned so far (progress numerator; restarts reset it).
     * @returns {number}
     */
    get rows_fed() {
        const ret = wasm.budgetedsearchresult_rows_fed(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Stable identity for the logical search, including its completing step;
     * `undefined` only for an empty/invalid query result.
     * @returns {bigint | undefined}
     */
    get search_id() {
        const ret = wasm.budgetedsearchresult_search_id(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : BigInt.asUintN(64, ret[1]);
    }
    /**
     * Total rows this search will scan (progress denominator).
     * @returns {number}
     */
    get total_rows() {
        const ret = wasm.budgetedsearchresult_total_rows(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) BudgetedSearchResult.prototype[Symbol.dispose] = BudgetedSearchResult.prototype.free;

/**
 * A detected link under a cell: its text/URL, the half-open display-column span
 * it covers, and a `kind` discriminant (0=osc8, 1=url, 2=file_path, 3=other).
 */
export class LinkHit {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(LinkHit.prototype);
        obj.__wbg_ptr = ptr;
        LinkHitFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LinkHitFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_linkhit_free(ptr, 0);
    }
    /**
     * Exclusive end display column of the link span.
     * @returns {number}
     */
    get end_col() {
        const ret = wasm.linkhit_end_col(this.__wbg_ptr);
        return ret;
    }
    /**
     * Link kind: 0=osc8, 1=url, 2=file_path, 3=other.
     * @returns {number}
     */
    get kind() {
        const ret = wasm.linkhit_kind(this.__wbg_ptr);
        return ret;
    }
    /**
     * Inclusive start display column of the link span.
     * @returns {number}
     */
    get start_col() {
        const ret = wasm.linkhit_start_col(this.__wbg_ptr);
        return ret;
    }
    /**
     * The link's URL/target text.
     * @returns {string}
     */
    get url() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.linkhit_url(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
}
if (Symbol.dispose) LinkHit.prototype[Symbol.dispose] = LinkHit.prototype.free;

/**
 * Metadata for a legacy-contract search ([`AtermTerminal::search_meta`]).
 */
export class SearchMeta {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(SearchMeta.prototype);
        obj.__wbg_ptr = ptr;
        SearchMetaFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        SearchMetaFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_searchmeta_free(ptr, 0);
    }
    /**
     * True when the results may be truncated: index eviction dropped old rows
     * before they could be searched, or the engine's match cap was reached.
     * @returns {boolean}
     */
    get incomplete() {
        const ret = wasm.searchmeta_incomplete(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Number of matches the paired [`AtermTerminal::search`] call returns
     * (i.e. its flat triplet array length / 3), after any cap.
     * @returns {number}
     */
    get match_count() {
        const ret = wasm.searchmeta_match_count(this.__wbg_ptr);
        return ret >>> 0;
    }
}
if (Symbol.dispose) SearchMeta.prototype[Symbol.dispose] = SearchMeta.prototype.free;

/**
 * Selection bounds in DISPLAY viewport cell coords (0 = top visible row),
 * inclusive of `start`, with `end` already side-adjusted to match
 * `selection_text` and the painted highlight.
 */
export class SelectionRange {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(SelectionRange.prototype);
        obj.__wbg_ptr = ptr;
        SelectionRangeFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        SelectionRangeFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_selectionrange_free(ptr, 0);
    }
    /**
     * End column (display-relative, side-adjusted/inclusive).
     * @returns {number}
     */
    get end_x() {
        const ret = wasm.selectionrange_end_x(this.__wbg_ptr);
        return ret;
    }
    /**
     * End row (display-relative).
     * @returns {number}
     */
    get end_y() {
        const ret = wasm.selectionrange_end_y(this.__wbg_ptr);
        return ret;
    }
    /**
     * Start column (display-relative).
     * @returns {number}
     */
    get start_x() {
        const ret = wasm.selectionrange_start_x(this.__wbg_ptr);
        return ret;
    }
    /**
     * Start row (display-relative, 0 = top visible row).
     * @returns {number}
     */
    get start_y() {
        const ret = wasm.selectionrange_start_y(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) SelectionRange.prototype[Symbol.dispose] = SelectionRange.prototype.free;

/**
 * STATELESS key encoder for worker-hosted engines: encode a DOM keyboard
 * event against an explicit mode-bits snapshot instead of a live terminal.
 *
 * Contract: the engine lives in a Web Worker while keydown handling runs on
 * the main thread, so the host mirrors [`AtermTerminal::keyboard_mode_bits`]
 * through its engine-state snapshot and encodes synchronously here, accepting
 * one-frame staleness — the same tradeoff the host already accepts for
 * DECCKM gating via `is_app_cursor_mode`.
 *
 * Parameters match [`AtermTerminal::encode_key`] (`key` = DOM
 * `KeyboardEvent.key`; `mods` = SHIFT=1, ALT=2, CTRL=4, SUPER=8;
 * `event_type` = 0=Press, 1=Repeat, 2=Release; `base_layout_key` = US-QWERTY
 * char for Kitty `REPORT_ALTERNATE_KEYS`), plus `mode_bits` from
 * `keyboard_mode_bits` (a `u16` bitflags value zero-extended to `u32`;
 * undefined bits are truncated away). With fresh bits the output is
 * byte-identical to the instance method.
 * @param {string} key
 * @param {number} mods
 * @param {number} event_type
 * @param {string | null | undefined} base_layout_key
 * @param {number} mode_bits
 * @returns {Uint8Array | undefined}
 */
export function encode_key_with_mode(key, mods, event_type, base_layout_key, mode_bits) {
    const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const char1 = isLikeNone(base_layout_key) ? 0xFFFFFF : base_layout_key.codePointAt(0);
    if (char1 !== 0xFFFFFF) { _assertChar(char1); }
    const ret = wasm.encode_key_with_mode(ptr0, len0, mods, event_type, char1, mode_bits);
    let v3;
    if (ret[0] !== 0) {
        v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v3;
}

/**
 * Register a font blob for handle-based reuse by every engine in this module.
 * Content-interned: registering identical bytes returns a handle to ONE shared
 * copy (and re-registration returns the same storage, so handles stay cheap).
 * @param {Uint8Array} bytes
 * @returns {number}
 */
export function register_font(bytes) {
    const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.register_font(ptr0, len0);
    return ret >>> 0;
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_is_undefined_9e4d92534c42d778: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_throw_be289d5034ed271b: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_call_389efe28435a9388: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments); },
        __wbg_error_7534b8e9a36f1ab4: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_new_8a6f238a6ece86ea: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_no_args_1c7c842f08d00ebb: function(arg0, arg1) {
            const ret = new Function(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_now_2c95c9de01293173: function(arg0) {
            const ret = arg0.now();
            return ret;
        },
        __wbg_now_a3af9a2f4bbaa4d1: function() {
            const ret = Date.now();
            return ret;
        },
        __wbg_performance_7a3ffd0b17f663ad: function(arg0) {
            const ret = arg0.performance;
            return ret;
        },
        __wbg_stack_0ed75d68575b0f3c: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_12837167ad935116: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_e628e89ab3b1c95f: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_a621d3dfbb60d0ce: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_f8727f0cf888e0bd: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./aterm_wasm_bg.js": import0,
    };
}

const AtermTerminalFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_atermterminal_free(ptr >>> 0, 1));
const BudgetedSearchResultFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_budgetedsearchresult_free(ptr >>> 0, 1));
const LinkHitFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_linkhit_free(ptr >>> 0, 1));
const SearchMetaFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_searchmeta_free(ptr >>> 0, 1));
const SelectionRangeFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_selectionrange_free(ptr >>> 0, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function _assertChar(c) {
    if (typeof(c) === 'number' && (c >= 0x110000 || (c >= 0xD800 && c < 0xE000))) throw new Error(`expected a valid Unicode scalar value, found ${c}`);
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasm;
function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('aterm_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
