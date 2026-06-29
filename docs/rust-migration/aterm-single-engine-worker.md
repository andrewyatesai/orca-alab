# aterm single-engine-in-worker (off-main parse + render, default-on)

Status: **in progress** (replaces the opt-in "render mirror"). Goal: move the aterm
engine **entirely** into a Web Worker so VT parsing AND rasterization run off the
renderer main thread, with **one** engine per pane (no duplicate), then make it the
default. This supersedes `aterm-worker-mirror.ts`, which ran a *second* full engine
on the main thread (double parse + 2× memory) purely to answer the synchronous
facade query API — the reason it could never be defaulted on.

## Why the render mirror was the wrong shape

`aterm-worker-mirror.ts` kept a full main-thread engine and fed every PTY byte to
*both* it and the worker (`process_str` proxy calls `target.process_str(s)` **and**
`post({type:'process'})`). Rendering is already coalesced to ≤1 frame and dirty-row
bounded — cheap. The expensive, flood-dominating work is **parsing**, which the
mirror kept on main *and* duplicated in the worker. So it offloaded the cheap half,
doubled the expensive half, doubled memory, and risked drift (theme/line-height/
cursor/search mutations weren't mirrored). That 2× cost is exactly why it stayed
default-off. The fix is one engine, in the worker.

## The seam: `AtermPaneController` / the wasm `term` shape

`wireAtermPane` (`aterm-pane-wiring.ts`) binds the entire controller + every input
handler + the buffer shim + a11y + search directly to `pending.term` (the wasm
`AtermTerminal` shape; CPU and GPU expose the same surface). So the async boundary
is **`term` itself**. We replace `pending.term` with a **worker-backed `term`** that:

- answers **reads** synchronously from the latest **state snapshot** the worker
  pushes each frame, and
- **posts mutations** to the worker (the sole engine).

Because the worker-backed `term` mimics the wasm method shape, `wireAtermPane`, the
controller (`aterm-engine-reads`, theme mutators, reply surface), the facade, the
buffer shim, a11y, and all ~46 facade consumers bind to it **unchanged**. Only the
handful of methods that cannot be faithfully synchronous over a remote engine force
a localized change (see "Forced changes").

## What the worker owns (it becomes the terminal)

Everything that touches the engine moves into the worker:

- **Parse + render** (CPU rasterize→blit, or GPU present to the OffscreenCanvas
  swapchain — already implemented in `aterm-render-worker.ts`).
- **Follow-bottom** ("scroll to bottom on new output only if already at bottom").
- **Side-channel drains**, posted to main after each processed chunk:
  - `take_response()` → reply bytes → main writes to the PTY (`inputSink`).
  - `take_osc_events()` → OSC app-events → main dispatches to orca's handlers.
  - `drain_bell()` → main re-emits the bell.
  - title change (OSC 0/2) → main re-emits.
  - alt-screen flip → main fires `onBufferChange`.
- **Search** (`term.search` + find/next/prev/clear state machine): worker runs it,
  renders highlights, and posts count + active index + active-match device rect.
- **Cursor blink**: worker runs the ~530ms timer keyed on focus (main posts
  focus/blur) and renders steady/blinking/hollow accordingly.
- **Selection**: `selection_start/extend/finish/word/line/clear` become posted
  commands; the worker renders the highlight and pushes `selection_range` (always)
  + `selection_text` (on change).
- **Link detection**: `link_at` answered from snapshot-pushed **visible link
  spans**; the worker renders the hover underline from a posted hover position.

The main thread keeps: PTY IPC, DOM (canvas/textarea/liveRegion/overlay), key &
paste encoding, pointer routing, and the synchronous snapshot read surface.

## State snapshot (worker → main, each frame)

Scalars (cheap, every frame): `cellWidth, cellHeight, width, height, displayOffset,
cursorX, cursorY, cursorStyle, baseY, displayOriginAbsolute, isAltScreen,
bracketedPasteMode, isMouseTracking, mouseWantsMotion, mouseWantsAnyMotion,
isFocusEventMode, isColorSchemeUpdatesMode, title, rendererKind, adapterInfo,
isReady, searchCount, searchActiveIndex, searchActiveRect`.

On-change (not every frame): the **visible grid** as dirty rows `{row, text,
wrapped, len, wideCols}` (every content read the controller serves is a *visible*
display row — the buffer shim returns `undefined` for off-screen rows), the
**selection** `{range, text, inactive}`, and the **visible link spans**
`[{row, startCol, endCol, url, kind}]`.

Main keeps a rolling mirror of the visible grid patched by each push, so
`row_text/cell_text/row_len/row_is_wrapped/cell_is_wide/link_at/selection_text`
answer synchronously from it.

## Async (worker round-trip, id-correlated query channel)

Only cold reads that touch **off-screen history** (the snapshot carries the visible
grid, so visible-row reads stay synchronous):

- `serialize(scrollbackRows)` / `serializeScrollback(maxRows)` — full history;
  called from async snapshot/save/restore/fork/layout-persist lifecycle code (~8
  sites). The one **non-awaitable** site is shutdown layout capture
  (`terminal-shutdown-layout-capture.ts`, called sync at unmount): the worker pushes
  a **debounced cached serialized blob** that main reads synchronously at teardown
  (accept minor staleness) — no SAB, no await.

## Decisions taken (vs the survey synthesis)

- **Snapshot-back the visible grid, don't make content reads async.** The synthesis
  preferred making ~37 reads async to shrink the per-frame payload. We instead push
  the visible grid (dirty rows only) so the buffer shim, a11y mirror, xterm-style
  link click fallbacks, `selection_text`, and `link_at` keep working **synchronously
  and unchanged**. This collapses the async surface to ~8 serialize sites and avoids
  any link/selection behavior change — lower risk on the path every pane hits. Cost:
  bounded per-frame dirty-row push (≈visible grid, only on change).
- **No SharedArrayBuffer.** The synthesis recommended an SAB scalar block + `Atomics`
  to kill read-after-write staleness. We decline it: `cols`/`rows` are
  **main-authoritative** in aterm (computed by `computeGrid`, stored in the wiring;
  `gridSize()` returns those, not the engine) → no staleness; viewport scalars
  (`baseY`/`displayOffset`) tolerate ≤1 frame (scrollbar) or move into the worker
  (follow-bottom); cell metrics after `set_px` re-reflow when the snapshot's
  `cellWidth` changes (rare path). SAB needs cross-origin isolation (COOP/COEP) —
  a broad-blast-radius change to the renderer (resource loading, Electron APIs) that
  is not worth a marginal exactness gain. The first metrics arrive by **awaiting the
  worker's first `state`** in the async loader before building the controller.

## Forced changes (cannot be faithfully synchronous over a remote engine)

- `aterm-mouse-input.ts`: `encode_mouse_*` returns PTY bytes synchronously today.
  Keep the synchronous `preventDefault`/gate decision (uses snapshot
  `isMouseTracking`/`mouseWantsMotion`), but post the event to the worker for
  encoding and forward the returned bytes to the PTY (~1–2 ms, imperceptible).
- `aterm-selection-input.ts`: double/triple-click `selection_word/line` returns
  text for copy-on-select — post the command, await the text for the copy.
- `aterm-search-api.ts` + search UI: `findMatches` returns a count — make the UI
  path await (Cmd+F tolerates it); the active-match rect is read from the snapshot.
- serialize call sites (TBD from survey synthesis) — `await` the async serialize.

## Staging (each stage independently correct, default-OFF until E)

- **A** — extend the protocol; worker owns the terminal (search/blink/follow-bottom/
  side-channel drains/rich snapshot). No main-side behavior change yet (still the
  duplicate-engine mirror feeds it).
- **B** — worker-backed `term` (snapshot reads + posted mutations) **replaces** the
  main-thread engine in the mirror loader → single engine. Delete `buildMainEngine`.
- **C** — async `serialize`/`serializeScrollback` at their lifecycle call sites.
- **D** — side-channel push wiring (reply→PTY, OSC, bell, title, selection-change,
  buffer-change driven by worker events) + the forced input round-trips.
- **E** — e2e validation at a real (non-1×1) grid; then flip the default in
  `aterm-strategy-select.ts` (GPU-in-worker for GPU-capable panes — no regression
  vs the prior main-thread GPU default; CPU-in-worker otherwise).

## Risks / resolutions

- **Side-channel timing**: replies (CPR/DA/DSR) round-trip; terminals already
  tolerate far larger latency over SSH. Ordering preserved (worker drains in order).
- **Mode-flag staleness**: `isMouseTracking`/`isAltScreen` come from the last
  snapshot; modes flip rarely (app startup), and the window is sub-frame. Acceptable.
- **Copy freshness**: copy-on-select awaits; Cmd+C reads the (by-then-fresh)
  snapshot.
- **Default flip is last** and only where it doesn't regress (GPU panes already had
  main-thread GPU; worker GPU keeps that and frees the main thread).

## External call sites that change (from survey synthesis)

_Pending the `aterm-controller-seam-survey` synthesis — serialize consumers, the
search UI await points, and any stuck-sync content reads outside the aterm dir._
