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
     * [`AtermTerminal::add_fallback_font`] from a registered handle.
     */
    add_fallback_font_registered(handle: number): void;
    /**
     * Advance the effects clock by `dt_ms` (the host's rAF delta). The
     * engines never read a wall clock: same PTY bytes + same `dt` stream ⇒
     * identical frames. Negative/NaN deltas are ignored.
     */
    advance_effects(dt_ms: number): void;
    /**
     * Authorize OSC 52 clipboard *write* (set) so the engine queues OSC 52
     * app-events for the host to drain via `take_osc_events`. Without this the
     * engine is fail-closed (CF-004) and silently drops PTY-origin OSC 52 set
     * sequences, so they never reach the host. The host still gates the actual
     * clipboard write on its own user setting (defense in depth).
     */
    authorize_clipboard_write(): void;
    /**
     * Mint an EXTRA OSC 8 URI scheme onto the engine's safe allowlist (orca
     * deep-links §7) — e.g. `authorize_hyperlink_scheme("orca")` so
     * host-emitted `orca://` OSC-8 hyperlinks linkify. Returns `false`
     * (refused, nothing changes) for a malformed / over-long scheme, a
     * never-allow scheme (`javascript`/`data`/`file`/…, however cased), or
     * when the bounded set (4) is full; `true` when live (idempotent).
     * Every other OSC-8 guard — byte cap, control-char and BiDi filters,
     * the OSC-8 capability gate — still applies to extra-scheme URIs.
     */
    authorize_hyperlink_scheme(scheme: string): boolean;
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
     * Whether the DISPLAY cell at `row`/`col` is a wide (double-width)
     * character; `None` when out of range. Resolved through the same
     * display-offset-aware row view as `cell_text` so a host's per-cell walk
     * sees one coherent row.
     */
    cell_is_wide(row: number, col: number): boolean | undefined;
    /**
     * Grapheme text at DISPLAY cell `row`/`col` (display_offset-aware, like
     * `row_text`) — base char plus complex cluster and combining marks. Empty
     * string for a blank cell, a wide-continuation spacer, or out-of-range
     * coords. Hosts rebuild scrolled-back rows per-cell from this, so it must
     * track the scroll position; the live-frame reader is `get_line_text`.
     */
    cell_text(row: number, col: number): string;
    /**
     * Drain the edge-triggered BEL flag: `true` if a BEL fired since the last
     * call, then clears it (so a poll-based host can flash/ring without the
     * synchronous bell callback).
     */
    drain_bell(): boolean;
    /**
     * Promote up to `max_lines` staged lines into the compressed store
     * (`0` = the render-frame batch size) and apply any pending global-share
     * change. Returns the lines STILL staged. For hosts draining a pane
     * whose `render()` is throttled — see the glue contract in the module
     * docs.
     */
    drain_scrollback_backlog(max_lines: number): number;
    /**
     * Milliseconds until the next rain engine tick, or `undefined` when
     * active frame-rate motion needs rAF (and when every effect is idle).
     */
    effects_next_deadline_ms(): number | undefined;
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
     */
    encode_key(key: string, mods: number, event_type: number, base_layout_key?: string | null): Uint8Array | undefined;
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
     * `true` while any effect is animating. Consult
     * [`Self::effects_next_deadline_ms`] first: rain is active at 12/30 Hz and
     * must not drive a 60/120 Hz display-rAF loop.
     */
    is_effects_active(): boolean;
    /**
     * The last completed OSC-133 block's output as JSON, following the
     * `take_osc_events` JSON-drain convention (CM-A3, "Copy Last Command
     * Output"):
     *   `{"status":"ok","text":"…","exitCode":0}` — output read in full
     *     (`exitCode` is `null` when the block was finalized without OSC 133 D,
     *     e.g. an interrupted command whose next prompt closed it);
     *   `{"status":"evicted"}` — the block's rows scrolled past the scrollback
     *     cap (DL-1: an honest marker, never silently-shifted/empty text);
     *   `undefined` — nothing to copy: no shell integration, no finished block
     *     yet (incl. a snapshot-rehydrated pane — blocks are excluded from
     *     checkpoints), or the block never reached its output phase.
     *
     * `&self` — the read rides `Terminal::last_completed_block`, which was added
     * alongside this binding precisely so the facade does not need the `&mut`
     * `output_blocks()` (`make_contiguous`) path.
     */
    last_command_output(): string | undefined;
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
     * [`AtermTerminal::new`] from a registered PRIMARY font handle.
     */
    static new_registered(rows: number, cols: number, font_handle: number, px: number, fg: number, bg: number, cursor: number, selection: number): AtermTerminal;
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
    note_keystroke(): void;
    /**
     * Feed wheel/PgUp activity from an alternate-screen TUI so rain pauses
     * while the user reads its transcript.
     */
    note_matrix_rain_alt_scroll(): void;
    /**
     * Feed a terminal visual bell into PHOSPHOR's bounded alert tint.
     */
    note_matrix_rain_bell(): void;
    /**
     * Payload-free observable-work pulse. Codes are `0 assistant, 1 inspect,
     * 2 modify, 3 execute, 4 network, 5 branch, 6 waiting, 7 success,
     * 8 failure, 9 interrupted, 10 turn-start`; weight clamps to `1..=8`.
     * Turn-start also releases the unsent-composer material gate.
     */
    note_matrix_rain_signal(code: number, weight: number): void;
    /**
     * Register a Backspace: cancels our OWN trailing guess only (erasing
     * already-committed real content is left to the program's echo). Returns
     * whether state changed.
     */
    predict_backspace(): boolean;
    /**
     * Register a printable character the host just wrote to the PTY (the
     * keydown seam — call beside `encode_key`). The guess anchors at the
     * engine's live cursor, extends pending type-ahead, and never crosses the
     * right margin. Returns whether a guess is now TRACKED — display is a
     * separate gate (see [`predict_overlay`](Self::predict_overlay)).
     */
    predict_char(ch: string): boolean;
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
    predict_line_submit(): void;
    /**
     * Milliseconds until the oldest pending guess self-expires (the glitch
     * flush), or `undefined` when none is pending. Arm ONE timer for this and
     * call [`predict_overlay`](Self::predict_overlay) + repaint there, so a
     * stale ghost is erased even when no further input or output arrives.
     */
    predict_next_deadline_ms(): number | undefined;
    /**
     * The ghost cells to paint THIS frame, as flat `[row, col, codepoint]`
     * triples (a `Uint32Array` in JS). The host renders them tentatively
     * (dim/underline) and may advance its DRAWN cursor past the last one,
     * mosh-style. Runs the expiry self-heal first, then the display gate:
     * `always` ⇒ all pending; `adaptive` ⇒ all pending after an echo is confirmed
     * on this line and measured RTT is high enough to help. Empty in app-owned
     * Kitty composers and while scrolled into history.
     */
    predict_overlay(): Uint32Array;
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
    predict_reconcile(): void;
    /**
     * Drop all in-flight guesses because this SAME terminal's coordinate space
     * changed (`resize` calls this automatically). The confirmation epoch is
     * forgotten, while this session's learned link RTT remains useful.
     */
    predict_reset(): void;
    /**
     * Reset for a DIFFERENT pane/session. In addition to coordinate-bound
     * guesses, forget the learned echo RTT so a slow remote pane cannot make a
     * newly selected local pane display speculation. Hosts that keep one
     * `AtermTerminal` per session never need this; pane-reusing hosts call it at
     * the identity switch.
     */
    predict_session_reset(): void;
    /**
     * Number of dirty present bands from the LAST `render()`. `0` = the
     * frame is byte-identical to the previous one — skip RGBA reads and
     * `putImageData` entirely. Read together with
     * [`present_bands_ptr`](Self::present_bands_ptr).
     */
    present_band_count(): number;
    /**
     * Byte offset (in wasm linear memory) of the packed dirty-band array:
     * `present_band_count()` bands of 4 `i32`s — `x, y, w, h`,
     * FRAME-ABSOLUTE device px, non-overlapping, top-to-bottom. Same read
     * discipline as `rgba_ptr`: consume synchronously after `render()`,
     * never cache the JS view across engine calls. The host presents each
     * band with `putImageData(imageData, 0, 0, x, y, w, h)` over the SAME
     * full-frame ImageData `rgba_ptr` backs. See the module docs for the
     * overlay/second-canvas contract.
     */
    present_bands_ptr(): number;
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
     */
    pump_reflow(): boolean;
    /**
     * Tune the per-pump rewrap budget (INPUT history lines per
     * [`Self::pump_reflow`] step). `0` restores the default
     * (`REFLOW_STEP_BUDGET_LINES`, 2_000 ≈ ~3ms native — see the constant's
     * sizing note). Hosts with generous idle windows can raise it to finish
     * deep histories in fewer tasks; latency-sensitive hosts can lower it.
     */
    pump_reflow_budget(max_lines: number): void;
    /**
     * Rasterize the current grid into the internal RGBA8 framebuffer via the
     * damage-tracked path: only rows that changed since the last frame are
     * re-rendered (the rest reuse the persistent cache), so streaming output and
     * single-keystroke edits don't re-rasterize the whole grid every frame.
     */
    render(): void;
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
     */
    resize(rows: number, cols: number): void;
    /**
     * Revoke OSC 52 clipboard *write* authorization (the user toggled the
     * clipboard setting off). Returns the engine to its fail-closed default.
     */
    revoke_clipboard_write(): void;
    /**
     * Remove a host-minted extra scheme (case-insensitive), restoring the
     * engine's default allowlist posture for it.
     */
    revoke_hyperlink_scheme(scheme: string): void;
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
     * Sub-row scroll input in fractional LINES (`deltaMode ==
     * DOM_DELTA_LINE` hosts, or a host that scales pixels itself). Same
     * accumulation contract as [`scroll_px`](Self::scroll_px): whole rows
     * flip at ±1.0 accumulated, the remainder banks.
     */
    scroll_lines_frac(delta_rows: number): void;
    /**
     * Sub-row scroll input in device PIXELS — the wheel/trackpad `deltaY` at
     * `deltaMode == DOM_DELTA_PIXEL`, sign-adjusted by the host so POSITIVE
     * reveals older lines (the [`scroll_lines`](Self::scroll_lines)
     * convention). Fractions accumulate across calls; each whole
     * `cell_height` of accumulation flips one engine row, and the sub-row
     * remainder is presented by the next `render()` as a pixel shift of the
     * grid band — the host only needs to redraw afterwards.
     */
    scroll_px(delta_px: number): void;
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
     * Lines currently staged for promotion (the compress backlog).
     */
    scrollback_backlog_lines(): number;
    /**
     * This pane's EFFECTIVE scrollback budget in bytes (per-pane budget
     * after the module-global equal-share cap).
     */
    scrollback_budget_effective(): number;
    /**
     * Bytes currently held by the tiered scrollback store (hot + warm + cold,
     * including caches/overhead). Staged-but-unpromoted lines are not yet
     * counted; `drain_scrollback_backlog` settles them.
     */
    scrollback_memory_used(): number;
    /**
     * Current scrollback memory-pressure watermark: 0 = green, 1 = yellow
     * (eager compression active), 2 = red (throttle recommended) — the
     * store's budget watermark, co-landed with the truncation counter
     * (audit E10a) so budget pressure is observable before loss begins.
     */
    scrollback_pressure(): number;
    /**
     * Monotonic count of history lines LOST to non-user-requested truncation
     * (audit E10a): flood-backpressure staged-line drops, reflow-window cap
     * drops, and memory-pressure store evictions. The OUT-OF-BAND truncation
     * signal — the engine never injects a sentinel line into content; the
     * host polls this (e.g. per frame settle) and surfaces the loss in its
     * own chrome. `f64` because a sustained flood can outgrow `u32` (exact
     * to 2^53).
     */
    scrollback_truncated_lines(): number;
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
     */
    search(query: string, case_sensitive: boolean, is_regex: boolean): Uint32Array;
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
     */
    search_budgeted(query: string, case_sensitive: boolean, is_regex: boolean, resume_cursor: bigint | null | undefined, row_budget: number): BudgetedSearchResult;
    /**
     * Drop any in-flight [`AtermTerminal::search_budgeted`] state (frees the
     * partial index; outstanding cursors go stale and restart if resumed).
     * Call when the find UI closes or the query is abandoned between slices.
     */
    search_budgeted_cancel(): void;
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
     */
    search_meta(query: string, case_sensitive: boolean, is_regex: boolean): SearchMeta;
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
     * `row_ansi_text_screen` (minimal change-based SGR, wide-char aware), then the cursor
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
     * Set the DEFAULT-background opacity (0..=1; Ghostty's
     * `background-opacity`). `1.0` (the default) keeps output byte-identical.
     * Below 1.0, pixels whose bg resolved to the frame's DEFAULT background
     * come out of [`rgba`](Self::rgba)/[`rgba_ptr`](Self::rgba_ptr) with
     * `alpha = round(opacity*255)`, so `putImageData` onto a (transparent)
     * canvas lets the page show through. SGR-colored bg cells, the selection
     * band and glyph pixels stay opaque so text keeps its contrast.
     * Appearance-only, so force one full repaint next frame.
     */
    set_background_opacity(opacity: number): void;
    /**
     * Inject a REAL bold weight of the primary family so SGR-bold cells render as a
     * true heavier weight instead of synthetic embolden. The host supplies the
     * bold-variant bytes (the canvas can't read the filesystem). No-throw: a bad
     * blob surfaces a catchable JS exception and leaves the existing weight intact.
     */
    set_bold_font(bytes: Uint8Array): void;
    /**
     * [`AtermTerminal::set_bold_font`] from a registered handle.
     */
    set_bold_font_registered(handle: number): void;
    /**
     * Tell the engine the real device-pixel cell size so its CSI 14t/16t
     * window/cell-pixel reports are accurate (the engine has no canvas otherwise).
     */
    set_cell_pixel_size(width: number, height: number): void;
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
     */
    set_chrome(pad: number, head: number): void;
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
     * Configure the LUMEN cursor aurora (additive light in the cursor's
     * wake). Mirrors the native knobs + clamps: `style` ∈
     * `lumen|phaser|nyan|sparkle|fire|laser|beam|water|comet` (unknown →
     * lumen; `rainbow` = the Nyan banded ribbon);
     * `color`/`accent` omitted derive from the theme cursor (accent = color
     * brightened 1.5×) exactly like the native app; `duration_ms` clamps
     * 30..=2000, `length` (cells) 1..=512, `intensity` 0..=1 (0 = off),
     * `radius` (bloom crown, cells) 0..=2, `ring` = landing-ring ping.
     */
    set_cursor_glow(enabled: boolean, style: string, color: number | null | undefined, accent: number | null | undefined, duration_ms: number, length: number, intensity: number, radius: number, ring: boolean): void;
    /**
     * Force a hollow (unfocused) cursor when `true`, or restore the terminal's
     * DECSCUSR style when `false` — the standard focused/unfocused affordance.
     */
    set_cursor_hollow(hollow: boolean): void;
    /**
     * Set the CURSOR-fill opacity (0..=1; Ghostty's `cursor-opacity`). `1.0`
     * (the default) keeps the opaque fill + block-cursor glyph cut-out
     * byte-identical. Below 1.0 the cursor fill blends over the cell so the
     * glyph shows through. Appearance-only, so force one full repaint.
     */
    set_cursor_opacity(opacity: number): void;
    /**
     * Configure the legacy opaque comet trail (the native `cursor_trail_style
     * = "comet"` look). `color` omitted = the theme cursor; `duration_ms`
     * clamps 30..=2000, `length` 1..=512. Exactly one of trail/glow is on in
     * the native app (chosen by style); the embedder decides here.
     */
    set_cursor_trail(enabled: boolean, duration_ms: number, length: number, color?: number | null): void;
    /**
     * Arm (or clear) a **Trail Pack** — user-generated cursor trails as data.
     * Pass the pack's TOML source (`trail_pack::compile_trail_pack_toml`);
     * `undefined` clears any live pack. On a compile ERROR the prior pack is
     * LEFT INTACT and the joined diagnostics are RETURNED (never silently
     * dropped — the `set_sparkle_custom_specs` gap this closes); `Ok` returns
     * `undefined`.
     */
    set_cursor_trail_pack(toml?: string | null): string | undefined;
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
     * Focus gate for the idle one-shots (`§5.6`): an unfocused pane fires no
     * blink events (and freezes their fingerprints). Pass the pane focus.
     */
    set_effects_focused(focused: boolean): void;
    /**
     * Tri-state pane visibility for bounded rain draining:
     * `focused|visible_unfocused|hidden`.
     */
    set_effects_visibility(state: string): void;
    /**
     * Inject a colour-emoji (sbix) face from font bytes, driving the existing
     * ColorEmoji colour path. Same rationale as [`set_fallback_font`]: the host
     * supplies the OS emoji font. No-throw (the `String` Err surfaces as a
     * catchable JS exception); a bad blob leaves the slot untouched.
     */
    set_emoji_font(bytes: Uint8Array): void;
    /**
     * [`AtermTerminal::set_emoji_font`] from a registered handle. Installs the
     * SHARED interned copy (no `to_vec` of the ~190MB emoji face per pane).
     */
    set_emoji_font_registered(handle: number): void;
    /**
     * Inject a broad-coverage (CJK + symbols) fallback face from font bytes, so
     * glyphs the primary face lacks render real shapes instead of `.notdef` tofu.
     * The canvas renderer can't read the host filesystem, so the host pushes the
     * OS font bytes in. No-throw: a bad blob leaves the existing face untouched.
     */
    set_fallback_font(bytes: Uint8Array): void;
    /**
     * [`AtermTerminal::set_fallback_font`] from a registered handle.
     */
    set_fallback_font_registered(handle: number): void;
    /**
     * OpenType FONT FEATURES for the primary face, as a space-separated spec
     * (`"+ss01 zero -calt"` — bare/`+tag` enables, `-tag` disables, `tag=N` sets a
     * value). Mirrors the native `font_features` config knob. An empty/blank spec
     * clears all features. Preserves the current ligature mode; forces a repaint.
     */
    set_font_features(spec: string): void;
    /**
     * Enable/disable the Kitty keyboard protocol capability (default ON). When
     * disabled the engine acts as if the protocol is unsupported — no `CSI ? u`
     * reply, push/set/pop consumed-and-ignored, `keyboard_mode` never carries
     * kitty bits — for hosts whose platform consumes kitty sequences itself
     * (Windows ConPTY; xterm.js `vtExtensions.kittyKeyboard = false`).
     */
    set_kitty_keyboard_enabled(enabled: boolean): void;
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
     * Configure PHOSPHOR using the native bounds. `hue` is
     * `matrix|theme|custom`; `hue_color` is used only for `custom`.
     * `output_material` opts into supported literal screen codepoints; hosts
     * that cannot protect their current composer can leave it false.
     */
    set_matrix_rain(fps: number, density: number, speed: number, trail: number, alpha: number | null | undefined, head_alpha: number | null | undefined, hue: string, hue_color: number | null | undefined, mutation_ms: number, idle_secs: number, suppress_in_alt_screen: boolean, turn_wave: boolean, bell_alert: boolean, output_material: boolean, seed: bigint): void;
    /**
     * Enable PHOSPHOR matrix rain. With output material opted in, the shared
     * pipeline samples supported literal codepoints outside the current
     * cursor/composer protection band and emits only into empty default-bg cells.
     */
    set_matrix_rain_enabled(on: boolean): void;
    /**
     * Accessibility motion gate for PHOSPHOR.
     */
    set_matrix_rain_reduced_motion(on: boolean): void;
    /**
     * Set the per-cell minimum contrast ratio (xterm's `minimumContrastRatio`,
     * 1..=21): every glyph fg is floored against its OWN cell bg — the classic
     * rescue for bright-white SGR text on a light theme. `ratio <= 1.0` turns
     * the floor off (the default; xterm treats 1 as "do nothing"). Cells whose
     * fg == bg are never adjusted (SGR 8 conceal renders fg = bg and must stay
     * hidden). Appearance-only, so force one full repaint next frame.
     */
    set_minimum_contrast(ratio: number): void;
    /**
     * Set an ANSI/indexed palette colour (index 0–255; 0–15 are the 16 ANSI
     * colours) to RGB components, so the renderer resolves SGR-indexed cell colours
     * through the host's theme palette instead of the engine's built-in VGA
     * defaults. Per-cell truecolor SGR still flows independently.
     */
    set_palette_color(index: number, r: number, g: number, b: number): void;
    /**
     * Set the predictive-echo display mode: `"off"` (the default) |
     * `"adaptive"` (show after the current line confirms echo and its measured
     * RTT is high enough to benefit) | `"always"` (power users / demos). Case-
     * insensitive; unknown strings fail safe to `off` — the native
     * `predictive_echo` domain.
     */
    set_predictive_echo(mode: string): void;
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
     * Set this pane's scrollback byte budget (the tiered store evicts oldest
     * history to stay inside it). The module-global budget can only lower
     * the effective value, never raise it past this. `0` clamps to the
     * engine's 1-byte floor (retain ~nothing) — pass the real budget.
     */
    set_scrollback_budget(bytes: number): void;
    /**
     * Set the MODULE-GLOBAL scrollback budget shared by every pane of this
     * wasm module/worker (`0` = unlimited, the default): each pane's
     * effective budget becomes `min(its own budget, global / live panes)`,
     * applied as each pane is next rendered/drained.
     */
    static set_scrollback_global_budget(bytes: number): void;
    /**
     * Set the engine's scrollback line limit (history lines retained behind the live
     * viewport). `lines == 0` means unlimited (bounded only by the byte budgets). The
     * limit is ONE TOTAL retention count (audit E1) across the hot ring, staged
     * lines, and the tiered store together — the store takes the remainder after the
     * ring's fixed share, so "retain N lines" retains N, not N + ring. Targets the
     * primary-content grid — reaching the saved primary through an alt screen; the
     * alt buffer keeps its spec'd zero scrollback — and re-clamps the scroll
     * position. Without this the engine keeps its construction default
     * (`DEFAULT_LINE_LIMIT`, 100k total).
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
     * Alt-screen suppression (native `[sparkle_words] suppress_in_alt_screen`,
     * default off): when on, full-screen apps render undecorated — the v1
     * launch behavior. Off, the alternate screen sparkles like the main one.
     */
    set_sparkle_alt_screen_suppression(on: boolean): void;
    /**
     * Per-class gates (native `[sparkle_words.<class>] enabled`): profanity
     * (supernova/sparkle), feline (peeking cat/paw), orca (water splash),
     * emphasis (ink-only; effective only while ink is enabled).
     */
    set_sparkle_classes(profanity: boolean, feline: boolean, orca: boolean, emphasis: boolean): void;
    /**
     * Custom word-effect specs (native `[[sparkle_words.custom]]`): pass the
     * SAME TOML fragment the native config carries — per-word `ink` /
     * `burst` / `graphic` axes. Custom words are auto-appended to the
     * emphasis class (CJK surfaces included), override class defaults, and
     * bypass per-class enable gates. Malformed TOML fails open to no
     * customs; pass `undefined` to clear.
     */
    set_sparkle_custom_specs(toml?: string | null): void;
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
     * Profanity knobs (native `[sparkle_words.profanity]`): `style` =
     * "rainbow" (the v3 animated rainbow ink, the default) | "nova" (the v2
     * classic nova) | "sparkle" (the exact v1 twinkle). Clamps are the
     * native flash-safety floors and are not bypassable: `density` 1..=12
     * sparks, `anim_ms` 350..=10000, `jitter` 0..=6 px, `intensity` 0..=1.
     * `magic` = rare Quasar/Singularity novas. `supernova_chance` (0..=100,
     * 0 disables) = the FUCK SUPER NOVA escalation chance under
     * `style = "rainbow"`. The window-wide ignition limiter (≤2 ignitions
     * per rolling second) is always on.
     */
    set_sparkle_profanity(style: string, density: number, anim_ms: number, jitter: number, intensity: number, magic: boolean, supernova_chance: number): void;
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
     * Include `HaloMode::Over` VEILS (light-theme smoke/steam) in the spill
     * band (default `true`, keeping the seam-continuity law universal).
     * `false` scopes the spill to additive light + fire ink — the policy
     * escape if veils over neighbouring panes read badly; the band then
     * intentionally diverges from the in-frame veil pixels at the clip line.
     * Applies from the next `render()`.
     */
    set_spill_include_veils(on: boolean): void;
    /**
     * Inject a broad-coverage SYMBOL fallback face from font bytes, so symbol
     * glyphs the primary + fallback faces lack render real shapes instead of
     * tofu. The byte-injection sibling of the config `symbol_font` path: the host
     * supplies the OS symbol bytes (the canvas can't read the filesystem).
     * No-throw: a bad blob surfaces a catchable JS exception and leaves the
     * existing face untouched.
     */
    set_symbol_font(bytes: Uint8Array): void;
    /**
     * [`AtermTerminal::set_symbol_font`] from a registered handle.
     */
    set_symbol_font_registered(handle: number): void;
    /**
     * Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB), so a host
     * theme change re-themes the pane without rebuilding it. Per-cell SGR colours
     * flow independently; pair with set_palette_color for the ANSI palette.
     */
    set_theme(fg: number, bg: number, cursor: number, selection: number): void;
    /**
     * Override the characters that BREAK a double-click word (the host's
     * word-separator setting, xterm.js `wordSeparators` semantics): a word
     * becomes a maximal run of NON-separator characters. `undefined` restores
     * the engine's default class-based word logic (alphanumeric + `_`)
     * exactly. Smart-selection RULES (url/file_path/email/…) still take
     * precedence for both `selection_word` and `link_at`; the separators only
     * shape the plain-word fallback.
     */
    set_word_separators(separators?: string | null): void;
    /**
     * Byte length of the spill buffer (`0` at 0/0 chrome — the identity law:
     * no band, no bytes, no per-frame cost).
     */
    spill_len(): number;
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
     */
    spill_ptr(): number;
    /**
     * Number of dirty rects from the LAST `render()` (0 on a no-change
     * frame). Read together with [`spill_rects_ptr`](Self::spill_rects_ptr).
     */
    spill_rect_count(): number;
    /**
     * Byte offset (in wasm linear memory) of the packed dirty-rect array:
     * `spill_rect_count()` rects of 4 `i32`s — `x, y, w, h`, FRAME-ABSOLUTE
     * device px. Same read discipline as [`rgba_ptr`](Self::rgba_ptr):
     * consume synchronously after `render()`, never cache the JS view.
     */
    spill_rects_ptr(): number;
    /**
     * Monotone revision of the spill-band content: advances ONLY when the
     * exported bytes changed. Typing-only frames with a settled (or
     * grid-interior) glow, idle re-renders, and 0/0 chrome keep it still —
     * an unchanged value is the engine's word that the host may skip its
     * blit without reading a single spill byte.
     */
    spill_rev(): number;
    /**
     * Drain the missing-font CLASS bits (1 = text/mono fallback, 2 = colour
     * emoji) accumulated by renders since the last call. The host polls this
     * after a frame and lazily injects ONLY the face class actually missed —
     * an ASCII-only session never pays the multi-hundred-MB emoji/CJK payload.
     * Latch per class host-side: a bit can re-fire if the injected faces still
     * miss a char.
     */
    take_missing_font_classes(): number;
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
     * BUILD-truth tier capabilities of this wasm module as one JSON object:
     * `{"coldCodec":"lz4"|"zstd","diskSpill":bool}`. Constant per build —
     * the host brands budget math/telemetry with it instead of assuming.
     */
    static tier_capabilities_json(): string;
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
     * The chrome top head band set via [`Self::set_chrome`] (px; 0 = none).
     */
    readonly chrome_head: number;
    /**
     * The chrome interior padding set via [`Self::set_chrome`] (px; 0 = exact fit).
     * Hosts read these back so canvas offsets and pointer math share one truth.
     */
    readonly chrome_pad: number;
    /**
     * The LIVE application cursor colour (OSC 12) as packed `0x00RRGGBB`, or
     * `undefined` while unset / after an OSC 112 reset — i.e. the host/theme
     * default applies. Read per frame so glow/trail colour derivation can
     * follow app-driven cursor-colour changes (the renderer already draws
     * the cursor itself with this colour).
     */
    readonly cursor_color: number | undefined;
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
     * True when DEC private mode 1007 (alternate scroll) is set: while the
     * alternate screen is active and mouse tracking is off, the host converts
     * wheel ticks into arrow-key presses (aterm-gui's WheelPlan behaviour) so
     * TUIs without mouse support (less, man, plain vim) still wheel-scroll.
     */
    readonly is_alternate_scroll: boolean;
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
     * The live `Terminal::keyboard_mode()` as its raw bitflags value, for
     * hosts that run the engine in a Web Worker: mirror these bits into the
     * main-thread engine-state snapshot and feed them to the free
     * [`encode_key_with_mode`], which encodes keydowns synchronously without
     * an instance. `KeyboardMode` is a `bitflags` struct over `u16` (bits
     * 0..=14 defined); the value is zero-extended to `u32` for headroom.
     */
    readonly keyboard_mode_bits: number;
    /**
     * Whether PHOSPHOR matrix rain is enabled.
     */
    readonly matrix_rain_enabled: boolean;
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
     * True while a deferred scrollback rewrap is stashed (deep history is
     * temporarily detached: only the ring is visible/searchable; a partly
     * stepped job holds its progress here between pumps). The host should
     * keep scheduling [`Self::pump_reflow`] while this is set.
     */
    readonly reflow_pending: boolean;
    /**
     * The SIGNED device-px band shift the next `render()` presents for the
     * banked residual (negative = band shifted DOWN, toward older). Exposed
     * so hosts/harnesses can assert the CPU and GPU bundles present the same
     * sub-row frame.
     */
    readonly scroll_frac_px: number;
    /**
     * The banked sub-row residual in ROWS — signed, in `(-1.0, 1.0)`,
     * positive = partway toward OLDER lines. `0` whenever the viewport is
     * row-aligned (after a flip, a whole-row navigation, or at a clamped
     * history end).
     */
    readonly scroll_frac_rows: number;
    /**
     * Absolute row of display row 0 at the live bottom (`display_offset == 0`):
     * `oldest_absolute_row + scrollback_lines`. A match at absolute `line` is at
     * display row `line - origin + display_offset`, so the host computes the
     * on-screen cell of any [`AtermTerminal::search`] match without a round-trip.
     */
    readonly search_display_origin: number;
    /**
     * Lexicon build diagnostics (v3 §6), newline-joined — one warning per
     * line for every user/custom surface that can never scan as written
     * (single-char CJK without the `cjk_single_char` opt-in, mixed-script /
     * multi-word) or collides across classes; the same warnings the native
     * resolver logs. Empty string while sparkle words are off or the lexicon
     * is clean. Filtered by the current knobs: a "requires cjk_single_char =
     * true" warning disappears once `set_sparkle_feline` enables the opt-in.
     */
    readonly sparkle_lexicon_warnings: string;
    /**
     * Whether the sparkle-words master is currently on.
     */
    readonly sparkle_words_enabled: boolean;
    /**
     * Last-rendered framebuffer width in pixels.
     */
    readonly width: number;
}

/**
 * One slice of a budgeted search ([`AtermTerminal::search_budgeted`]).
 */
export class BudgetedSearchResult {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Whether every retained row has been scanned and every match delta has
     * been delivered. Dense searches can have `rows_fed == total_rows` while
     * this remains false for bounded backlog-drain turns.
     */
    readonly complete: boolean;
    /**
     * Token to resume with; `undefined` once complete.
     */
    readonly cursor: bigint | undefined;
    /**
     * True when the results may be truncated: index eviction dropped old
     * rows, or the engine's match cap was reached. (The legacy
     * [`AtermTerminal::search`] export silently drops this signal.)
     */
    readonly incomplete_index: boolean;
    /**
     * Final oldest absolute line retained by the completed search index. The
     * deterministic eviction schedule makes this stable from the first turn.
     * When nonzero with `incomplete_index`, older history was evicted; a zero
     * watermark with `incomplete_index` indicates match-cap-only truncation.
     */
    readonly lowest_retained_line: number;
    /**
     * Stable match DELTA as flat `[abs_line, start_col, len]` triplets (same
     * coordinate contract as [`AtermTerminal::search`]). Append across calls;
     * already-reported matches keep their order and positions.
     */
    readonly matches: Uint32Array;
    /**
     * Whether this step starts a new logical result stream. Clear previously
     * accumulated match deltas before appending this step when true.
     */
    readonly reset: boolean;
    /**
     * Rows scanned so far (progress numerator; restarts reset it).
     */
    readonly rows_fed: number;
    /**
     * Stable identity for the logical search, including its completing step;
     * `undefined` only for an empty/invalid query result.
     */
    readonly search_id: bigint | undefined;
    /**
     * Total rows this search will scan (progress denominator).
     */
    readonly total_rows: number;
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
 * Metadata for a legacy-contract search ([`AtermTerminal::search_meta`]).
 */
export class SearchMeta {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * True when the results may be truncated: index eviction dropped old rows
     * before they could be searched, or the engine's match cap was reached.
     */
    readonly incomplete: boolean;
    /**
     * Number of matches the paired [`AtermTerminal::search`] call returns
     * (i.e. its flat triplet array length / 3), after any cap.
     */
    readonly match_count: number;
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
 */
export function encode_key_with_mode(key: string, mods: number, event_type: number, base_layout_key: string | null | undefined, mode_bits: number): Uint8Array | undefined;

/**
 * Register a font blob for handle-based reuse by every engine in this module.
 * Content-interned: registering identical bytes returns a handle to ONE shared
 * copy (and re-registration returns the same storage, so handles stay cheap).
 */
export function register_font(bytes: Uint8Array): number;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_atermterminal_free: (a: number, b: number) => void;
    readonly __wbg_budgetedsearchresult_free: (a: number, b: number) => void;
    readonly __wbg_linkhit_free: (a: number, b: number) => void;
    readonly __wbg_searchmeta_free: (a: number, b: number) => void;
    readonly __wbg_selectionrange_free: (a: number, b: number) => void;
    readonly atermterminal_add_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_add_fallback_font_registered: (a: number, b: number) => [number, number];
    readonly atermterminal_advance_effects: (a: number, b: number) => void;
    readonly atermterminal_authorize_clipboard_write: (a: number) => void;
    readonly atermterminal_authorize_hyperlink_scheme: (a: number, b: number, c: number) => number;
    readonly atermterminal_authorize_notifications: (a: number, b: number) => void;
    readonly atermterminal_base_y: (a: number) => number;
    readonly atermterminal_bracketed_paste_mode: (a: number) => number;
    readonly atermterminal_cell_height: (a: number) => number;
    readonly atermterminal_cell_is_wide: (a: number, b: number, c: number) => number;
    readonly atermterminal_cell_text: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_cell_width: (a: number) => number;
    readonly atermterminal_chrome_head: (a: number) => number;
    readonly atermterminal_chrome_pad: (a: number) => number;
    readonly atermterminal_cursor_color: (a: number) => number;
    readonly atermterminal_cursor_style: (a: number) => number;
    readonly atermterminal_cursor_x: (a: number) => number;
    readonly atermterminal_cursor_y: (a: number) => number;
    readonly atermterminal_display_offset: (a: number) => number;
    readonly atermterminal_display_origin_absolute: (a: number) => number;
    readonly atermterminal_drain_bell: (a: number) => number;
    readonly atermterminal_drain_scrollback_backlog: (a: number, b: number) => number;
    readonly atermterminal_effects_next_deadline_ms: (a: number) => [number, number];
    readonly atermterminal_encode_key: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly atermterminal_encode_mouse_motion: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_encode_mouse_press: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_encode_mouse_release: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_encode_mouse_wheel: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_height: (a: number) => number;
    readonly atermterminal_is_alt_screen: (a: number) => number;
    readonly atermterminal_is_alternate_scroll: (a: number) => number;
    readonly atermterminal_is_app_cursor_mode: (a: number) => number;
    readonly atermterminal_is_color_scheme_updates_mode: (a: number) => number;
    readonly atermterminal_is_effects_active: (a: number) => number;
    readonly atermterminal_is_focus_event_mode: (a: number) => number;
    readonly atermterminal_is_mouse_tracking: (a: number) => number;
    readonly atermterminal_keyboard_mode_bits: (a: number) => number;
    readonly atermterminal_last_command_output: (a: number) => [number, number];
    readonly atermterminal_link_at: (a: number, b: number, c: number) => number;
    readonly atermterminal_matrix_rain_enabled: (a: number) => number;
    readonly atermterminal_mouse_wants_any_motion: (a: number) => number;
    readonly atermterminal_mouse_wants_motion: (a: number) => number;
    readonly atermterminal_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number];
    readonly atermterminal_new_registered: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number];
    readonly atermterminal_note_keystroke: (a: number) => void;
    readonly atermterminal_note_matrix_rain_alt_scroll: (a: number) => void;
    readonly atermterminal_note_matrix_rain_bell: (a: number) => void;
    readonly atermterminal_note_matrix_rain_signal: (a: number, b: number, c: number) => void;
    readonly atermterminal_predict_backspace: (a: number) => number;
    readonly atermterminal_predict_char: (a: number, b: number) => number;
    readonly atermterminal_predict_line_submit: (a: number) => void;
    readonly atermterminal_predict_next_deadline_ms: (a: number) => [number, number];
    readonly atermterminal_predict_overlay: (a: number) => [number, number];
    readonly atermterminal_predict_reconcile: (a: number) => void;
    readonly atermterminal_predict_reset: (a: number) => void;
    readonly atermterminal_predict_session_reset: (a: number) => void;
    readonly atermterminal_present_band_count: (a: number) => number;
    readonly atermterminal_present_bands_ptr: (a: number) => number;
    readonly atermterminal_process: (a: number, b: number, c: number) => void;
    readonly atermterminal_process_str: (a: number, b: number, c: number) => void;
    readonly atermterminal_pump_reflow: (a: number) => number;
    readonly atermterminal_pump_reflow_budget: (a: number, b: number) => void;
    readonly atermterminal_reflow_pending: (a: number) => number;
    readonly atermterminal_render: (a: number) => void;
    readonly atermterminal_resize: (a: number, b: number, c: number) => void;
    readonly atermterminal_revoke_clipboard_write: (a: number) => void;
    readonly atermterminal_revoke_hyperlink_scheme: (a: number, b: number, c: number) => void;
    readonly atermterminal_rgba: (a: number) => [number, number];
    readonly atermterminal_rgba_ptr: (a: number) => number;
    readonly atermterminal_row_is_wrapped: (a: number, b: number) => number;
    readonly atermterminal_row_len: (a: number, b: number) => number;
    readonly atermterminal_row_text: (a: number, b: number) => [number, number];
    readonly atermterminal_scroll_frac_px: (a: number) => number;
    readonly atermterminal_scroll_frac_rows: (a: number) => number;
    readonly atermterminal_scroll_lines: (a: number, b: number) => void;
    readonly atermterminal_scroll_lines_frac: (a: number, b: number) => void;
    readonly atermterminal_scroll_px: (a: number, b: number) => void;
    readonly atermterminal_scroll_search_line_into_view: (a: number, b: number) => void;
    readonly atermterminal_scroll_to_bottom: (a: number) => void;
    readonly atermterminal_scroll_to_top: (a: number) => void;
    readonly atermterminal_scrollback_backlog_lines: (a: number) => number;
    readonly atermterminal_scrollback_budget_effective: (a: number) => number;
    readonly atermterminal_scrollback_memory_used: (a: number) => number;
    readonly atermterminal_scrollback_pressure: (a: number) => number;
    readonly atermterminal_scrollback_truncated_lines: (a: number) => number;
    readonly atermterminal_search: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly atermterminal_search_budgeted: (a: number, b: number, c: number, d: number, e: number, f: number, g: bigint, h: number) => number;
    readonly atermterminal_search_budgeted_cancel: (a: number) => void;
    readonly atermterminal_search_display_origin: (a: number) => number;
    readonly atermterminal_search_meta: (a: number, b: number, c: number, d: number, e: number) => number;
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
    readonly atermterminal_set_background_opacity: (a: number, b: number) => void;
    readonly atermterminal_set_bold_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_bold_font_registered: (a: number, b: number) => [number, number];
    readonly atermterminal_set_cell_pixel_size: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_chrome: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_color_scheme: (a: number, b: number) => void;
    readonly atermterminal_set_cursor_blink_phase: (a: number, b: number) => void;
    readonly atermterminal_set_cursor_glow: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number) => void;
    readonly atermterminal_set_cursor_hollow: (a: number, b: number) => void;
    readonly atermterminal_set_cursor_opacity: (a: number, b: number) => void;
    readonly atermterminal_set_cursor_trail: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_set_cursor_trail_pack: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_default_background: (a: number, b: number, c: number, d: number) => void;
    readonly atermterminal_set_default_cursor_style: (a: number, b: number) => void;
    readonly atermterminal_set_default_foreground: (a: number, b: number, c: number, d: number) => void;
    readonly atermterminal_set_effects_focused: (a: number, b: number) => void;
    readonly atermterminal_set_effects_visibility: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_emoji_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_emoji_font_registered: (a: number, b: number) => [number, number];
    readonly atermterminal_set_fallback_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_fallback_font_registered: (a: number, b: number) => [number, number];
    readonly atermterminal_set_font_features: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_kitty_keyboard_enabled: (a: number, b: number) => void;
    readonly atermterminal_set_ligatures: (a: number, b: number) => void;
    readonly atermterminal_set_line_height: (a: number, b: number) => void;
    readonly atermterminal_set_matrix_rain: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number, n: number, o: number, p: number, q: bigint) => void;
    readonly atermterminal_set_matrix_rain_enabled: (a: number, b: number) => void;
    readonly atermterminal_set_matrix_rain_reduced_motion: (a: number, b: number) => void;
    readonly atermterminal_set_minimum_contrast: (a: number, b: number) => void;
    readonly atermterminal_set_palette_color: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_set_predictive_echo: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_primary_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_px: (a: number, b: number) => void;
    readonly atermterminal_set_scrollback_budget: (a: number, b: number) => void;
    readonly atermterminal_set_scrollback_global_budget: (a: number) => void;
    readonly atermterminal_set_scrollback_limit: (a: number, b: number) => void;
    readonly atermterminal_set_selection_fg: (a: number, b: number) => void;
    readonly atermterminal_set_selection_inactive: (a: number, b: number) => void;
    readonly atermterminal_set_selection_inactive_bg: (a: number, b: number) => void;
    readonly atermterminal_set_sparkle_alt_screen_suppression: (a: number, b: number) => void;
    readonly atermterminal_set_sparkle_classes: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_set_sparkle_custom_specs: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_sparkle_deny: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_sparkle_feline: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => void;
    readonly atermterminal_set_sparkle_ink: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_set_sparkle_languages: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_sparkle_lexicon_override: (a: number, b: number, c: number) => void;
    readonly atermterminal_set_sparkle_profanity: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => void;
    readonly atermterminal_set_sparkle_reduced_motion: (a: number, b: number) => void;
    readonly atermterminal_set_sparkle_words_enabled: (a: number, b: number) => void;
    readonly atermterminal_set_spill_include_veils: (a: number, b: number) => void;
    readonly atermterminal_set_symbol_font: (a: number, b: number, c: number) => [number, number];
    readonly atermterminal_set_symbol_font_registered: (a: number, b: number) => [number, number];
    readonly atermterminal_set_theme: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly atermterminal_set_word_separators: (a: number, b: number, c: number) => void;
    readonly atermterminal_sparkle_lexicon_warnings: (a: number) => [number, number];
    readonly atermterminal_sparkle_words_enabled: (a: number) => number;
    readonly atermterminal_spill_len: (a: number) => number;
    readonly atermterminal_spill_ptr: (a: number) => number;
    readonly atermterminal_spill_rect_count: (a: number) => number;
    readonly atermterminal_spill_rects_ptr: (a: number) => number;
    readonly atermterminal_spill_rev: (a: number) => number;
    readonly atermterminal_take_missing_font_classes: (a: number) => number;
    readonly atermterminal_take_notifications: (a: number) => [number, number];
    readonly atermterminal_take_osc_events: (a: number) => [number, number];
    readonly atermterminal_take_response: (a: number) => [number, number];
    readonly atermterminal_tier_capabilities_json: () => [number, number];
    readonly atermterminal_title: (a: number) => [number, number];
    readonly atermterminal_width: (a: number) => number;
    readonly budgetedsearchresult_complete: (a: number) => number;
    readonly budgetedsearchresult_cursor: (a: number) => [number, bigint];
    readonly budgetedsearchresult_incomplete_index: (a: number) => number;
    readonly budgetedsearchresult_lowest_retained_line: (a: number) => number;
    readonly budgetedsearchresult_matches: (a: number) => [number, number];
    readonly budgetedsearchresult_reset: (a: number) => number;
    readonly budgetedsearchresult_rows_fed: (a: number) => number;
    readonly budgetedsearchresult_search_id: (a: number) => [number, bigint];
    readonly budgetedsearchresult_total_rows: (a: number) => number;
    readonly encode_key_with_mode: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly linkhit_end_col: (a: number) => number;
    readonly linkhit_kind: (a: number) => number;
    readonly linkhit_start_col: (a: number) => number;
    readonly linkhit_url: (a: number) => [number, number];
    readonly register_font: (a: number, b: number) => number;
    readonly searchmeta_incomplete: (a: number) => number;
    readonly searchmeta_match_count: (a: number) => number;
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
