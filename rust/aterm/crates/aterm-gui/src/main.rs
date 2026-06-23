// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `aterm-gui` — a native windowed aterm terminal.
//!
//! A real window (winit) presenting the `aterm-render` CPU framebuffer over a
//! real `$SHELL` in a PTY. A background thread reads the PTY and feeds the
//! engine; the main thread rasterizes the grid and handles keyboard/resize.
//! Per-cell colours and a GPU path come later; this is the first window you can
//! actually launch and use.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aterm_containment::ContainmentMode;
use aterm_core::bell::{BellFlash, BellRateLimiter};
use aterm_core::selection::{SelectionAnchor, SelectionSide, SelectionState, SelectionType};
use aterm_core::terminal::{CursorStyle, RenderCell, Terminal};
use aterm_render::{Frame, RenderInput, Renderer, Theme, WindowCpu};
use aterm_session::sink::SinkWriter;
use aterm_session::{EdgeTable, LaunchNonce, SessionId};
use winit::application::ApplicationHandler;
use winit::event::{Ime, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{UserAttentionType, Window, WindowId as WinitWindowId};

mod accessibility;
mod app_config;
mod app_input;
mod app_introspect;
mod app_mouse;
mod app_render;
mod app_search;
mod app_tabs;
mod app_window;
mod build_info;
mod cast;
mod cli;
mod config_watcher;
mod control;
mod control_auth;
mod input;
mod keybinding;
mod keymap;
mod logging;
mod menu;
mod metrics;
mod notify;
mod pane;
mod proxy;
mod session_store;
mod snapshot_path;
mod spawn;
mod subscribe;
mod tab_bar;
mod temporal;
mod toolbar;

use app_config::{
    config_path, env_u16, load_config, resolve_font_px, resolve_force_scale, resolve_tab_strip_rows,
};
use app_search::{MAX_SEARCH_HISTORY, SearchState};
use cli::{Cli, parse_cli};
use input::{InputEvent, InputOutcome, Source};
use spawn::{SessionFactory, spawn_session};

// Default glyph rasterization size in PHYSICAL px. On a 2× Retina display the
// HiDPI auto-scale (`resumed`) renders this at `round(13·2)=26` px ≈ 13 logical
// points — close to iTerm2's 12 pt / macOS Terminal's 11 pt, rather than the
// oversized 16 logical pt the old 16 px base produced. Override with
// `$ATERM_FONT_PX`, config `font_px`, or live Cmd +/-/0.
const FONT_PX: f32 = 13.0;

/// Cmd-+/Cmd-- live font-zoom step, in physical px; Cmd-0 resets to the launch size.
const FONT_ZOOM_STEP: f32 = 2.0;
/// Clamp for the live font size (matches the $ATERM_FONT_PX bounds).
const FONT_PX_MIN: f32 = 6.0;
const FONT_PX_MAX: f32 = 200.0;

/// Security allowlist for opening a hyperlink that arrived in (untrusted) program
/// output via Cmd-click: ONLY `http`/`https`/`mailto`. Rejects `file://`, custom
/// app schemes (`x-apple-…`, `tel:`, …), and anything with control bytes or
/// whitespace — so `open` can never be steered into launching an app or reaching
/// the local filesystem from hostile terminal output. (OSC 8 hyperlinks are also
/// already authorization-gated in the engine; this is the second gate, at the
/// point of action.)
fn is_safe_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() || u.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return false;
    }
    let l = u.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:")
}

/// Find a plain-text `http(s)://` URL covering position `col` in a row of chars.
/// Returns `(url, start_col, end_col)` with INCLUSIVE column bounds. Scans for a
/// scheme start, extends over URL-permitted ASCII, trims trailing sentence
/// punctuation (so `(see http://x.com).` yields `http://x.com`), and checks `col`
/// is inside. Pure over `&[char]` so it is unit-testable; `plain_url_at` adapts a
/// `RenderCell` row to it.
fn find_url_span(chars: &[char], col: usize) -> Option<(String, usize, usize)> {
    let n = chars.len();
    let is_url = |c: char| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c);
    let mut i = 0;
    while i < n {
        let s = &chars[i..];
        if s.starts_with(&['h', 't', 't', 'p', ':', '/', '/'])
            || s.starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/'])
        {
            let mut j = i;
            while j < n && is_url(chars[j]) {
                j += 1;
            }
            let mut end = j;
            while end > i
                && matches!(
                    chars[end - 1],
                    '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '\'' | '"'
                )
            {
                end -= 1;
            }
            if (i..end).contains(&col) {
                return Some((chars[i..end].iter().collect(), i, end - 1));
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// Plain-text URL covering cell `col` in a rendered row (one cell = one char;
/// wide-continuation cells read as a space, which breaks a run as it should).
fn plain_url_at(cells: &[RenderCell], col: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<char> = cells
        .iter()
        .map(|c| if c.wide { ' ' } else { c.ch })
        .collect();
    find_url_span(&chars, col)
}

/// (Re)build the [`Backend`] at `px` for live font-zoom. When `use_gpu`, rebuilds
/// the GPU renderer — it renders offscreen and the window's GPU surface is
/// re-created separately by `on_resize`, so re-creating the renderer mid-session
/// is the same proven call as startup. A GPU failure or a missing system font
/// yields `None`, and the caller keeps the current backend (zoom is a no-op
/// rather than a crash).
fn build_backend(px: f32, use_gpu: bool, theme: Theme, family: Option<&str>) -> Option<Backend> {
    if use_gpu && let Ok(g) = aterm_gpu::GpuRenderer::new_with_family(family, px, theme) {
        return Some(Backend::Gpu(g));
    }
    Renderer::from_system_with_family(family, px, theme).map(Backend::Cpu)
}

/// Interior padding at scale 1.0 (logical), in px: the breathing room between the
/// window edge and the grid. Scaled by the display scale factor so it stays a
/// constant LOGICAL size on HiDPI (≈8 device px at 1×, 16 at 2× — `round(8·scale)`).
/// See [`Backend::set_pad`] / the `pad` renderer field.
/// 10 px: COMPACT, in Ghostty/iTerm2 territory, but a hair more than a flush 8 px so
/// text isn't pinned to the window edge — the one persistent visual-judge complaint
/// (codex+claude both flagged "left/top padding too tight" twice). An earlier loop had
/// over-corrected to 22 px (wasteful next to Ghostty's tight modern look); 10 px keeps
/// the compact target while giving the grid a little breathing room. (Override-free.)
const PAD_LOGICAL_PX: f32 = 10.0;

/// The interior padding (device px per edge) for a display `scale` factor:
/// `round(8 · scale)`, clamped non-negative. A single source so the window-create
/// path (before the window's scale is known — uses 1.0) and `resumed` agree.
fn pad_for_scale(scale: f64) -> usize {
    (PAD_LOGICAL_PX * scale as f32).round().max(0.0) as usize
}

/// Half-period of the cursor blink: a `Blinking*` DECSCUSR cursor toggles
/// on/off every this long (the classic terminal cadence).
const BLINK_INTERVAL: Duration = Duration::from_millis(530);

/// Multi-click window: a left press within this many milliseconds of the
/// previous press, in the SAME cell, advances the click count (1 -> 2 -> 3,
/// then wraps back to 1).
const MULTI_CLICK_MS: u128 = 500;

/// Audible-bell floor: at most one beep per this interval, however fast BEL
/// floods (the visual flash still re-arms on every bell).
const BELL_BEEP_INTERVAL: Duration = Duration::from_secs(1);

/// Lock the shared [`Terminal`], recovering from a poisoned mutex.
///
/// A panic on the renderer/PTY-reader/control thread poisons this lock; the
/// previous `.unwrap()` then panicked every other thread that touched it and
/// killed the whole session. A possibly mid-update grid beats process death
/// here: the next `process()`/redraw pass repaints a consistent screen while
/// the user's foreground job keeps running. Warns once — not per acquisition;
/// the lock stays poisoned forever — so recovery is visible without flooding.
pub(crate) fn term_lock(term: &Mutex<Terminal>) -> MutexGuard<'_, Terminal> {
    term.lock().unwrap_or_else(|poisoned| {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            aterm_log::warn!("Terminal mutex poisoned by a panicked thread; recovering");
        }
        poisoned.into_inner()
    })
}

/// Origin unit of a double/triple-click gesture: the word (or line) the
/// gesture started on, kept while the button stays down so a drag extends the
/// selection by whole words/lines with the origin unit always fully selected.
#[derive(Debug, Clone, Copy)]
struct GestureOrigin {
    /// Live-screen selection row of the original word/line.
    row: i32,
    /// Inclusive first column of the original unit (0 for a line).
    start_col: u16,
    /// Inclusive last column of the original unit (`cols-1` for a line).
    end_col: u16,
    /// `Semantic` (double-click, by words) or `Lines` (triple-click, by rows).
    kind: SelectionType,
}

/// A cheap fingerprint of the text selection, for the redraw early-out (D-1).
///
/// The grid's damage tracker does NOT see selection changes (they mutate
/// `Terminal::text_selection`, not the grid), yet the selection IS painted, so
/// a selection change must still force a repaint. Comparing this `Copy`,
/// `PartialEq` fingerprint of the selection's state machine captures exactly
/// that. `SelectionAnchor`/`SelectionState`/`SelectionType` are all `PartialEq`.
#[derive(Clone, Copy, PartialEq)]
struct SelectionFingerprint {
    state: SelectionState,
    kind: SelectionType,
    start: SelectionAnchor,
    end: SelectionAnchor,
}

impl SelectionFingerprint {
    /// Fingerprint a live selection (read from a [`RenderInput`] or a `Terminal`).
    fn of(sel: &aterm_core::selection::TextSelection) -> Self {
        SelectionFingerprint {
            state: sel.state(),
            kind: sel.selection_type(),
            start: sel.start(),
            end: sel.end(),
        }
    }
}

/// Everything that affects the PRESENTED frame which the grid damage tracker
/// does not already cover, plus the grid's own monotonic damage epoch (D-1).
///
/// The redraw early-out compares the key for the current frame against the key
/// recorded at the last real present: if they are equal, nothing the user can
/// see has changed, so the whole extract + rasterize + present is skipped. The
/// `damage_epoch` term covers every grid mutation (writes, scroll, erase,
/// resize); the other terms cover purely-visual state the grid never tracks —
/// the cursor blink phase, the bell-flash invert, the unfocused cursor-style
/// override, and the text selection.
#[derive(Clone, Copy, PartialEq)]
struct RepaintKey {
    /// `Terminal::damage_epoch()` — advances on any net-new grid damage.
    damage_epoch: u64,
    /// Cursor blink phase pushed to the renderer (a flip toggles the cursor).
    blink_phase: bool,
    /// Visual-bell invert active for this frame (toggles the whole frame).
    invert: bool,
    /// Unfocused cursor-style override (hollow block), if any.
    cursor_override: Option<CursorStyle>,
    /// The active text selection fingerprint.
    selection: SelectionFingerprint,
    /// Fingerprint of the VISIBLE tab strip (tab count + active index + the active
    /// tab's title), so a tab switch / open / close / title change repaints the
    /// strip even when the terminal grid below is unchanged. Always `0` when
    /// `tab_strip_rows == 0` — then the key is byte-identical to the pre-strip path.
    tab_strip: u64,
}

/// Wake the UI when the PTY produced output, exited, a screen snapshot was
/// requested (SIGUSR1), or the control socket needs the renderer (`Control`) —
/// the latter two are aterm introspecting itself: it renders its CURRENT live
/// screen to a PNG (pixels) + a .txt (text), so an intelligence can "see"
/// exactly what is on the terminal without any OS screen-recording.
// NOTE: Wake is NO LONGER `Clone`/`Copy` — the `Wake::Input` payload carries a
// `Vec<InputEvent>` (not `Copy`) and an `Option<Sender>` (not `Clone`). Nothing
// clones or copies a `Wake` value: every variant is matched by value in
// `user_event` and moved into `send_event` (which takes `T` by value), so
// dropping the derives is a pure subtraction with no call-site churn.
#[derive(Debug)]
enum Wake {
    /// A session's PTY produced output. `session` is the stable [`Session::id`]
    /// of the tab that produced it, so `user_event` feeds the right engine and
    /// only requests a redraw when that tab is the ACTIVE one. `window` is the
    /// logical window that OWNS the originating tab (stamped at spawn), so the
    /// redraw is routed per-window — every window currently DISPLAYING this
    /// session is redrawn (the owner today; co-viewers in a later step).
    Output {
        session: u64,
        window: WindowId,
    },
    /// A session's PTY hit EOF (its shell/`-e` command exited). `session`
    /// identifies the tab to close; `window` is the logical window that owns it,
    /// so the close is routed to that window. The app exits only when it was the
    /// LAST tab (and `--hold` keeps even the last tab's window open). With one
    /// tab this is exactly the old single-session "close the app" behavior.
    Exit {
        session: u64,
        window: WindowId,
    },
    Snapshot,
    /// The engine saw BEL (0x07) on a session: flash the frame, beep
    /// (rate-limited), and request user attention when the window is unfocused.
    /// `session` is the originating tab; `window` is the logical window that owns
    /// it, so the flash/attention is routed to that window. A background tab's
    /// bell still flashes the owning window — the "bell on activity" affordance.
    Bell {
        session: u64,
        window: WindowId,
    },
    /// The control thread queued one or more `ImageReq`s and needs the main
    /// thread (which owns the renderer) to render and reply.
    Control,
    /// The `chrome` introspection verb needs the main thread to read the
    /// frontmost window's NATIVE macOS UI — its `NSToolbar` items and the app
    /// menu bar — which only the main thread may touch (AppKit objects are
    /// main-thread-only). The control thread builds the one-shot `reply` channel,
    /// posts this, and blocks; the main thread reads the chrome into a `Vec` of
    /// text lines (one per toolbar item / menu) and sends it back. Mirrors the
    /// `Wake::Control` image round-trip, but for read-only UI introspection. Off
    /// macOS this is never constructed (the verb replies with a no-chrome note on
    /// the control thread), but the variant exists on every target so `Wake` stays
    /// platform-independent.
    ReadChrome {
        reply: std::sync::mpsc::Sender<Vec<String>>,
    },
    /// The user edited `~/.config/aterm/aterm.toml` (the config-watcher thread
    /// saw its mtime change). The main thread — the SOLE owner of the renderer,
    /// window, and per-tab engines — re-reads + validates the file and applies
    /// the new font/theme/engine config to every live session (see
    /// [`App::reload_config`]). A malformed mid-edit file is rejected with a
    /// warning, leaving the previous config intact.
    ConfigReload,
    /// Phase 0.5 (A.2.3): a control verb built one or more engine-neutral
    /// [`InputEvent`]s and needs the main thread — the SOLE owner of term
    /// geometry + gesture state + the encoders — to apply them via
    /// [`App::input`]. A whole controller gesture (press -> move -> release) is
    /// ONE `batch` so it applies ATOMICALLY in a single main-loop turn (no
    /// foreign `Wake::Output` redraw interleaves mid-gesture). `reply` is `Some`
    /// only for verbs that must report success (resize range-reject, a refused
    /// scroll); `None` = fire-and-forget.
    ///
    /// RES-1 NOTE: the control `resize` verb used to post a dedicated
    /// `Wake::Resize`; it is now just an `InputEvent::Resize` in this one channel,
    /// applied (with the term+PTY+window+framebuffer update + redraw) by the seam's
    /// `Resize` arm — the SOLE geometry owner is still the main thread.
    Input {
        batch: Vec<InputEvent>,
        src: Source,
        reply: Option<std::sync::mpsc::Sender<InputOutcome>>,
    },
    /// A macOS menu-bar item was clicked (see `menu.rs`). The item's action
    /// target posts this off the AppKit menu-tracking call; `user_event`
    /// dispatches it on the main loop turn via `App::dispatch_menu_action`, which
    /// calls the SAME `App` command method the matching keybinding uses (no
    /// behavior duplication). Always carries a decoded [`menu::MenuAction`] (the
    /// target ignores any item whose tag doesn't decode), so this is never a
    /// no-op variant. On non-macOS targets this variant exists but is never
    /// constructed (no platform menu), keeping `Wake` platform-independent.
    MenuAction {
        action: menu::MenuAction,
    },
    /// Open a new IN-PROCESS window (Cmd-N / Window ▸ New Window from `on_key`,
    /// which has no `ActiveEventLoop` to create a window itself). Posted onto the
    /// loop so `user_event` — which DOES have `el` — runs `create_window_internal`.
    /// Under headless this is IGNORED (one logical window only); the menu/key path
    /// in `dispatch_menu_action` already has `el` and calls `create_window_internal`
    /// directly, so this variant exists for the keyboard path.
    CreateWindow,
    /// "Move Tab to New Window" (Cmd-Shift-N / Window ▸ Move Tab to New Window) from
    /// the keyboard path (which has no `ActiveEventLoop` to attach a new OS window).
    /// Posted onto the loop so `user_event` — which DOES have `el` — runs
    /// `detach_active_tab`, moving the frontmost window's active tab out into a fresh
    /// in-process window. A single-tab source is a no-op; headless never attaches an
    /// OS surface (the logical move still applies). The menu path in
    /// `dispatch_menu_action` already has `el` and calls `detach_active_tab` directly.
    DetachActiveTab,
    /// "Open Active Session in New Window" (Cmd-Shift-O / Window ▸ Open Session in New
    /// Window) from the keyboard path (which has no `ActiveEventLoop` to attach a new
    /// OS window). Posted onto the loop so `user_event` — which DOES have `el` — runs
    /// `open_active_session_in_new_window`, ADDING a second window that views the
    /// frontmost window's active session (same live grid in two windows). Under
    /// headless the logical attach still applies but no OS surface is attached. The
    /// menu path in `dispatch_menu_action` already has `el` and calls it directly.
    ViewActiveSessionInNewWindow,
    /// A native tab was selected — either the window toolbar's
    /// [`NSSegmentedControl`](crate::toolbar) (a click on segment `index` of
    /// `window`) or the control socket's `tab <N>` verb. Posted onto the loop so
    /// `user_event` switches `window` to tab `index` via [`App::switch_tab_in`] and
    /// re-mirrors it ([`App::sync_window`] / [`App::sync_active_session`]). The
    /// segmented control knows its own `window` (the toolbar item carries the
    /// `WindowId` it was installed for); a `tab <N>` verb targets the FRONT window,
    /// resolved on the main thread (see [`Wake::TabCmd`]). Fire-and-forget.
    SelectTab {
        window: WindowId,
        index: usize,
    },
    /// A native tab's CLOSE × was clicked (the per-tab close button in the window
    /// toolbar's view-based tab strip) — close tab `index` of `window` as a unit.
    /// Posted onto the loop so `user_event` runs [`App::close_tab_at`] (the SAME
    /// whole-tab close the renderer strip's `✕` / the `tab close` verb take) and, if
    /// that was the window's LAST tab, escalates to closing the window via
    /// `escalate_pending_close` (the AppKit action call site has no `ActiveEventLoop`,
    /// so it flags `pending_close` and the handler tears the window down). The button
    /// knows its own `window` + tab `index` (baked into the per-tab close target).
    /// Fire-and-forget.
    CloseTab {
        window: WindowId,
        index: usize,
    },
    /// The control socket's `tab` verb (`new`/`<N>`/`next`/`prev`), driving the
    /// FRONT window's tabs. Unlike [`Wake::SelectTab`] (which knows its window) the
    /// verb targets `self.frontmost_window`, which only the main thread can resolve,
    /// and it must REPLY with the resulting `(active_index, tab_count)` so the
    /// `aterm-ctl` client can print `OK <active> <count>`. So the control thread
    /// builds a one-shot reply channel, posts this, and BLOCKS on it — mirroring the
    /// `Wake::Control`/`Wake::ReadChrome` round-trips. The action mutates `App`
    /// (open/switch/cycle) ON the main loop turn, then sends back the new state.
    TabCmd {
        action: TabAction,
        reply: std::sync::mpsc::Sender<(usize, usize)>,
    },
    /// The `window` introspection verb wants the frontmost window's ENTIRE
    /// on-screen pixels — the native OS chrome (titlebar, traffic lights, the
    /// unified toolbar, the full-width tab strip) AS WELL AS the terminal content
    /// — captured to a PNG. Unlike `image` (which rasterizes only the terminal
    /// content framebuffer via the renderer), this reaches the window's `NSWindow`
    /// and `CGWindowListCreateImage`s its real composited pixels, which ONLY the
    /// main thread may do (AppKit + the window number are main-thread state). The
    /// control thread builds the one-shot `reply` channel, posts this, and BLOCKS;
    /// the main thread captures into the CONFINED `path` and replies `Ok((w, h))`
    /// (the PNG's pixel dims) or an `Err(msg)` (no window / capture failure /
    /// off-macOS). Mirrors the `Wake::ReadChrome` round-trip. The variant exists on
    /// every target so `Wake` stays platform-independent; off macOS the main thread
    /// replies with the off-macOS error string.
    CaptureWindow {
        path: control_auth::ConfinedImage,
        reply: std::sync::mpsc::Sender<Result<(u32, u32), String>>,
    },
}

/// What the control socket's `tab` verb asks the main thread to do to the FRONT
/// window's tabs. Carried by [`Wake::TabCmd`]; resolved against
/// `App::frontmost_window` on the event loop (the sole `App` mutator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    /// `tab new` — open a new tab in the front window (reuses [`App::open_tab`], the
    /// SAME path File ▸ New Tab and the toolbar "+" take).
    New,
    /// `tab <N>` — select the 0-based tab `N` in the front window.
    Select(usize),
    /// `tab next` — cycle to the next tab (wrapping).
    Next,
    /// `tab prev` — cycle to the previous tab (wrapping).
    Prev,
    /// `tab close [N]` — close tab `N` (or the active tab when `None`) in the front
    /// window. Reuses [`App::close_tab_at`] (the SAME path the native × button and the
    /// renderer strip's `✕` take); closing the LAST tab flags the window to close.
    Close(Option<usize>),
    /// `tab move <from> <to>` — reorder the front window's tab `from` to position
    /// `to` via [`App::move_tab`], fixing the active index so it follows the dragged
    /// tab. Out-of-range indices are a no-op. Drives + tests drag-to-reorder.
    Move { from: usize, to: usize },
}

impl Wake {
    /// A cross-session redraw NUDGE for `session` (used by the control thread's
    /// `select`/`scroll` verbs, which know only the session, not its owning
    /// window). The `Output` arm routes redraws by scanning which windows DISPLAY
    /// `session` ([`App::windows_displaying`]) and ignores the stamped `window`, so
    /// the window here is a don't-care sentinel — the control thread cannot resolve
    /// the owning window, and need not. Keeps the `WindowId` construction in this
    /// module (the only producer of real owning windows is `spawn_session`).
    fn redraw(session: u64) -> Wake {
        Wake::Output {
            session,
            window: WindowId(0),
        }
    }
}

/// The single resident renderer (ATERM_DESIGN WS-F): EXACTLY ONE is held — the
/// GPU `GpuRenderer` (wgpu/Metal) when `ATERM_GPU` is live, else the CPU
/// `Renderer`. A concrete enum (not `Box<dyn Rasterizer>`) so the GPU variant can
/// expose its on-glass present API (`present_input`/surface management) that the
/// trait can't, while keeping the single-font-cache property — only one renderer,
/// hence one glyph cache, is ever built.
///
/// The inherent methods below mirror the `Rasterizer` calls the frontend uses, so
/// call sites stay branch-free; `is_gpu`/`gpu_mut` reach the GPU-only present path.
#[allow(
    clippy::large_enum_variant,
    reason = "held once per App for its lifetime; Gpu is the common path, so boxing it to shrink the rare Cpu fallback would add a per-frame indirection for a one-time few-KB saving"
)]
enum Backend {
    Cpu(Renderer),
    Gpu(aterm_gpu::GpuRenderer),
}

impl Backend {
    /// Pixel size of one cell, `(width, height)` — from the live renderer.
    fn cell_size(&self) -> (usize, usize) {
        match self {
            Backend::Cpu(r) => r.cell_size(),
            Backend::Gpu(g) => g.cell_size(),
        }
    }

    /// Interior padding (px per edge) the live renderer insets the grid by. The
    /// window/swapchain are sized to fit the grid PLUS this border, and the mouse
    /// pixel→cell mapping subtracts it.
    fn pad(&self) -> usize {
        match self {
            Backend::Cpu(r) => r.pad(),
            Backend::Gpu(g) => g.pad(),
        }
    }

    /// Set the interior padding on the live renderer (and invalidate its damage
    /// cache so the next frame repaints at the new size).
    fn set_pad(&mut self, pad: usize) {
        match self {
            Backend::Cpu(r) => r.set_pad(pad),
            Backend::Gpu(g) => g.set_pad(pad),
        }
    }

    /// Padded pixel size of a `rows`×`cols` grid — the size to give the window /
    /// GPU swapchain so the grid fits WITH its `pad` border. With `pad == 0` this
    /// is the historical `cols·cell_w × rows·cell_h`.
    fn frame_size(&self, rows: usize, cols: usize) -> (usize, usize) {
        match self {
            Backend::Cpu(r) => r.frame_size(rows, cols),
            Backend::Gpu(g) => g.frame_size(rows, cols),
        }
    }

    /// Push the cursor blink phase (`on` = solid) into the renderer's state.
    fn set_cursor_blink_phase(&mut self, on: bool) {
        match self {
            Backend::Cpu(r) => r.set_cursor_blink_phase(on),
            Backend::Gpu(g) => g.set_cursor_blink_phase(on),
        }
    }

    /// Override the rendered cursor style regardless of DECSCUSR; `None` clears.
    fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        match self {
            Backend::Cpu(r) => r.set_cursor_style_override(style),
            Backend::Gpu(g) => g.set_cursor_style_override(style),
        }
    }

    /// Render a frame offscreen and read it back into an owned [`Frame`]. Used by
    /// the snapshot + `image` introspection paths on BOTH backends (not the hot
    /// path). The per-frame CPU window present instead calls `Renderer`'s
    /// `render_input_into` directly (reusing the renderer's buffer, no per-frame
    /// pixel alloc — C-1), and the GPU window present uses `present_input`.
    ///
    /// `gpu_scratch` is the introspection-only [`WindowGpu`] (the GPU readback
    /// needs a per-window offscreen to draw into); it is IGNORED on the CPU path.
    /// The readback always does a FULL repaint, so this scratch only governs
    /// offscreen REUSE across snapshot/`image` calls — the pixels are identical
    /// regardless. It is SEPARATE from any window's on-glass present `window_gpu`,
    /// so a snapshot never disturbs a window's scissor/dirty-gate caches.
    fn render_input(
        &mut self,
        gpu_scratch: &mut aterm_gpu::WindowGpu,
        input: &RenderInput,
    ) -> Frame {
        match self {
            Backend::Cpu(r) => r.render_input(input),
            Backend::Gpu(g) => g.render_input(gpu_scratch, input),
        }
    }

    /// Whether the GPU on-glass present path is active.
    fn is_gpu(&self) -> bool {
        matches!(self, Backend::Gpu(_))
    }

    /// The GPU renderer, for the on-glass surface + present calls; `None` on CPU.
    fn gpu_mut(&mut self) -> Option<&mut aterm_gpu::GpuRenderer> {
        match self {
            Backend::Gpu(g) => Some(g),
            Backend::Cpu(_) => None,
        }
    }
}

/// One in-window TAB: an independent shell session (its own PTY master, engine
/// `Terminal`, reader thread, policy engine, OSC52 authorization, and — when
/// shell integration is on — its own FRESH capability nonce). The window,
/// renderer, and surface are shared across all sessions; only this per-session
/// state is multiplexed. `App` keeps the ACTIVE session's `term`/`master` mirrored
/// into its own fields (a cheap `Arc` clone + an `i32`) so the ~44 existing
/// `self.term`/`self.master` call sites and their disjoint-field borrows are
/// UNCHANGED; a tab switch just re-mirrors from `sessions[active]`.
/// Per-session fabric context: the ONE byte sink for this PTY master, the
/// destination-side edge table, and this session's stable fabric identity +
/// launch nonce. One per tab (1:1 with `Session`/`Session.id`). Cloned into the
/// reader thread and published into `ActiveSession` so the active tab's sink +
/// edge table back every writer and the op-scope gate.
pub struct SessionCtx {
    pub sink: Arc<SinkWriter>,
    pub edges: std::sync::Mutex<EdgeTable>,
    pub self_id: SessionId,
    pub nonce: LaunchNonce,
    /// asciicast v2 recorder for this session's PROGRAM OUTPUT (design A.5.1).
    /// The reader thread hands output bursts lock-free to a writer thread that
    /// folds them in here; the `cast` control verb serializes the current
    /// recording out of it. `Mutex`-wrapped because the writer thread and the
    /// control thread both touch it (never on the reader's hot path).
    pub cast: Arc<std::sync::Mutex<crate::cast::CastRecorder>>,
    /// Live, byte-lossless, every-frame output fan-out (Item 2): the PUSH twin of
    /// `cast`. The reader thread `tee`s every program-output burst here (one extra
    /// `Arc` refcount, no copy); a `subscribe … bytes` connection registers and
    /// drains a byte-exact queue. Empty/cheap when nobody is subscribed.
    pub byte_fanout: Arc<crate::cast::ByteFanout>,
    /// Temporal recorder for this session: the `aterm-buffer` event-log spine
    /// (keyframe + `RawIn`/`Reply`/`Resize` events) that makes the session
    /// hydratable at any instant (design Addendum B / B.9). Fed off the reader
    /// hot path on a dedicated writer thread (mirroring `cast`); `Mutex`-wrapped
    /// because the writer thread, the resize tap, and any future scrub reader
    /// touch it (never on the reader's hot path under `term_lock`).
    pub temporal: Arc<std::sync::Mutex<crate::temporal::TemporalRecorder>>,
}

struct Session {
    /// Stable identity used to route [`Wake`] events from this session's reader
    /// thread (NOT the Vec index, which shifts when an earlier tab closes).
    id: u64,
    term: Arc<Mutex<Terminal>>,
    master: i32,
    /// This session's child shell pid == its process-group id (`forkpty` ->
    /// `login_tty` -> `setsid` makes it a session leader). Used by `Drop` to
    /// HANG UP the job tree (SIGHUP) so the reader's blocking `read(master)` gets
    /// EOF and ends — the non-blocking teardown that avoids the macOS quit-hang.
    pid: i32,
    ctx: Arc<SessionCtx>,
    /// The proxy-table key for the child this session spawned (Item 5b), retained
    /// so `Drop` can deregister it — otherwise the process-wide `PROXIES` map grows
    /// for the process lifetime as tabs open/close. `None` for one-shot `-e`
    /// sessions (which never provision a child).
    child_proxy_sid: Option<SessionId>,
}

impl Drop for Session {
    /// Tear this tab's PTY down WITHOUT ever blocking the UI thread.
    ///
    /// THE BUG THIS FIXES (architectural): the old `Drop` called
    /// `libc::close(self.master)` inline on whatever thread dropped the `Session`
    /// — the main/UI thread on a mid-run Cmd-W or pane close. A macOS stackshot
    /// proved `close(master)` then wedges in `lck_mtx_sleep` on the tty lock,
    /// racing this session's reader thread still parked in `read(master)`: the
    /// UI thread hung ~49 s at exit. Invariant restored: the UI/main thread NEVER
    /// makes an unbounded blocking syscall.
    ///
    /// The non-blocking teardown:
    ///   1. `hangup(pid)` — SIGHUP the child's process group. The shell (and its
    ///      jobs) exit, the PTY slave closes, and the reader thread's blocked
    ///      `read(master)` returns EOF and the thread ends ON ITS OWN (dropping its
    ///      `Arc<SinkWriter>` clone). `killpg` never touches the tty lock, so this is
    ///      safe on the UI thread.
    ///   2. Hand the `pid` to a DETACHED reaper thread for a bounded `reap(pid)`
    ///      (poll + SIGKILL escalation, see [`aterm_pty::reap`]), so a child that
    ///      ignores SIGHUP can't wedge the UI thread — and SIGKILL still forces the
    ///      slave closed → the reader EOFs. The reap runs OFF the UI thread.
    ///
    /// The master fd is NOT closed here. It is OWNED by the session's `SinkWriter`
    /// (built via `SinkWriter::new_owned`) and closes exactly when the LAST
    /// `Arc<SinkWriter>` clone drops — i.e. after the reader thread has EOF'd and
    /// every window mirror / in-flight control verb has released its clone. This is
    /// the fix for the close-vs-use race: an out-of-band `close(master)` on a
    /// detached thread could close the fd while the reader was parked in
    /// `read(master)` or a writer was inside `write_frame`, and a `forkpty` could
    /// then recycle the freed number — routing a read or a keystroke to the WRONG
    /// session. Tying the close to the last clone's drop makes that impossible.
    ///
    /// NOTE: the FINAL app-exit path does NOT rely on this — `main` calls
    /// `std::process::exit(0)` after the event loop returns, so the OS reclaims
    /// every fd and SIGHUPs the children via controlling-tty teardown and this
    /// `Drop` never runs at exit. `Drop` is the MID-RUN close path (Cmd-W / pane
    /// close), which must also stay non-blocking on the UI thread — hence this.
    fn drop(&mut self) {
        // (0) Drop the proxy-table capability for the child we spawned (Item 5b),
        // so a long-lived aterm opening/closing tabs does not grow `PROXIES`. Also
        // remove the parent→child edge-token file we wrote (audit finding F1): it
        // PERSISTS for the session (so a same-shell child relaunch can re-read it),
        // and the parent OWNS its removal — this is the graceful per-child teardown
        // (mid-run tab/pane close). A crash that skips this leaves an inert file: its
        // tokens bind a random `(sid, nonce)` never reissued, so it authorizes nothing.
        if let Some(sid) = &self.child_proxy_sid {
            crate::proxy::deregister_child(sid);
            if let Some(dir) = crate::control_auth::socket_dir() {
                crate::proxy::remove_edge_tokens(&dir, sid);
            }
        }
        // (1) Hang up the child's session so the reader unblocks (EOF). Cheap,
        // non-blocking, UI-thread-safe. Dropping the last `Arc<SinkWriter>` clone
        // (here + when the reader exits + when mirrors release) closes the fd.
        aterm_pty::hangup(self.pid);
        let pid = self.pid;
        // Nothing to reap for a stub/sentinel session (no real child).
        if pid <= 1 {
            return;
        }
        // (2) Reap the child OFF the UI thread — bounded, self-terminating.
        std::thread::spawn(move || {
            aterm_pty::reap(pid);
        });
    }
}

/// Pure tab-index state machine, factored out of `App` so the add/switch/cycle/
/// close logic is unit-testable headlessly (no window / PTY / event loop). Holds
/// ONLY the active index and the live session count; `App` owns the actual
/// `Vec<Session>` and applies the same operations to it. Every method keeps
/// `active < count` (the single invariant the renderer relies on) as long as
/// `count >= 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabIndex {
    active: usize,
    count: usize,
}

impl TabIndex {
    fn new(active: usize, count: usize) -> Self {
        TabIndex { active, count }
    }

    /// A tab was appended: the new tab (last index) becomes active, the standard
    /// "open a tab and switch to it" behavior. Returns the new active index.
    fn add(&mut self) -> usize {
        self.count += 1;
        self.active = self.count - 1;
        self.active
    }

    /// Switch to tab `i` if it exists (Cmd-1..Cmd-9). No-op (returns the current
    /// active) when `i` is out of range, so Cmd-5 in a 3-tab window does nothing.
    fn switch_to(&mut self, i: usize) -> usize {
        if i < self.count {
            self.active = i;
        }
        self.active
    }

    /// Cycle to the next (`forward`) / previous tab, WRAPPING at the ends
    /// (Cmd-Shift-] / Cmd-Shift-[). No-op with zero/one tab. Returns the new active.
    fn cycle(&mut self, forward: bool) -> usize {
        if self.count <= 1 {
            return self.active;
        }
        self.active = if forward {
            (self.active + 1) % self.count
        } else {
            (self.active + self.count - 1) % self.count
        };
        self.active
    }

    /// Close the tab at index `i`. Returns `true` iff that was the LAST tab (so
    /// the caller exits the app). Otherwise decrements `count` and CLAMPS `active`:
    /// closing a tab before the active one shifts the active index down by one so
    /// it still points at the SAME session; closing the active (or any later) tab
    /// clamps `active` into the new range (so closing the last-in-list active tab
    /// moves focus to the new last tab). The caller removes element `i` from its
    /// `Vec<Session>` in lockstep so indices stay aligned.
    fn close(&mut self, i: usize) -> bool {
        if i >= self.count {
            return false; // out of range: nothing to close
        }
        if self.count <= 1 {
            return true; // closing the last tab → exit the app
        }
        self.count -= 1;
        if i < self.active {
            // An EARLIER tab closed: the active session shifted down one slot.
            self.active -= 1;
        } else if self.active >= self.count {
            // The active (or a later) tab closed and active now points past the
            // end: clamp to the new last tab.
            self.active = self.count - 1;
        }
        false
    }
}

/// Process-global pool that OWNS every live `Session`, keyed by the stable,
/// never-reused local id. Refcounted by `views` (how many windows display the
/// session) so the PTY master closes — via `Session::drop` — exactly when the
/// LAST view detaches. That is the precondition for same-session-in-two-windows
/// and detach-tab with zero PTY churn (a later step); today every session has
/// exactly one view.
#[derive(Default)]
struct SessionPool {
    sessions: std::collections::HashMap<u64, PooledSession>,
}

struct PooledSession {
    session: Session,
    /// Number of windows currently displaying this session (>= 1 while live).
    views: u32,
}

impl SessionPool {
    /// Take ownership of a freshly-spawned session with one view. Returns its id.
    fn insert(&mut self, session: Session) -> u64 {
        let id = session.id;
        self.sessions
            .insert(id, PooledSession { session, views: 1 });
        id
    }
    /// Borrow a session by id (None if unknown).
    fn get(&self, id: u64) -> Option<&Session> {
        self.sessions.get(&id).map(|p| &p.session)
    }
    /// Add a view: a second window now displays this session
    /// (`open_active_session_in_new_window`). No-op if unknown.
    ///
    /// TRUST anchor: the `Acquire` action of the ty-proven `session_pool` machine
    /// (`session_pool_model()`); Tier-1 binding is `session_pool_conformance`.
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "session_pool",
            action = "Acquire",
            project = "aterm_gui::session_pool_conformance::project"
        )
    )]
    fn attach(&mut self, id: u64) {
        if let Some(p) = self.sessions.get_mut(&id) {
            p.views += 1;
        }
    }
    /// The current view-count for `id` (None if unknown). Lets the
    /// same-session-in-two-windows test assert the 1→2→1→0 refcount accounting
    /// directly, and lets `split_focused_pane` enforce the "a shared session is
    /// never split" invariant (a shared grid must not be resized by a split).
    fn views(&self, id: u64) -> Option<u32> {
        self.sessions.get(&id).map(|p| p.views)
    }
    /// Drop a view; remove + drop the `Session` (closing its PTY exactly once)
    /// iff the refcount hits 0. Returns true iff the session was dropped.
    ///
    /// TRUST anchor: the `Release` action of the ty-proven `session_pool` machine
    /// (`session_pool_model()`) — refcount-1, retiring the entry IFF it hits 0.
    /// Tier-1 binding is `session_pool_conformance`.
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "session_pool",
            action = "Release",
            project = "aterm_gui::session_pool_conformance::project"
        )
    )]
    fn detach(&mut self, id: u64) -> bool {
        if let Some(p) = self.sessions.get_mut(&id) {
            p.views = p.views.saturating_sub(1);
            if p.views == 0 {
                self.sessions.remove(&id);
                return true;
            }
        }
        false
    }
    /// Iterate every live session (for window-level apply-to-all operations).
    fn iter(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values().map(|p| &p.session)
    }
}

/// Synthetic, process-unique identity for a LOGICAL window — NOT winit's
/// `WindowId`. A logical window exists from `App` construction (and throughout
/// headless mode) before, or without ever, an OS window, so it cannot be keyed
/// by an id that only exists once `el.create_window` has run. When an OS window
/// is attached, its winit id is mapped to this via `App.winit_to_window`.
/// Monotonic, never reused (the multi-window analogue of `next_session_id`), so
/// a stale `Wake` for a closed window can never address a live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WindowId(u64);

/// Result of closing a logical window: whether the APP should now exit. `Exit`
/// only when the LAST window was just torn down (no live windows remain) — the
/// code-level shadow of the ty-proven `ExitIffEmpty` invariant. `Stay` keeps the
/// run loop alive (a sibling window survives). Routed by `close_window`, which
/// calls `el.exit()` IFF `Exit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseOutcome {
    Exit,
    Stay,
}

/// Where a logical window presents frames. `None` until an OS window+surface are
/// attached (and forever in headless). Mirrors the old surface/_context/gpu_surface.
#[allow(
    clippy::large_enum_variant,
    reason = "one per window for its lifetime; Gpu is the common path, so boxing it to shrink the rare Cpu surface variant would add indirection to every present for a one-time saving"
)]
enum PresentTarget {
    Cpu {
        surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
        _context: softbuffer::Context<Arc<Window>>,
    },
    Gpu {
        gpu_surface: aterm_gpu::GpuSurface,
        /// This window's per-window GPU state (offscreen render target + dirty-gate
        /// / scissor caches). The shared device / glyph atlas / pipelines live on
        /// the one `Backend::Gpu` renderer; only this is per-window, so N windows
        /// share ~1 device + 1 atlas and each owns a small offscreen. Recreated
        /// fresh (via `WindowGpu::new`) whenever the surface is (re)built, so it
        /// never holds resources from a dropped device.
        window_gpu: aterm_gpu::WindowGpu,
    },
}

/// Per-window view state: everything that belonged to one window when `App` was
/// single-window — the OS handle + present target, the active-session mirror
/// (term/master/sink/active_id), grid size, and all per-window input / selection
/// / IME / find / blink / bell state. `App` now holds a `BTreeMap<WindowId,
/// WindowState>` (one entry today). UI defaults come from `WindowState::new`;
/// the OS window + present target attach later in `resumed` (never in headless).
struct WindowState {
    os_window: Option<Arc<Window>>,
    present: Option<PresentTarget>,
    /// Mirror of the ACTIVE session's `term` for this window's call sites
    /// (the source of truth is `sessions[active]`; re-mirrored on tab switch).
    term: Arc<Mutex<Terminal>>,
    /// Mirror of the ACTIVE session's PTY master fd.
    master: i32,
    /// Mirror of the ACTIVE session's `SinkWriter`, so this window's GUI egress
    /// funnels through the one per-session sink.
    sink: Arc<SinkWriter>,
    /// Stable id of the session this window currently shows: the FOCUSED pane of
    /// the active tab (`layouts[tabs.active].focus()`). With no splits this is the
    /// active tab's single session — byte-identical to the old `tab_ids[active]`.
    active_id: u64,
    /// This window's tabs, in order. INVARIANT: `tabs.count == layouts.len()`. The
    /// sessions themselves are owned by `App.pool`; this is just the per-window
    /// ordering + active index.
    tabs: TabIndex,
    /// One binary pane tree per TAB, indexed in lockstep with [`Self::tabs`]
    /// (`layouts[tabs.active]` is the visible tab's panes). REPLACES the old
    /// `tab_ids: Vec<u64>`: a no-split tab is a single [`pane::PaneTree`] leaf, so
    /// `layouts[i].focus()` == the old `tab_ids[i]` (byte-identical no-split path).
    /// Cmd-D / Cmd-Shift-D split the active tab's FOCUSED pane; the tree's focused
    /// leaf is the session keyboard input + the control socket target.
    layouts: Vec<pane::PaneTree>,
    rows: u16,
    cols: u16,
    mods: ModifiersState,
    /// Last cell (row, col) the cursor moved over, in the FOCUSED PANE's LOCAL grid
    /// coordinates (window cell minus the focused pane's offset), updated on
    /// `CursorMoved` and used to position mouse button/wheel reports for the focused
    /// pane's engine (winit delivers the pointer position on motion, not on
    /// click/scroll). With no splits the focused pane fills the window, so this is
    /// the raw window cell — byte-identical to before.
    last_mouse_cell: (u16, u16),
    /// Last cell (row, col) the cursor moved over in WINDOW coordinates (before the
    /// focused-pane offset is subtracted). Used to hit-test which pane a click lands
    /// in (click-to-focus). Equal to `last_mouse_cell` when there are no splits.
    last_mouse_window_cell: (u16, u16),
    /// The last raw pixel position (`CursorMoved`), kept so a button press (which
    /// carries no pixel position of its own) can tell whether the pointer is over
    /// the tab strip ([`Self::strip_col_at`]) before mapping to a terminal cell.
    last_cursor_px: (f64, f64),
    /// Whether the OS cursor is currently the link "pointer" (Cmd-hovering a link),
    /// so `set_cursor` is only called on a state change, not every mouse move.
    hover_pointer: bool,
    /// True while a left-button drag is building a text selection (only when
    /// no app is tracking the mouse). Cleared on release.
    selecting: bool,
    /// Whether the pointer left the press cell during the current drag: a
    /// press+release within one cell is a plain click, which deselects.
    sel_dragged: bool,
    /// Live-screen selection cell of the initial press (row may be negative
    /// when the press lands in scrollback), for click-vs-drag detection.
    sel_press_cell: (i32, u16),
    /// Instant + live-screen cell of the last left press, for multi-click
    /// (double/triple-click) detection.
    last_press: Option<(Instant, (i32, u16))>,
    /// Click count of the current multi-click streak: 1 = single (simple
    /// drag), 2 = double (word selection), 3 = triple (line selection); a
    /// fourth rapid click wraps back to 1.
    click_count: u8,
    /// Which half of the cell the pointer was last in (updated on
    /// `CursorMoved`): the anchor side for drag updates and shift-click
    /// extension — Right includes the hovered cell, Left stops before it.
    last_mouse_side: SelectionSide,
    /// Origin word/line of an in-flight double/triple-click drag, so motion
    /// extends the selection by whole units. `None` for simple/block drags.
    gesture: Option<GestureOrigin>,
    /// The mouse button currently held down (set on press, cleared on release),
    /// so a motion report under app mouse-tracking (DECSET 1002/1003) carries the
    /// real held button instead of the no-button hover code. Without this, drags
    /// in tracking apps (vim `mouse=a` visual drag, tmux pane drag, less, htop)
    /// emitted `button 3` — which 1002 drops entirely and 1003 misreads as hover.
    held_mouse_button: Option<aterm_types::mouse::MouseButton>,
    /// Whether the window currently has keyboard focus (from
    /// `WindowEvent::Focused`). Unfocused: the cursor draws as a hollow block
    /// and blink scheduling stops (the loop stays in `Wait` — 0% idle).
    focused: bool,
    /// Current cursor blink phase: `true` = the cursor is shown. Only consulted
    /// by the renderers for the `Blinking*` DECSCUSR styles.
    blink_phase: bool,
    /// The next blink toggle deadline. Armed ONLY while blinking is active
    /// (focused window + Blinking* style + cursor visible); `None` keeps the
    /// event loop in pure `Wait` so an idle steady/unfocused session burns 0%.
    next_blink: Option<Instant>,
    /// Visual bell: while active the presented frame is inverted; like blink,
    /// its deadline arms `WaitUntil` only while a flash is pending, so the
    /// loop stays in pure `Wait` (0% idle) between bells.
    bell_flash: BellFlash,
    /// The title currently shown in the window chrome. Mirrors the engine's
    /// program-set title (OSC 0/2); `redraw()` calls `set_title` only when this
    /// diverges, so a program that updates its title (shell cwd, vim, ssh) is
    /// reflected in the titlebar like any real terminal.
    current_title: String,
    /// The `(active session id, title)` last PUBLISHED to the shared `SessionStore`
    /// for this window. `apply_title` only takes the process-wide store WRITE lock
    /// (contended with the control thread) and re-publishes when EITHER the active
    /// session OR its title differs from this — so a steady screen redrawing at the
    /// present cadence no longer grabs the exclusive lock every frame. Keying on the
    /// SESSION id (not just the title) keeps it correct across a tab switch / migrate
    /// and a backgrounded session whose title changed. Init id `u64::MAX` (no real
    /// session) so the first publish always writes.
    store_title: (u64, String),
    /// Persistent per-frame snapshot buffer (C-1): `redraw()` refills this in
    /// place via the engine's `Terminal::cell_frame_into` (A-3) under the lock
    /// instead of allocating a fresh `RenderInput` every frame, so a steady-size
    /// session does no per-frame heap allocation for the grid snapshot.
    input_scratch: RenderInput,
    /// Persistent PER-PANE snapshot buffer reused while composing a split tab's
    /// frame: each visible pane's `Terminal::cell_frame_into` refills this in place,
    /// then its cells are blitted into `input_scratch` at the pane's offset. Unused
    /// (and empty) on the single-pane path, which fills `input_scratch` directly.
    /// Disjoint from `input_scratch` so the compose loop can borrow both at once.
    pane_scratch: RenderInput,
    /// The tab strip's laid-out segments from the LAST composed frame for THIS
    /// window (column ranges + close-`x` columns + click targets), cached so a
    /// mouse click in the strip maps back to a tab in O(segments) without re-laying
    /// out. Empty until the first strip is drawn (and always empty when the global
    /// `tab_strip_rows == 0`). Per-window so each window hit-tests its own strip.
    tab_segments: Vec<tab_bar::TabSegment>,
    /// E3 strip-row cache: the `tab_strip_fingerprint` + column count the
    /// `cached_strip_rows` below were painted for, or `None` before the first strip.
    /// `splice_tab_strip` reuses the cached rows when the fingerprint AND width still
    /// match — the common present (terminal content changed, the strip did not), so
    /// the per-present `paint_strip` + row build is skipped. Invalidated wherever
    /// `tab_segments` is cleared (geometry/font change).
    last_strip_fp: Option<(u64, usize)>,
    /// The painted tab-strip rows from the last build (see `last_strip_fp`). Cloned
    /// into `input_scratch` on a cache hit; rebuilt on a miss.
    cached_strip_rows: Vec<Vec<RenderCell>>,
    /// The last `(segment_count, selected)` this window's NATIVE titlebar tab strip
    /// (`refresh_window_tabs` → `toolbar::set_window_tabs`) was told to render — a
    /// faithful shadow of the `NSSegmentedControl`. Written AT the push point, so a tab
    /// mutation that FORGETS to call `refresh_window_tabs` leaves it STALE, exactly as
    /// the live control would be. The `tab_strip` Tier-1 conformance reads it to witness
    /// a strip↔model desync invisible in a headless test (no real toolbar). Tiny +
    /// always-on; production writes it and never reads it.
    strip_shadow: std::cell::Cell<(usize, usize)>,
    /// Per-window CPU damage-cache state (S5c) — the CPU analog of the GPU
    /// present target's `window_gpu`. The shared CPU [`Renderer`] holds only
    /// glyph/metrics/cursor state; the damage cache (previous frame's pixels +
    /// input, the dirty-row diff base) is keyed on `(w, h)` + the prior
    /// `RenderInput` with NO window identity, so it MUST live per-window — else
    /// two windows sharing one `Renderer` would diff against each other's cached
    /// input and hand one window the other's pixels. Threaded into
    /// `Renderer::render_input_cached` at the CPU present call site.
    cpu_cache: WindowCpu,
    /// The [`RepaintKey`] of the last frame actually presented (D-1), or `None`
    /// before the first present. `redraw()` skips the whole extract + rasterize +
    /// present when the current key equals this (see [`should_repaint`]).
    last_present: Option<RepaintKey>,
    /// IME composition (IME-1): the marked/preedit text currently being
    /// composed (CJK input, dead keys), or empty when no composition is active.
    /// While non-empty, direct key sends are SUPPRESSED so composing keystrokes
    /// don't ALSO emit raw bytes; the committed text arrives via `Ime::Commit`.
    /// A minimal inline indicator is rendered (see `preedit_indicator`).
    preedit: String,
    /// Active Cmd-F find: query + matches on the visible screen + current index.
    /// `None` when not searching. While `Some`, keystrokes edit the query instead
    /// of going to the PTY.
    search: Option<SearchState>,
    /// Set by Cmd-W; the loop closes this window after the handler returns
    /// (renamed from the old `App.should_exit`).
    pending_close: bool,
}

impl WindowState {
    #[allow(
        clippy::too_many_arguments,
        reason = "a window's full session+layout state (term/master/sink/active + rows/cols/tabs/layouts); bundling into structs only relocates the argument list"
    )]
    fn new(
        term: Arc<Mutex<Terminal>>,
        master: i32,
        sink: Arc<SinkWriter>,
        active_id: u64,
        rows: u16,
        cols: u16,
        tabs: TabIndex,
        layouts: Vec<pane::PaneTree>,
    ) -> Self {
        WindowState {
            os_window: None,
            present: None,
            term,
            master,
            sink,
            active_id,
            tabs,
            layouts,
            rows,
            cols,
            mods: ModifiersState::empty(),
            last_mouse_cell: (0, 0),
            last_mouse_window_cell: (0, 0),
            last_cursor_px: (0.0, 0.0),
            hover_pointer: false,
            selecting: false,
            sel_dragged: false,
            sel_press_cell: (0, 0),
            last_press: None,
            click_count: 0,
            last_mouse_side: SelectionSide::Left,
            gesture: None,
            held_mouse_button: None,
            focused: true,
            blink_phase: true,
            next_blink: None,
            bell_flash: BellFlash::new(),
            current_title: "aterm".to_string(),
            store_title: (u64::MAX, String::new()),
            input_scratch: RenderInput::empty(),
            pane_scratch: RenderInput::empty(),
            tab_segments: Vec::new(),
            last_strip_fp: None,
            cached_strip_rows: Vec::new(),
            // A fresh window has one tab, selected — the strip's initial mirror.
            strip_shadow: std::cell::Cell::new((1, 0)),
            cpu_cache: WindowCpu::new(),
            last_present: None,
            preedit: String::new(),
            search: None,
            pending_close: false,
        }
    }
}

struct App {
    /// Process-global pool that OWNS every live `Session` (≥1), keyed by stable
    /// id and refcounted by view-count. Each window's `tab_ids`/`tabs` index INTO
    /// this pool; the active tab's `term`/`master` are mirrored into the front
    /// window's `WindowState` for the existing call sites (see [`Session`]).
    pool: SessionPool,
    /// Monotonic id source for new sessions ([`Session::id`]); never reused, so a
    /// late `Wake` from a just-closed tab's reader can never address a live tab.
    next_session_id: u64,
    /// `--hold`: keep the window open after a session's command exits instead of
    /// closing its tab on EOF (mirrors the single-session behavior, per-tab).
    hold: bool,
    /// Captured startup inputs for spawning a NEW tab's session ([`spawn_session`]):
    /// the by-reference spawn/sandbox caps (the SINGLE root authority, minted once
    /// in `main` — never re-minted), the baseline child environment, the engine
    /// config, the shell-integration decision, and the working directory.
    session_factory: SessionFactory,
    /// Proxy for spawning a Cmd-T tab's reader/bell `Wake`s back to this loop.
    /// `Some` for the whole life of a real run (built from the event loop in
    /// `main`); `None` ONLY under `headless_for_test`, where no event loop exists,
    /// so no PTY/menu/wake path that needs it is ever driven. Every use is a guarded
    /// early-return on `None`, never a panic.
    proxy: Option<EventLoopProxy<Wake>>,
    /// Shared pointer to the ACTIVE session's `term`+`master` that the control
    /// socket reads, kept in sync by `sync_active_session` so introspection follows
    /// tab switches. Always present (cheap) even when the socket is disabled.
    active_handle: control::ActiveHandle,
    /// Process-wide session registry (P1.1): the ADDITIVE index that makes every
    /// live session resolvable by stable `SessionId` (and by `u64` id) so a
    /// cross-session `@<selector>` verb can reach a sibling. The `pool` + windows'
    /// `tab_ids` stay the pane view; this is a pure index registered at spawn,
    /// deregistered at
    /// close. Cloned into the control thread alongside `active_handle`.
    store: session_store::Store,
    /// Real-time SUBSCRIBER registry (P1.3): the additive index a `subscribe`
    /// connection registers in so the ONE `Wake::Output { session }` hook below can
    /// notify every live watcher of that session in O(1). The notify is a
    /// single-slot non-blocking `try_send` — a slow/dead subscriber NEVER blocks
    /// this (GUI) thread. Cloned into the control thread alongside `store`.
    subscribers: subscribe::Subscribers,
    /// The single resident renderer (CPU or GPU) — see [`Backend`]. EXACTLY ONE
    /// is held: the GPU `GpuRenderer` (wgpu/Metal) when `ATERM_GPU` is live (and
    /// initializes), else the CPU `Renderer`. The CPU path presents via softbuffer
    /// and the GPU path blits straight to the swapchain (`present_input`).
    backend: Backend,
    /// Introspection-only per-window GPU state for the snapshot / `image` readback
    /// path ([`Backend::render_input`]). The GPU readback renders into an offscreen
    /// it owns; this is that offscreen's home, kept SEPARATE from any window's
    /// on-glass present `window_gpu` so a snapshot/`image` never perturbs a
    /// window's scissor/dirty-gate caches. Inert on the CPU backend. Reset to a
    /// fresh `WindowGpu` whenever the GPU device is rebuilt ([`Self::rebuild_backend`]),
    /// since the old offscreen lived on the now-dropped device.
    introspect_gpu: aterm_gpu::WindowGpu,
    /// Current and launch-default font size (physical px), for live Cmd-+/-/0 zoom.
    font_px: f32,
    default_font_px: f32,
    /// Whether the live backend is the GPU one, so a zoom rebuilds the same kind.
    use_gpu: bool,
    /// The configured renderer theme, re-applied when a font-zoom rebuilds the backend.
    theme: Theme,
    /// All logical windows (≥1). EXACTLY one entry today (`WindowId(0)`); later
    /// steps add more. Each holds the per-window view state that used to live
    /// directly on `App` (term/master/sink mirror, grid size, input/selection/IME/
    /// find/blink/bell state, OS window + present target).
    windows: BTreeMap<WindowId, WindowState>,
    /// The logical window that currently has focus (the one most call sites act on
    /// via `front`/`front_mut`). `None` only if every window has been closed.
    frontmost_window: Option<WindowId>,
    /// Focus-order (MRU) stack: window ids in order of LAST focus gain, most-recent
    /// LAST. Updated only by real `WindowEvent::Focused(true)` (so it stays EMPTY in
    /// headless, where no OS focus events fire) and pruned when a window closes. When
    /// the FRONTMOST window closes, the next frontmost is the most-recently-focused
    /// SURVIVOR (matching the window macOS raises), with a lowest-live-id fallback for
    /// the no-focus-history case — see [`Self::next_frontmost_after_close`]. Bounded
    /// by the live-window count (each focus removes any prior entry before pushing).
    focus_order: Vec<WindowId>,
    /// Monotonic id source for new logical windows ([`WindowId`]); never reused, so
    /// a stale `Wake` for a closed window can never address a live one. Initialized
    /// to 1 (window 0 already minted at construction). Read + bumped by
    /// [`Self::create_window_logical`] — the multi-window analogue of `next_session_id`.
    next_window_id: u64,
    /// Maps an attached OS window's winit id back to our synthetic [`WindowId`].
    /// Populated in `resumed` when the OS window is created; an entry exists only
    /// while that window has an `os_window` (never in headless).
    winit_to_window: HashMap<WinitWindowId, WindowId>,
    /// When set ($ATERM_HEADLESS), no window/surface is ever created: the
    /// engine, control socket, and offscreen rendering (`image`/snapshot via
    /// [`Wake::Control`]) all run, but nothing is presented on screen.
    headless: bool,
    /// Audible bell gate: one beep per [`BELL_BEEP_INTERVAL`] (the engine
    /// already throttles BEL callbacks to 10/s; this slows the sound further).
    bell_beep: BellRateLimiter,
    /// Shared queue of control-socket `image` requests, drained on
    /// [`Wake::Control`] (the control thread cannot touch the renderer).
    image_queue: control::ImageQueue,
    /// Latency self-introspection ($ATERM_TRACE_LATENCY). When on, the PTY
    /// reader stamps the leading edge of each output burst into `last_output_ns`
    /// (nanos since `lat_epoch`), and `redraw()` logs output→present latency
    /// after `present()` — the software-controllable slice of input-to-photon
    /// (the rest is fixed panel scan-out, identical across every terminal).
    trace_latency: bool,
    lat_epoch: Instant,
    last_output_ns: Arc<AtomicU64>,
    /// Desktop-notification SUPPRESSION SET, read by the `notify` delivery thread:
    /// the active-tab focused-pane session id of every FOCUSED window. The UI thread
    /// rebuilds it (`recompute_notify_suppress`) on any focus or active-tab change; a
    /// notification is suppressed iff its session is in the set (the user is already
    /// looking at it in some focused window). The shared map is what crosses the
    /// thread boundary (the delivery thread can't see `self`). Per-window-correct for
    /// multi-window; `{active}`/`{}` at one window (byte-identical to the old gate).
    notify_suppress: Arc<Mutex<std::collections::HashSet<u64>>>,
    /// Whether the launch font size was set EXPLICITLY (`$ATERM_FONT_PX` or
    /// `config.font_px`), as opposed to the built-in [`FONT_PX`] default. When the
    /// size is the default, `resumed()` auto-scales it by the display scale factor
    /// on a HiDPI/Retina screen; an explicit size is taken verbatim (no double-apply).
    /// GLOBAL (window-uniform). Set once at startup.
    font_px_explicit: bool,
    /// The configured font FAMILY name (config `font_family`), re-applied when a
    /// font-zoom / config reload rebuilds the backend. `None` = the default
    /// `$ATERM_FONT` → built-in candidate chain (byte-identical to before). GLOBAL.
    font_family: Option<String>,
    /// macOS: whether Option/Alt sends ESC-prefixed (Meta) sequences (config
    /// `option_as_meta`, default `true`). When `false`, Option types the OS-
    /// composed character instead. Read in `on_key`. GLOBAL (window-uniform).
    option_as_meta: bool,
    /// User keyboard shortcuts (config `[keybindings]`). Consulted FIRST in
    /// `on_key`; a miss falls through to the hardcoded matches. Empty (the
    /// default) means the lookup is a single always-missing probe. GLOBAL: the
    /// table is window-independent; dispatch is threaded with the routed `wid`.
    keybindings: keybinding::Keybindings,
    /// Cmd-F find scan depth, in scrollback lines (config `search_history_lines`,
    /// default [`MAX_SEARCH_HISTORY`]). Read by `search_recompute`.
    search_history_lines: i32,
    /// The retained native menu-bar action target (macOS), kept alive for the
    /// whole run loop — AppKit holds a menu item's target only WEAKLY, so dropping
    /// this would silently break menu dispatch. Installed once in `resumed` (only
    /// when not headless); `None` while no menu exists (headless, off macOS, or
    /// before `resumed`). The type is `()` off macOS (`menu::MenuHandle`).
    _menu: Option<menu::MenuHandle>,
    /// Retained native window-toolbar backing objects (macOS), keyed BY WINDOW and
    /// kept alive for each window's life — AppKit holds a toolbar's delegate and an
    /// item's target only WEAKLY, so dropping a handle silently kills that window's
    /// "+" New Tab button. Per-window (NSToolbar is per-NSWindow, unlike the singleton
    /// menu bar): inserted in `attach_os_window`, removed in `close_window_logical`.
    /// `ToolbarHandle` is `()` off macOS, so this is an inert empty map there.
    _toolbars: BTreeMap<WindowId, toolbar::ToolbarHandle>,
    /// Rows reserved at the TOP of the window for the VISIBLE, CLICKABLE tab strip
    /// (config `tab_strip_rows`, default [`DEFAULT_TAB_STRIP_ROWS`]). `self.rows` is
    /// the TERMINAL grid (window rows minus this), so the pane layout / mouse / resize
    /// math is unchanged; the strip is spliced ABOVE the terminal content only in the
    /// composed `RenderInput`. `0` is the byte-identical no-strip path. GLOBAL
    /// (window-uniform): read by every window's `splice_tab_strip`/`strip_col_at`.
    /// NOTE: the per-frame laid-out `tab_segments` are PER-WINDOW (in `WindowState`).
    tab_strip_rows: u16,
}

impl App {
    /// Window `wid`'s visible tab's pane tree (`layouts[tabs.active]`), or `None`
    /// for a stale/unknown `wid` (a stale `Wake`/event must be a silent no-op, never
    /// a panic that crashes every other live window). For a live window the
    /// `layouts` Vec is kept in lockstep with `tabs`, so the active index is in range.
    fn active_tree(&self, wid: WindowId) -> Option<&pane::PaneTree> {
        let ws = self.windows.get(&wid)?;
        ws.layouts.get(ws.tabs.active)
    }

    /// Mutable handle to window `wid`'s visible tab's pane tree (split/close/focus),
    /// or `None` for a stale/unknown `wid`.
    fn active_tree_mut(&mut self, wid: WindowId) -> Option<&mut pane::PaneTree> {
        let ws = self.windows.get_mut(&wid)?;
        let active = ws.tabs.active;
        ws.layouts.get_mut(active)
    }

    /// The session id keyboard input + the control socket currently target for
    /// window `wid`: the FOCUSED pane of its visible tab. With no splits this is the
    /// tab's one session, byte-identical to the old `tab_ids[tabs.active]`.
    fn focused_session_id(&self, wid: WindowId) -> u64 {
        self.active_tree(wid).map_or(0, pane::PaneTree::focus)
    }

    /// Look up a live session by its stable `id`. The pool is the single
    /// process-global owner; panes are addressed by id, not by `Vec` index (which
    /// shifts when an earlier pane/tab closes). A thin wrapper over `pool.get`.
    fn session_by_id(&self, id: u64) -> Option<&Session> {
        self.pool.get(id)
    }

    /// Re-mirror the FOCUSED PANE's `term`/`master` into `App`'s own fields, the
    /// single source of truth for the ~44 existing `self.term`/`self.master` call
    /// sites (kept as fields, not accessors, so their disjoint-field borrows still
    /// compile). Called after every tab add/switch/cycle/close AND every pane
    /// split/close/focus change. Cheap: an `Arc` clone + an `i32` copy.
    /// `last_present = None` forces the next redraw to paint the newly-focused grid.
    /// Re-mirror window `wid`'s `term`/`master`/`sink`/`active_id` from the pool's
    /// copy of its ACTIVE tab, and clear that window's in-flight per-tab state
    /// (`last_present`/`search`/`selecting`/`gesture`) + request its redraw. This is
    /// the per-window half of [`Self::sync_active_session`] — the bookkeeping every
    /// window needs when ITS active tab changes (a switch/cycle/close, or a detach
    /// that leaves a NON-frontmost source window with a stale mirror) — but WITHOUT
    /// touching the global `active_handle`/`notify_active`, which follow only the
    /// FRONTMOST window. A window with no OS surface (headless) just updates the
    /// mirror (the redraw request is then a no-op). A stale/unknown `wid` is a
    /// silent no-op (`.get_mut` returns `None`).
    fn sync_window(&mut self, wid: WindowId) {
        // Disjoint borrows: the target WindowState is a different field from `pool`,
        // so destructuring lets us write the mirror while reading the pool.
        let App { windows, pool, .. } = self;
        let Some(ws) = windows.get_mut(&wid) else {
            return;
        };
        // A window always has ≥1 tab, so `layouts[tabs.active]` is in range (the
        // structural invariant); its FOCUSED pane is the active session. A stale id
        // is a no-op via `pool.get`.
        let aid = ws.layouts[ws.tabs.active].focus();
        let Some(s) = pool.get(aid) else {
            return;
        };
        ws.term = s.term.clone();
        ws.master = s.master;
        ws.sink = s.ctx.sink.clone();
        ws.active_id = s.id;
        // A switch changes which engine drives the screen; force a real paint and
        // clear any in-flight selection/find state that belonged to the old tab.
        ws.last_present = None;
        ws.search = None;
        ws.selecting = false;
        ws.gesture = None;
        if let Some(w) = &ws.os_window {
            w.request_redraw();
        }
        // The window's active tab changed → its notification-suppression contribution
        // (its active focused-pane id) may have moved; rebuild the shared set.
        self.recompute_notify_suppress();
        // A shared (Cmd-Shift-O) session entering or leaving this window's FOREGROUND
        // changes its foreground-min, so re-fit the grid: a session returning to a
        // window's active tab is sized to `min(this window, the other viewers)` (so
        // this window never over-reads it), and one leaving lets the grid grow back
        // to the remaining viewers. `resize_panes` is a no-op for non-shared sessions
        // and when nothing changed, so this only does work when a shared grid moves.
        self.resize_panes(wid);
        // Re-sync the NATIVE toolbar tab segments to this window's new tab state
        // (count / labels / active index). `sync_window` is the convergence point
        // after EVERY tab mutation (open / close / switch / detach / migrate), so the
        // native segments always track app state. A no-op off macOS / with no toolbar.
        self.refresh_window_tabs(wid);
    }

    /// Rebuild the notification SUPPRESSION SET: the active-tab FOCUSED-pane session
    /// id of every FOCUSED window. A notification mutes iff its session is in this
    /// set (the user is already looking at it in some focused window); an empty set
    /// (no focused window) delivers everything. Called on any focus or active-tab
    /// change. Cheap (one pass over the few live windows).
    fn recompute_notify_suppress(&self) {
        // Use `.get()` (not `[]`): this scans EVERY window, and a window may be
        // momentarily inconsistent mid-migration (its `tabs.active` past a just-
        // shortened `layouts`) when another window's `sync_window` triggers this — a
        // transiently-skipped window is re-folded by the next stable sync.
        let set: std::collections::HashSet<u64> = self
            .windows
            .values()
            .filter(|ws| ws.focused)
            .filter_map(|ws| ws.layouts.get(ws.tabs.active).map(|t| t.focus()))
            .collect();
        *self
            .notify_suppress
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = set;
    }

    fn sync_active_session(&mut self) {
        // The frontmost window is the mirror/title target.
        let Some(front) = self.frontmost_window else {
            return;
        };
        // The per-window half: re-mirror the FRONT window's active tab + reset its
        // in-flight state + request its redraw (no global handle touched here).
        self.sync_window(front);
        // Now re-point the GLOBAL control/notify handles at that window's active tab
        // (these follow the FRONTMOST window only). Re-read the just-mirrored window;
        // a stale id (gone between the two reads) is a no-op.
        // (The notification suppression set is rebuilt in `sync_window`, called just
        // above, so a tab switch's muting follows the new active tab automatically.)
        let App {
            windows,
            pool,
            active_handle,
            ..
        } = self;
        let Some(ws) = windows.get_mut(&front) else {
            return;
        };
        // The FRONT window's active tab's FOCUSED pane is the global active session.
        let aid = ws.layouts[ws.tabs.active].focus();
        let Some(s) = pool.get(aid) else {
            return;
        };
        // Point the control socket at the new active session so its text/drive/
        // scroll verbs follow tab switches (and don't break when an earlier tab,
        // incl. tab 0, closes). Auth is unaffected — only the target moves.
        {
            let mut g = active_handle.lock().unwrap_or_else(|p| p.into_inner());
            g.term = ws.term.clone();
            g.master = ws.master;
            g.id = s.id;
            g.ctx = s.ctx.clone();
        }
        // The window title is refreshed on the next redraw via `apply_title`; nudge
        // it now too so a switch with
        // no pending output still updates the chrome immediately. Capture the OS
        // handle + title (owned) so the `windows`/`pool` borrow is released before
        // the `&mut self` `apply_title` call.
        let nudge = ws
            .os_window
            .clone()
            .map(|w| (w, term_lock(&ws.term).title().to_string()));
        if let Some((w, title)) = nudge {
            self.apply_title(front, &w, &title);
        }
        // Live structural oracle (debug-only): after re-mirroring, the window/
        // session model is consistent — see `structural_invariants_ok`. Zero cost
        // in release. `sync_active_session` is the re-stabilization point reached
        // after every tab add/switch/cycle/close.
        debug_assert!(
            self.structural_invariants_ok(),
            "window/session structural invariants violated after sync_active_session",
        );
    }

    /// Re-mirror after a change to `wid`'s active tab/pane (append, switch, close,
    /// collapse): if `wid` is the FRONT window, re-point the GLOBAL control/notify
    /// handle too ([`Self::sync_active_session`]); otherwise just its per-window mirror
    /// ([`Self::sync_window`]). This is the invariant EVERY active-tab mutation must
    /// restore — a stale global handle drives control-socket verbs (`text`/`feed`/
    /// `signal`) at the WRONG, or a just-closed, session (and `Owner`/aterm-ctl verbs
    /// bypass the per-request edge gate, so they always hit whatever it points at).
    /// Mirrors the inline guard in `open_tab_in`.
    fn resync_active_or_window(&mut self, wid: WindowId) {
        if self.frontmost_window == Some(wid) {
            self.sync_active_session();
        } else {
            self.sync_window(wid);
        }
    }

    /// Build a MINIMAL headless `App` for the multi-window state-transition test:
    /// one window (`WindowId(0)`) with one stub tab (session 0, `master = -1` so
    /// `Session::drop` is a no-op — no real PTY), `frontmost = WindowId(0)`,
    /// `next_window_id = 1`, `next_session_id = 1`, `headless = true`, `proxy = None`
    /// (no event loop in a unit test). Every other field is the cheapest REAL value
    /// (a real CPU backend, registry, subscriber + image queues), so the tested seams
    /// — `insert_logical_window` / `install_window_state` / `close_window_logical` —
    /// exercise the genuine windows/pool/frontmost/`CloseOutcome` logic. It NEVER
    /// touches `proxy`/`session_factory` (no spawn), so `None`/an empty factory is
    /// fine. Threads spawned (notify delivery) are harmless and exit at process end.
    #[cfg(test)]
    fn headless_for_test() -> App {
        let session0 = stub_session(0);
        let term = session0.term.clone();
        let master = session0.master;
        let app_sink = session0.ctx.sink.clone();

        let store = session_store::new_store();
        App::register_session(&store, &session0, None);
        let subscribers = subscribe::new_registry();
        let image_queue: control::ImageQueue = Arc::new(Mutex::new(VecDeque::new()));

        let active_handle: control::ActiveHandle = Arc::new(Mutex::new(control::ActiveSession {
            term: term.clone(),
            master,
            id: 0,
            ctx: session0.ctx.clone(),
        }));

        // The initial window (id 0) is focused with tab 0's session (id 0) active, so
        // seed the suppression set with {0} — byte-identical to the old
        // focused=true/active=0 pair.
        let notify_suppress = Arc::new(Mutex::new(std::collections::HashSet::from([0u64])));
        let notify_tx = notify::spawn_delivery(notify_suppress.clone());

        let theme = Theme::default();
        // A real CPU renderer (the test env has a system monospace font, exactly as
        // the renderer crate's own tests rely on). `headless_for_test` doesn't render,
        // but `backend` is a non-optional field; a real one keeps the App honest.
        let backend = Backend::Cpu(
            Renderer::from_system(FONT_PX, theme).expect("system font for test backend"),
        );

        let session_factory = SessionFactory {
            // Minted from the single root authority (the test never spawns through it).
            spawn_cap: unsafe { aterm_cap::Authority::root_authority() }
                .grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted),
            sandbox_cap: unsafe { aterm_cap::Authority::root_authority() }
                .grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted),
            env_add: Vec::new(),
            exec_command: None,
            cwd: None,
            sandbox_wrap: None,
            terminal_config: None,
            integrate: false,
            lat_epoch: Instant::now(),
            last_output_ns: Arc::new(AtomicU64::new(0)),
            notify_tx,
        };

        let ws0 = WindowState::new(
            term.clone(),
            master,
            app_sink,
            0,
            24,
            80,
            TabIndex::new(0, 1),
            vec![pane::PaneTree::new(0)],
        );
        let mut pool = SessionPool::default();
        pool.insert(session0);

        App {
            pool,
            next_session_id: 1,
            hold: false,
            session_factory,
            proxy: None,
            active_handle,
            store,
            subscribers,
            backend,
            introspect_gpu: aterm_gpu::WindowGpu::new(),
            font_px: FONT_PX,
            default_font_px: FONT_PX,
            font_px_explicit: false,
            use_gpu: false,
            theme,
            font_family: None,
            option_as_meta: true,
            keybindings: keybinding::Keybindings::default(),
            windows: {
                let mut m = BTreeMap::new();
                m.insert(WindowId(0), ws0);
                m
            },
            frontmost_window: Some(WindowId(0)),
            focus_order: Vec::new(),
            next_window_id: 1,
            winit_to_window: HashMap::new(),
            headless: true,
            bell_beep: BellRateLimiter::new(BELL_BEEP_INTERVAL),
            image_queue,
            trace_latency: false,
            lat_epoch: Instant::now(),
            last_output_ns: Arc::new(AtomicU64::new(0)),
            notify_suppress,
            search_history_lines: MAX_SEARCH_HISTORY,
            _menu: None,
            _toolbars: BTreeMap::new(),
            tab_strip_rows: 0,
        }
    }

    /// Cmd-D / Cmd-Shift-D: split the FOCUSED pane of the active tab in `dir`,
    /// spawning a fresh session for the new pane via the SAME factory `open_tab`
    /// uses (own PTY/engine/policy/OSC52/reader + fresh nonce). The new session is
    /// sized to its sub-rect (so its app sees the right `SIGWINCH` geometry), and
    /// the original pane is resized to ITS sub-rect. Focus moves to the new pane.
    /// A spawn failure is logged and ignored (the layout is untouched, so the
    /// just-failed split never half-applies). Sized at the CURRENT grid; the
    /// post-split resize pass gives every pane its real sub-rect.
    fn split_focused_pane(&mut self, dir: pane::SplitDir) {
        // The split spawns a NEW session (views=1, never `attach`) owned by the
        // frontmost window, so its output/exit/bell route back to THIS window.
        let Some(owner) = self.frontmost_window else {
            return;
        };
        // INVARIANT: a SHARED (Cmd-Shift-O, views>1) session is NEVER split. A split
        // resizes the focused pane's session grid to its sub-rect; for a session
        // viewed in a co-viewing window that would corrupt the OTHER window's grid
        // (it shares the one live grid). Bail before spawning anything.
        if self
            .pool
            .views(self.focused_session_id(owner))
            .is_some_and(|v| v > 1)
        {
            return;
        }
        let id = self.next_session_id;
        let (rows, cols) = self.front().map_or((0, 0), |ws| (ws.rows, ws.cols));
        // A real run always has a proxy; guard rather than panic (test-only None).
        let Some(proxy) = self.proxy.clone() else {
            return;
        };
        // Spawn at the window grid; `resize_panes` immediately re-sizes every pane
        // (incl. this one) to its computed sub-rect, so the initial size is transient.
        match spawn_session(id, owner, rows, cols, &self.session_factory, &proxy) {
            Ok(session) => {
                self.next_session_id += 1;
                // The new pane is a child of the pane it was split from (family tree).
                let parent = self
                    .session_by_id(self.focused_session_id(owner))
                    .map(|s| s.ctx.self_id.clone());
                Self::register_session(&self.store, &session, parent);
                // A split ALWAYS inserts a brand-new view (views=1); it NEVER
                // `attach`es (that is only for same-session-in-two-windows).
                self.pool.insert(session);
                // Mutate the active tab's tree: the focused leaf becomes a split of
                // (original, new), focus moves to the new pane. A stale owner (gone
                // mid-spawn) leaves the just-inserted view orphaned — detach it.
                if let Some(tree) = self.active_tree_mut(owner) {
                    tree.split_focused(dir, id);
                } else {
                    self.pool.detach(id);
                    return;
                }
                // Size every pane in the active tab to its new sub-rect (the original
                // pane shrank to half; the new pane gets the other half).
                self.resize_panes(owner);
                self.sync_window(owner);
            }
            Err(e) => eprintln!("aterm-gui: could not split the pane: {e}"),
        }
    }

    /// Every logical window currently DISPLAYING `session` in a VISIBLE pane of its
    /// active tab — the FOCUSED pane OR a split sibling. This is the SINGLE co-viewer
    /// routing predicate: the `Wake::Output` repaint fan-out routes through it, and
    /// the multi-window detach/migrate/share tests assert against it, so the test
    /// oracle and the live routing can never diverge. (The `Wake::Bell` arm inlines
    /// the same `layouts[active].contains` predicate because it must `get_mut` each
    /// window to ring its flash, which an immutable iterator can't express.)
    fn windows_displaying(&self, session: u64) -> impl Iterator<Item = WindowId> + '_ {
        self.windows
            .iter()
            .filter(move |(_, ws)| ws.layouts[ws.tabs.active].contains(session))
            .map(|(wid, _)| *wid)
    }

    /// Whether session `id` is VISIBLE in ANY window right now — a pane of some
    /// window's ACTIVE tab (the focused pane OR any of its split siblings). Output
    /// from any visible pane must repaint that window; only background TABS are gated
    /// out. Generalizes both the background-pane gate AND the multi-window co-viewer
    /// case into one predicate.
    fn is_visible_session(&self, id: u64) -> bool {
        self.windows
            .values()
            .any(|ws| ws.layouts[ws.tabs.active].contains(id))
    }

    /// The grid geometry a SHARED (Cmd-Shift-O, `views > 1`) session must be sized
    /// to: the ELEMENT-WISE MINIMUM `(rows, cols)` across every window currently
    /// DISPLAYING it — i.e. where it is in the window's ACTIVE tab (the same
    /// visible-pane predicate as [`Self::windows_displaying`]; a shared session is
    /// never split, so it fills each such window's grid). One `Arc<Terminal>` grid
    /// cannot be two sizes — sizing it to the min of its FOREGROUND viewers lets each
    /// render it without OVER-reading: a larger window letterboxes the surplus (the
    /// engine blank-fills out-of-grid rows/cols), a smaller one sees the min exactly.
    ///
    /// CRITICAL: this uses the ACTIVE-tab predicate, NOT "any tab" — a window where
    /// the session sits in a BACKGROUND tab is NOT painting it, so folding that
    /// window's size into the min would shrink the grid for the actual foreground
    /// viewer. When the session is in NO window's active tab (backgrounded
    /// everywhere) the min is empty: KEEP its current grid size rather than collapse
    /// it to 1×1 (its running program must not be SIGWINCH'd to nothing while merely
    /// off-screen); `switch_tab_in` re-fits it the moment it returns to a foreground.
    /// Clamped to ≥ 1×1.
    fn shared_target_geometry(&self, id: u64) -> (u16, u16) {
        self.windows
            .values()
            // `.get()` (not `[]`): this scans EVERY window and is reachable from
            // `sync_window`, where another window may be momentarily inconsistent
            // mid-migration (its `tabs.active` past a just-shortened `layouts`) — the
            // same `.get()` discipline `recompute_notify_suppress` uses on that line.
            .filter(|ws| {
                ws.layouts
                    .get(ws.tabs.active)
                    .is_some_and(|t| t.contains(id))
            })
            .map(|ws| (ws.rows, ws.cols))
            .reduce(|(ar, ac), (br, bc)| (ar.min(br), ac.min(bc)))
            .map_or_else(
                || {
                    // Visible in no active tab → leave the grid as it is.
                    self.pool.get(id).map_or((1, 1), |s| {
                        let t = term_lock(&s.term);
                        (t.rows().max(1), t.cols().max(1))
                    })
                },
                |(r, c)| (r.max(1), c.max(1)),
            )
    }

    /// Focus change: an unfocused window draws the cursor as a steady hollow
    /// block regardless of DECSCUSR (standard terminal behavior) and stops
    /// blink scheduling; regaining focus restores the app's style and re-arms
    /// the blink (via `about_to_wait`).
    fn on_focus(&mut self, wid: WindowId, focused: bool) {
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.focused = focused;
        }
        // Republish the per-window-correct notification suppression set: this
        // window's focus changed, so its active tab joins/leaves the muted set.
        self.recompute_notify_suppress();
        // Phase 0.5: the focus-report EGRESS (ESC[I / ESC[O under DEC 1004) goes
        // through the seam so a controller `focus` verb can satisfy a focus-
        // tracking app's oracle too (kills j). The GUI-VISUAL side-effects below
        // (cursor-style override, blink reset) are not egress and stay here. Routed
        // to THIS window's session.
        self.input(wid, InputEvent::Focus(focused), Source::Human);
        // The hollow-cursor override is re-applied per-window in `redraw_window`, so
        // do NOT push it into the shared backend here.
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.blink_phase = true;
        }
        self.sync_blink_phase();
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.next_blink = None;
        }
        if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
            w.request_redraw();
        }
    }

    /// BEL reached a tab's engine: audible beep (rate-limited), visual flash
    /// (repaint now; `about_to_wait` arms the un-flash wake), and ask the OS to
    /// mark the window urgent (Dock bounce / taskbar highlight) when the bell
    /// can't otherwise be seen — the tmux bell-on-activity flow. `session` is the
    /// originating tab: attention is requested when the window is unfocused OR the
    /// bell came from a BACKGROUND tab (its flash isn't on the visible screen), so
    /// a background tab's activity still surfaces even on a focused window.
    fn on_bell(&mut self, window: WindowId, session: u64) {
        // The `Wake::Bell` spawn stamp `window` may be STALE (a migrate/detach moved
        // the session's tab to another window), so we do NOT trust it for the flash.
        let _ = window;
        let now = Instant::now();
        if self.bell_beep.try_fire(now) {
            // The user's configured macOS alert sound. AppKit is already
            // in-process (winit); safe to call from the main thread.
            #[cfg(target_os = "macos")]
            unsafe {
                objc2_app_kit::NSBeep();
            }
        }
        // Background iff this session is NOT in a VISIBLE pane of ANY window's
        // active tab (the focused pane OR a split sibling) — the generalized
        // visible-pane predicate (also covers a co-viewer window).
        let background = !self.is_visible_session(session);
        // Flash every window whose ACTIVE tab actually displays this session —
        // found by scanning the windows, NOT the stale stamp. Collect the matching
        // ids first to drop the immutable borrow before mutating each window.
        let flashing: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, ws)| ws.layouts[ws.tabs.active].contains(session))
            .map(|(wid, _)| *wid)
            .collect();
        for wid in flashing {
            if let Some(ws) = self.windows.get_mut(&wid)
                && let Some(w) = ws.os_window.clone()
            {
                ws.bell_flash.ring(now);
                w.request_redraw();
                if !ws.focused || background {
                    w.request_user_attention(Some(UserAttentionType::Informational));
                }
            }
        }
    }

    /// Publish the current visible screen to the macOS accessibility tree.
    ///
    /// Real only under the off-by-default `a11y-appkit` feature on macOS; a no-op
    /// otherwise, so the call site in `redraw` stays unconditional. Builds the
    /// accessibility snapshot from the just-rendered `input_scratch` (same cells the
    /// frame was drawn from) and hands its text/role/label to the content NSView.
    #[cfg(all(target_os = "macos", feature = "a11y-appkit"))]
    fn update_accessibility(&mut self, id: WindowId, window: &Window) {
        let Some(ws) = self.windows.get(&id) else {
            return;
        };
        // The tab strip occupies the top `tab_strip_rows` rows of `input_scratch`;
        // it is window CHROME, not terminal content, so the accessibility snapshot
        // skips it (a screen reader reads the terminal grid only). The cursor row is
        // shifted down by the strip in the composed frame; subtract it back so the
        // snapshot's cursor is in terminal-grid coordinates. A no-op offset when the
        // strip is disabled.
        let strip = self.tab_strip_rows as usize;
        let cells = ws.input_scratch.cells.get(strip..).unwrap_or(&[]);
        let cursor = ws.input_scratch.cursor_visible.then_some((
            ws.input_scratch.cursor_row.saturating_sub(strip),
            ws.input_scratch.cursor_col,
        ));
        let snap = accessibility::AccessibleSnapshot::from_cells(cells, ws.cols as usize, cursor);
        accessibility::apply_to_ns_view(window, &snap);
    }

    /// No-op accessibility publish (feature off / non-macOS).
    #[cfg(not(all(target_os = "macos", feature = "a11y-appkit")))]
    #[inline]
    fn update_accessibility(&mut self, _id: WindowId, _window: &Window) {}

    /// Paste the macOS system clipboard (`pbpaste`) to the PTY via
    /// [`Terminal::format_paste`], which strips control bytes a hostile
    /// clipboard could use to escape the guards (ESC, C1 controls), converts
    /// line breaks to CR, and wraps the body in the bracketed-paste guards
    /// (ESC[200~ .. ESC[201~) when the app has enabled bracketed paste — so
    /// editors/shells treat it as inert pasted text.
    fn paste_clipboard(&mut self) {
        // A window-level command (Cmd-V / menu Paste): targets the frontmost window.
        let Some(wid) = self.frontmost_window else {
            return;
        };
        // UTF-8-pinned (see `control::clipboard_command`): otherwise a Finder/.app
        // launch's non-UTF-8 process locale makes pbpaste transcode multibyte
        // clipboard text to the C codeset — mojibake, and a literal `?` for chars
        // it can't map — before we ever decode it. With the pin, pbpaste emits
        // clean UTF-8 and the `from_utf8_lossy` below round-trips it intact.
        let Ok(out) = control::clipboard_command("pbpaste").output() else {
            return;
        };
        if !out.status.success() || out.stdout.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        // Route through the seam so paste-formatting + the snap-to-bottom side
        // effect converge with the controller `paste` verb.
        self.input(wid, InputEvent::Paste(text), Source::Human);
    }
}

impl ApplicationHandler<Wake> for App {
    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        // A `WaitUntil` deadline fired: a bell-flash end and/or a blink tick. On a
        // single `ResumeTimeReached` wake, several windows' deadlines may have
        // passed at once, so service EVERY window (not just the frontmost).
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            let now = Instant::now();
            let mut to_redraw: Vec<WindowId> = Vec::new();
            for (id, ws) in self.windows.iter_mut() {
                // Flash over: repaint the normal (un-inverted) frame.
                let mut dirty = ws.bell_flash.expire(now);
                // Blink tick: flip the phase and arm the next half-period
                // (`about_to_wait` re-schedules). Gated on the armed deadline so an
                // earlier bell-flash wake doesn't clip the period.
                if ws.next_blink.is_some_and(|d| now >= d) {
                    ws.blink_phase = !ws.blink_phase;
                    ws.next_blink = Some(now + BLINK_INTERVAL);
                    dirty = true;
                }
                if dirty {
                    to_redraw.push(*id);
                }
            }
            for id in to_redraw {
                if let Some(w) = self.windows.get(&id).and_then(|ws| ws.os_window.as_ref()) {
                    w.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // Fold the MIN deadline across ALL windows. Each window independently may be
        // blinking (focused + visible Blinking* cursor) and/or have a pending bell
        // flash; arm winit's single timer at the earliest deadline, and sleep in
        // pure `Wait` iff none is armed (preserving the 0%-idle property for
        // steady/unfocused/hidden/headless sessions).
        let mut deadline: Option<Instant> = None;
        // Iterate the windows in place (no per-wake Vec<WindowId> snapshot, no
        // redundant per-window lookup). The blink predicate — a real focused window
        // with a visible `Blinking*` cursor; anything else (steady, unfocused,
        // hidden, headless) leaves the loop in pure `Wait` — is inlined on the
        // `&mut WindowState` in hand.
        let headless = self.headless;
        for ws in self.windows.values_mut() {
            // Read the blink predicate WITHOUT blocking on the engine lock. Under
            // heavy PTY output the reader thread holds `term` for the whole of
            // `process(&buf)`; a blocking `lock()` here would stall the event loop
            // behind the engine, adding input latency exactly when the terminal is
            // busiest. `try_lock` instead: `Some(bool)` when we read it, `None`
            // when the engine is mid-process. Blink is cosmetic — a one-tick-late
            // update is imperceptible, and the existing schedule is kept meanwhile.
            let blinking: Option<bool> = if !headless && ws.os_window.is_some() && ws.focused {
                ws.term.try_lock().ok().map(|term| {
                    term.cursor_visible()
                        && matches!(
                            term.cursor_style(),
                            CursorStyle::BlinkingBlock
                                | CursorStyle::BlinkingUnderline
                                | CursorStyle::BlinkingBar
                        )
                })
            } else {
                Some(false) // headless / no window / unfocused: definitely not blinking
            };
            match blinking {
                Some(true) => {
                    let d = *ws
                        .next_blink
                        .get_or_insert_with(|| Instant::now() + BLINK_INTERVAL);
                    deadline = Some(deadline.map_or(d, |b| b.min(d)));
                }
                Some(false) => {
                    // Not blinking: disarm and leave the cursor SOLID so a steady
                    // style is never stuck "off"; repaint the window we just flipped.
                    ws.next_blink = None;
                    if !ws.blink_phase {
                        ws.blink_phase = true;
                        if let Some(w) = ws.os_window.as_ref() {
                            w.request_redraw();
                        }
                    }
                }
                None => {
                    // Engine mid-process: leave blink state untouched and keep
                    // folding any already-armed deadline so blinking continues
                    // uninterrupted once the burst clears.
                    if let Some(d) = ws.next_blink {
                        deadline = Some(deadline.map_or(d, |b| b.min(d)));
                    }
                }
            }
            if let Some(d) = ws.bell_flash.deadline() {
                deadline = Some(deadline.map_or(d, |b| b.min(d)));
            }
        }
        match deadline {
            Some(d) => el.set_control_flow(ControlFlow::WaitUntil(d)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        // Event-driven: sleep until the PTY produces output or the user acts,
        // instead of busy-polling. Redraws are requested explicitly on Wake.
        el.set_control_flow(ControlFlow::Wait);
        // Headless: run the engine + control socket + offscreen rendering, but
        // never open a window. The front window keeps `os_window: None`/`present:
        // None`, so `redraw()` is a no-op, and the winit run loop still delivers
        // `user_event` (Wake::Control for `image`, Wake::Snapshot, Wake::Exit)
        // with no window present.
        if self.headless {
            return;
        }
        // Idempotence: if the frontmost logical window already has an OS window
        // attached, do nothing (a second `resumed`).
        let Some(target_id) = self.frontmost_window else {
            return;
        };
        if self
            .windows
            .get(&target_id)
            .is_some_and(|ws| ws.os_window.is_some())
        {
            return;
        }
        if !self.attach_os_window(el, target_id) {
            // The FIRST/only window's GPU surface failed: there is no other window to
            // keep the app alive and no CPU fallback in GPU mode — exit rather than
            // run blind with a black screen.
            eprintln!("aterm-gui: could not create the initial window surface; exiting");
            el.exit();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: Wake) {
        match ev {
            // A pane produced output. Its reader thread already fed the matching
            // engine (it holds that session's own `term`); the main thread only
            // needs to repaint, and ONLY when the producing pane is VISIBLE — a pane
            // of some window's active tab (the focused pane OR a split sibling). A
            // background TAB's output updates its off-screen grid silently. The
            // stamped `window` is the owning-window hint; the visible-pane scan
            // generalizes to co-viewers (same-session-in-two-windows). With one pane
            // per tab and one window this is the old unconditional request_redraw.
            Wake::Output { session, window } => {
                // P1.3 NOTIFY HOOK: ONE non-blocking line — wake every live
                // subscriber of this session so it re-reads the latest state and
                // pushes a coalesced delta. The notify is a single-slot
                // `try_send` (drops on a full slot / dead receiver), so a slow or
                // dead subscriber can NEVER block this GUI thread or backpressure
                // the producing session. A session with no subscribers is a cheap
                // O(1) miss. KEPT first + unconditional (the headless ordering —
                // runs even with no os_window).
                // Lock-free fast-path: skip the mutex entirely when nobody is
                // subscribed (the common case) — a single Relaxed atomic load instead
                // of an acquire/release on EVERY output burst. The lock + notify still
                // run, unchanged, whenever a subscriber exists.
                if self.subscribers.any() {
                    self.subscribers
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .notify(session);
                }
                // Repaint every window that currently DISPLAYS this session in a
                // VISIBLE pane of its active tab (the focused pane OR a split
                // sibling). A background tab's output updates its off-screen grid
                // silently. The stamped `window` is the owning-window hint; the
                // visible-pane scan generalizes to co-viewers, so prefer it. At n==1
                // the only displaying window is the front one — identical behavior.
                // With same-session-in-two-windows (Open Session in New Window /
                // Cmd-Shift-O) this fan-out is GENUINELY exercised: a shared session
                // yields BOTH viewing windows here, so one session's output repaints
                // every window that can see it. Nothing assumes a session is shown by
                // ≤1 window.
                let _owner = window; // owning-window hint (S10 co-viewer route)
                // Route through the shared `windows_displaying` predicate (no per-chunk
                // Vec alloc — iterate it directly; `request_redraw` only needs `&self`).
                for wid in self.windows_displaying(session) {
                    if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
                        w.request_redraw();
                    }
                }
            }
            // A tab's shell/`-e` command exited. Close only THAT tab; exit the app
            // only when it was the last (and `--hold` keeps even that open). With
            // one tab and no `--hold`, this exits the app exactly as before.
            Wake::Exit { session, window } => {
                // P1.1: mark the registry handle `Exited`, then close the session in
                // EVERY window that views it. The STALE spawn-stamped `window` is
                // ignored — a migrate / detach moves a tab's panes to a DIFFERENT
                // window, and a Cmd-Shift-O SHARE views one session in several windows
                // off a single reader thread (one `Wake::Exit`), so the close must
                // scan all current viewers, not the stamp. `exit_session_logical`
                // returns the windows whose LAST tab thereby closed; escalate each to
                // a window close, which exits the app IFF it was the last window (the
                // `ExitIffEmpty` invariant). At n==1 this exits the app exactly as
                // before. An already-closed/unknown session closes nothing.
                let _ = window;
                for o in self.exit_session_logical(session) {
                    self.close_window(el, o);
                }
            }
            Wake::Snapshot => self.snapshot(),
            // BEL on any tab flashes the (shared) window — the standard
            // "bell on activity" affordance; a background tab's bell additionally
            // requests user attention so off-screen activity still surfaces.
            Wake::Bell { session, window } => self.on_bell(window, session),
            Wake::Control => {
                // Drain off-lock so the control thread can keep queuing, then
                // render each request and reply with the frame dimensions. A
                // dropped receiver (dead client) just makes send() fail; ignore.
                // Poison-recovery (matches `term_lock`): a panicked control thread
                // must not abort the whole GUI — recover the queue and drain it.
                let reqs: Vec<control::ImageReq> = self
                    .image_queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .drain(..)
                    .collect();
                for req in reqs {
                    let dims = self.render_image(&req.target);
                    let _ = req.reply.send(dims);
                }
            }
            // The `chrome` verb wants the frontmost window's NATIVE UI (NSToolbar
            // + app menu bar). Only the main thread may touch AppKit, so we read it
            // HERE and reply with the text lines. A dropped receiver (dead client)
            // just makes send() fail; ignore.
            Wake::ReadChrome { reply } => {
                let lines = self.read_native_chrome();
                let _ = reply.send(lines);
            }
            // A native tab segment was clicked (toolbar NSSegmentedControl) — switch
            // `window` to tab `index`. `switch_tab_in` is window-aware and re-mirrors
            // the window (`sync_active_session` when it is the front window); the
            // native segments then re-track via `refresh_window_tabs` in `sync_window`.
            Wake::SelectTab { window, index } => {
                self.switch_tab_in(window, index);
            }
            // A native tab's close × was clicked — close tab `index` of `window` as a
            // unit via the SAME path the renderer strip's `✕` / the `tab close` verb
            // take. If it was the window's LAST tab, flag + escalate (we have `el`
            // here) so the window tears down, exactly like a tab-strip close.
            Wake::CloseTab { window, index } => {
                if self.close_tab_at(window, index)
                    && let Some(ws) = self.windows.get_mut(&window)
                {
                    ws.pending_close = true;
                }
                self.escalate_pending_close(el);
            }
            // The control socket's `tab` verb (new/<N>/next/prev), driving the FRONT
            // window's tabs. Resolve the front window HERE (only the main thread can),
            // apply the action via the SAME command paths the keyboard/menu use, then
            // reply with the resulting `(active_index, tab_count)`. A window with no
            // tabs / no front window replies `(0, 0)`.
            Wake::TabCmd { action, reply } => {
                let state = self.apply_tab_cmd(action);
                // A `tab close` of the front window's LAST tab flags `pending_close`;
                // escalate it (we have `el` here) so the window actually tears down —
                // mirrors the keyboard/menu/strip close paths.
                self.escalate_pending_close(el);
                let _ = reply.send(state);
            }
            // The `window` verb wants the frontmost window's ENTIRE on-screen pixels
            // (OS chrome + content) as a PNG. Only the main thread may touch AppKit
            // and read the window number, so we capture HERE and reply with the PNG
            // dims (or a clear error). A dropped receiver (dead client) just makes
            // send() fail; ignore.
            Wake::CaptureWindow { path, reply } => {
                let result = self.capture_window(&path);
                let _ = reply.send(result);
            }
            // The user edited the config file (the watcher saw its mtime change):
            // re-read + validate + apply to every live session. A malformed
            // mid-edit file is rejected (the previous config stays intact).
            Wake::ConfigReload => self.reload_config(),
            // Phase 0.5 (A.2.3): apply a controller-built batch on the main thread
            // (the sole owner of term geometry + gesture state + the encoders), IN
            // ORDER, so a press/move/release gesture lands atomically in this one
            // turn. `src` is recorded for audit and NEVER branched in `input`. The
            // reply (if any) carries the LAST event's outcome — sufficient for the
            // single-event reply-bearing verbs (resize, refused scroll).
            Wake::Input { batch, src, reply } => {
                // The control socket's input targets the active/front window (its
                // design contract). At n==1 this is the only window.
                let Some(wid) = self.frontmost_window else {
                    return;
                };
                let mut outcome = InputOutcome::Ok;
                for ev in batch {
                    outcome = self.input(wid, ev, src);
                }
                if let Some(tx) = reply {
                    let _ = tx.send(outcome); // mirrors the Wake::Control reply
                }
            }
            // A macOS menu item was clicked (menu.rs posted it). Dispatch into the
            // SAME command method the matching keybinding uses — no behavior is
            // duplicated; the menu is just a second entry point. `el` is needed
            // because Quit / the last-tab Close must exit the loop.
            Wake::MenuAction { action } => self.dispatch_menu_action(el, action),
            // Cmd-N from the keyboard path (which has no `el`): open a real new
            // in-process window. Headless never opens an OS window and keeps exactly
            // ONE logical window, so a CreateWindow there is ignored.
            Wake::CreateWindow => {
                if self.headless {
                    aterm_log::info!("headless: ignoring CreateWindow");
                } else {
                    self.create_window_internal(el);
                }
            }
            // Cmd-Shift-N from the keyboard path (which has no `el`): move the
            // frontmost window's active tab out into a fresh in-process window. A
            // single-tab source window is a no-op (detaching its only tab would just
            // relocate the window). Under headless the logical move applies but no OS
            // surface is attached.
            Wake::DetachActiveTab => self.detach_active_tab(el),
            // Cmd-Shift-D from the keyboard path (which has no `el`): open the frontmost
            // window's ACTIVE session in a SECOND window, so the same live grid is
            // visible in two windows at once. ADDS a view (the source keeps its tab);
            // the PTY survives until both viewers close. Under headless the logical
            // attach applies but no OS surface is attached.
            Wake::ViewActiveSessionInNewWindow => self.open_active_session_in_new_window(el),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WinitWindowId, event: WindowEvent) {
        // Resolve the winit id to our logical WindowId. An event for an unknown
        // window (e.g. one already torn down) is ignored.
        let Some(wid) = self.winit_to_window.get(&id).copied() else {
            return;
        };
        match event {
            // Close JUST this window (its red traffic-light / Window ▸ Close
            // window). The app exits only when it was the LAST window.
            WindowEvent::CloseRequested => self.close_window(el, wid),
            WindowEvent::RedrawRequested => self.redraw_window(wid),
            WindowEvent::Focused(f) => {
                if f {
                    // Track focus order (MRU) so a later close of the front window
                    // re-points to the window the OS will raise, not the lowest id.
                    self.note_window_focused(wid);
                    // Re-point the control socket / notify_active / registry title at
                    // the now-front window, exactly like a tab switch — but ONLY when
                    // the frontmost window actually CHANGES. `sync_active_session`
                    // also clears in-flight find/selection + forces a repaint, which
                    // must NOT happen on a same-window focus-gain (clicking back into
                    // the one window must preserve its selection/find). At n==1
                    // frontmost is already this window, so the guard skips the sync →
                    // byte-identical; with a 2nd window, cross-window focus changes
                    // frontmost and the sync runs (the multi-window hook).
                    if self.frontmost_window != Some(wid) {
                        self.frontmost_window = Some(wid);
                        self.sync_active_session();
                    }
                }
                self.on_focus(wid, f);
            }
            WindowEvent::ModifiersChanged(m) => {
                if let Some(ws) = self.windows.get_mut(&wid) {
                    ws.mods = m.state();
                }
                self.update_hover_cursor(wid);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(wid, event);
                // Cmd-W set `pending_close` iff it closed a window's LAST tab — on the
                // FRONTMOST window (which `close_active_tab` operates on), NOT this
                // event window `wid`; the two diverge when OS keyboard focus lags the
                // logical frontmost (after a migrate/detach/new-window with no OS focus
                // move). Escalate the window that actually carries the flag.
                self.escalate_pending_close(el);
            }
            // IME-1: composition events. Without this arm winit's IME was dropped
            // by the catch-all, so CJK/dead-key/Option composition never worked.
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(text, _cursor) => self.on_ime_preedit(wid, text),
                Ime::Commit(text) => self.on_ime_commit(wid, text),
                // Enabled/Disabled: clear any stale composition so suppression
                // can't get stuck on (e.g. focus loss mid-composition).
                Ime::Enabled | Ime::Disabled => self.on_ime_preedit(wid, String::new()),
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.on_cursor_moved(wid, position.x, position.y);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_input(wid, state, button);
                // A tab-strip click closing the last tab sets the clicked window's
                // `pending_close`; escalate whichever window carries the flag (the
                // app exits only when that was the last window), exactly like Cmd-W.
                self.escalate_pending_close(el);
            }
            WindowEvent::MouseWheel { delta, .. } => self.on_mouse_wheel(wid, delta),
            WindowEvent::Resized(size) => {
                self.on_resize(wid, size);
                if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
                    w.request_redraw();
                }
            }
            // HiDPI follow-through: the window moved to a display with a different
            // scale factor (or its display's scale changed). Re-derive the auto-scaled
            // font + interior pad for the new DPI. `..` ignores `inner_size_writer` so
            // winit applies its default logical-size-preserving resize, whose `Resized`
            // then re-grids the window at the new cell metrics.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.on_scale_factor_changed(scale_factor);
            }
            _ => {}
        }
    }
}

// PTY spawn lives in `aterm-pty` (the single WS-G spawn seam); the frontend
// passes it the shell-integration injection computed below.

/// Minimal, hand-rolled CoreGraphics / CoreFoundation FFI for the `window`
/// control-socket verb's full-window capture. The workspace has no `core-graphics`
/// crate, so we declare exactly the symbols this one capture needs, well-commented
/// with SAFETY notes. CoreGraphics + CoreFoundation are already linked in-process
/// (AppKit pulls them in), so these `#[link]`s add bindings, not a new dependency.
///
/// All of these are documented C ABI functions whose contracts we honour below; the
/// pointer types are kept opaque (`*mut c_void`) since we only pass them straight
/// back to other CG calls or release them.
#[cfg(target_os = "macos")]
mod cg_capture {
    use std::ffi::c_void;

    /// Opaque CoreGraphics object pointers (we never deref them in Rust — they are
    /// handed back to CG or released).
    pub type CGImageRef = *mut c_void;
    pub type CGContextRef = *mut c_void;
    pub type CGColorSpaceRef = *mut c_void;

    /// A `CGRect` as CoreGraphics lays it out (two `CGFloat` pairs). We only ever
    /// pass `CGRectNull`, so the exact field values never matter beyond the layout.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }

    /// `CGRectNull` — the documented sentinel that tells `CGWindowListCreateImage`
    /// to use the window's own bounds (we ALSO pass `…IgnoreFraming`, so the result
    /// is exactly the window's on-screen rectangle). Its components are `CGFLOAT_MAX`
    /// per the CoreGraphics headers; passing it by value matches the C ABI.
    pub const CG_RECT_NULL: CGRect = CGRect {
        origin: CGPoint {
            x: f64::MAX,
            y: f64::MAX,
        },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };

    // `CGWindowListOption` / `CGWindowImageOption` bit flags (from CGWindow.h).
    /// Capture only the single window named by the windowID argument.
    pub const K_CG_WINDOW_LIST_OPTION_INCLUDING_WINDOW: u32 = 1 << 3;
    /// Exclude the window-server drop shadow / framing padding, so the image is the
    /// window's own pixels — chrome included, but no extra transparent margin.
    pub const K_CG_WINDOW_IMAGE_BOUNDS_IGNORE_FRAMING: u32 = 1 << 0;
    /// Capture at the display's full (Retina) resolution rather than a downscaled
    /// point-size image, so the PNG is pixel-accurate.
    pub const K_CG_WINDOW_IMAGE_BEST_RESOLUTION: u32 = 1 << 5;

    // `CGBitmapInfo` / `CGImageAlphaInfo` — RGBA8, alpha last, premultiplied. This is
    // the format we ASK `CGBitmapContextCreate` for, so the bytes we read back are
    // tightly-packed RGBA8 regardless of the source CGImage's native layout.
    pub const K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST: u32 = 1;
    pub const BITS_PER_COMPONENT: usize = 8;
    pub const BYTES_PER_PIXEL: usize = 4;

    // SAFETY (whole block): these are the standard, stable CoreGraphics /
    // CoreFoundation C entry points with the signatures published in Apple's
    // headers. We uphold each contract at the call site in `capture_window_pixels`:
    // every `Create`d object is released exactly once, every pointer passed in is a
    // live object we created or got from CG, and the draw target is a context whose
    // backing buffer outlives the read.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        /// Photograph one or more on-screen windows into a CGImage. Returns NULL when
        /// it cannot capture — notably when Screen Recording permission is not
        /// granted, which we surface as a clear, actionable error.
        pub fn CGWindowListCreateImage(
            screen_bounds: CGRect,
            list_option: u32,
            window_id: u32,
            image_option: u32,
        ) -> CGImageRef;

        pub fn CGImageGetWidth(image: CGImageRef) -> usize;
        pub fn CGImageGetHeight(image: CGImageRef) -> usize;
        pub fn CGImageRelease(image: CGImageRef);

        /// Make a device-RGB colour space for the destination bitmap context.
        pub fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
        pub fn CGColorSpaceRelease(space: CGColorSpaceRef);

        /// Create an RGBA8 bitmap-backed context we control the layout of. Passing a
        /// NULL `data` lets CG allocate the backing store (read back later via
        /// `CGBitmapContextGetData`).
        pub fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: CGColorSpaceRef,
            bitmap_info: u32,
        ) -> CGContextRef;
        pub fn CGBitmapContextGetData(context: CGContextRef) -> *mut c_void;
        pub fn CGContextRelease(context: CGContextRef);

        /// Draw the captured CGImage into our known-format context, normalizing its
        /// pixels to tightly-packed RGBA8 regardless of the source layout.
        pub fn CGContextDrawImage(context: CGContextRef, rect: CGRect, image: CGImageRef);
    }

    // NOTE: we deliberately take the ROBUST bitmap-context route — draw the captured
    // CGImage into an RGBA8 `CGBitmapContext` we create, then read its tightly-packed
    // backing buffer (see `capture_window_pixels`). That normalizes the pixel layout
    // regardless of the source CGImage's native format, so we never touch the source
    // image's data provider / `CFData` directly — hence no CoreFoundation / `CFData`
    // / `CGImageGet{DataProvider,BytesPerRow,BitsPerPixel,Alpha/BitmapInfo}` bindings
    // are needed here. The context owns its buffer until `CGContextRelease`, so there
    // is no CF object for us to `CFRelease` either.
}

fn main() {
    // CLI first: `-e <cmd>` to run a command instead of $SHELL, `-d <dir>` to set
    // the working directory; `--help`/`--version` print and exit before any setup.
    // A Finder/.app launch passes no args, so this is a no-op there and a normal
    // interactive shell starts.
    let Cli {
        exec_command,
        cwd,
        hold,
    } = parse_cli();
    // Diagnostics first, before any thread spawns: without a logger every
    // aterm_log record — including containment_audit denials — is discarded.
    logging::init();
    // Self-update apply, BEFORE any thread spawn or window: if a previous run
    // staged a verified, strictly-newer build, swap aterm.app in place and re-exec
    // the new binary (never returns on success). A no-op for dev/`cargo run`
    // builds, when nothing is staged, or when the updater is disabled/unpinned —
    // see crate aterm-update. Running here keeps the env-var loop-guard single-
    // threaded and avoids swapping a bundle with the engine already live.
    match aterm_update::apply_staged_if_ready(build_info::BUILD_NUMBER.parse::<u64>().unwrap_or(0))
    {
        aterm_update::ApplyOutcome::NotApplicable | aterm_update::ApplyOutcome::NoUpdate => {}
        other => eprintln!("aterm-gui: update apply: {other:?}"),
    }
    // SEC-1: establish the containment mode ONCE, here in the trusted launcher,
    // before any subsystem (the spawn seam, the control socket) queries it. The
    // launcher owns the mode (ATERM_DESIGN §5): `ATERM_CONTAINMENT_MODE` selects
    // it, defaulting to `User` (standard safeguards) for an interactive launch.
    // A bad/unparseable value fails closed: we fall back to `Containment`, the
    // most restrictive mode, and log the rejection rather than silently widening.
    let containment_mode = aterm_containment::init_mode_from_env(ContainmentMode::User)
        .unwrap_or_else(|e| {
            eprintln!(
                "aterm-gui: invalid ATERM_CONTAINMENT_MODE ({e}); falling back to \
                 Containment (most restrictive)"
            );
            // The parse error happened before init_mode ran, so the OnceLock is
            // still unset — set it to the fail-closed default now and proceed.
            let _ = aterm_containment::init_mode(ContainmentMode::Containment);
            ContainmentMode::Containment
        });
    eprintln!("aterm-gui: containment mode = {containment_mode}");
    // $ATERM_HEADLESS: bind the control socket and run the engine + offscreen
    // renderer without ever opening a window (clean automated introspection).
    let headless = std::env::var_os("ATERM_HEADLESS").is_some();
    // User config (~/.config/aterm/aterm.toml). Precedence: env > config > default.
    let config = load_config();
    // Initial grid size: env `ATERM_COLUMNS`/`ATERM_LINES` (set by --columns/--lines)
    // win, else config `columns`/`lines`, else 24×80 — all clamped sane. This is the
    // TERMINAL grid; the window is grown by `tab_strip_rows` extra pixel rows for the
    // tab strip (`resumed`), so the terminal keeps its configured `lines`.
    let cols = env_u16("ATERM_COLUMNS")
        .or(config.columns)
        .unwrap_or(80)
        .clamp(20, 500);
    let rows = env_u16("ATERM_LINES")
        .or(config.lines)
        .unwrap_or(24)
        .clamp(5, 300);
    // Rows reserved at the TOP of the window for the visible tab strip (env > config
    // > default 1). `0` is the byte-identical no-strip path.
    let tab_strip_rows = resolve_tab_strip_rows(&config);
    // Glyph rasterization size in PHYSICAL pixels. $ATERM_FONT_PX overrides the
    // config / 13 px default (clamped to a sane 6..=200), e.g. 26 on a 2× Retina
    // display for native-size text — see the HiDPI note at window creation.
    // Precedence lives in `resolve_font_px` so a live config reload re-applies it
    // identically (an env override still wins after an edit).
    let mut font_px: f32 = resolve_font_px(&config);
    // Was the size set EXPLICITLY (env or config), or is it the built-in default?
    // The HiDPI auto-scale in `resumed()` only kicks in for the default, so an
    // explicit size is never double-scaled. (Mirrors `resolve_font_px`'s
    // env > config > default precedence: either source counts as explicit.)
    let font_px_explicit = std::env::var_os("ATERM_FONT_PX").is_some() || config.font_px.is_some();
    // GPU (Metal) is the DEFAULT on macOS: the CPU renderer re-rasterizes every
    // glyph on heavy full-screen colour output (the dominant per-frame cost for
    // streaming TUIs like Claude Code), while the GPU path re-encodes cached glyph
    // instances instead. Init is robust — a missing/failed device falls back to the
    // CPU renderer below. Opt-out precedence, most specific first: `--cpu`/$ATERM_CPU
    // force CPU (and already cleared $ATERM_GPU in `parse_cli`); else $ATERM_GPU
    // forces GPU; else config `gpu = false`/`true` decides; else default to GPU on
    // macOS, CPU elsewhere.
    let force_cpu = std::env::var_os("ATERM_CPU").is_some();
    let want_gpu = !force_cpu
        && match (std::env::var_os("ATERM_GPU").is_some(), config.gpu) {
            (true, _) => true,
            (false, Some(explicit)) => explicit,
            (false, None) => cfg!(target_os = "macos"),
        };
    // Renderer theme (window clear colour, cursor, selection) from config; the
    // engine themes the CELLS, this themes the chrome around them.
    let theme = config.theme();
    // Opt into GPU (wgpu/Metal) rasterization with ATERM_GPU=1; falls back to
    // the CPU renderer if no GPU/font is available. Built FIRST so we can skip
    // the standalone CPU renderer entirely when the GPU path is live (the GPU
    // renderer already carries its own CPU face — a second is wasted RSS).
    // The injected rasterizer (ATERM_DESIGN WS-F): the GPU renderer when
    // ATERM_GPU is set and a device initializes, else the CPU renderer. Exactly
    // ONE is built — the GPU path already carries its own CPU face, so a
    // standalone CPU `Renderer` alongside it would parse the font and build a
    // glyph cache twice for nothing (~several MB idle RSS).
    // Optional configured font family (config `font_family`); resolved to a file
    // first, then $ATERM_FONT, then the built-in candidates. `None` = unchanged.
    let font_family: Option<String> = config.font_family.clone();
    let build_cpu = || -> Renderer {
        Renderer::from_system_with_family(font_family.as_deref(), font_px, theme).unwrap_or_else(
            || {
                eprintln!("aterm-gui: no system monospace font found (set $ATERM_FONT)");
                std::process::exit(1);
            },
        )
    };
    // `use_gpu` records which path is LIVE (GPU init can fail and fall back to
    // CPU), so live font-zoom rebuilds the backend as the same kind.
    let mut use_gpu = false;
    let mut backend: Backend = if want_gpu {
        match aterm_gpu::GpuRenderer::new_with_family(font_family.as_deref(), font_px, theme) {
            Ok(g) => {
                let (name, backend) = g.adapter();
                eprintln!("aterm-gui: GPU rendering on {name} ({backend})");
                use_gpu = true;
                Backend::Gpu(g)
            }
            Err(e) => {
                eprintln!("aterm-gui: GPU unavailable ({e}); using CPU renderer");
                Backend::Cpu(build_cpu())
            }
        }
    } else {
        Backend::Cpu(build_cpu())
    };
    // Record the live backend for the `metrics` control verb (font-zoom/HiDPI
    // rebuilds preserve the kind, so this one-time set stays accurate).
    metrics::set_backend_gpu(use_gpu);
    // Explicit render-scale override ($ATERM_FORCE_SCALE / --scale). When set it
    // wins over the headless 1.0 default (and, in `resumed`, over the real window's
    // scale_factor). Precedence: --scale flag > ATERM_FORCE_SCALE env > (headless
    // 1.0 / windowed scale_factor).
    let force_scale = resolve_force_scale();
    // Interior padding so text doesn't sit flush against the window edge. Seeded
    // at scale 1.0 (so the initial window — created before its display's scale is
    // known — fits the grid + border); `resumed()` re-applies it at the window's
    // real scale (`round(22·scale)`). Headless (no window) keeps this 1× value
    // UNLESS a scale override is given, so the `image` introspection renders at the
    // requested DPI (e.g. --scale 2 → ~2× the 1× framebuffer).
    let headless_scale = force_scale.unwrap_or(1.0);
    // With a scale override and a DEFAULT (non-explicit) font, auto-scale the glyph
    // size the same way `resumed` does for a real HiDPI window — `round(FONT_PX·scale)` —
    // so the headless capture's cell metrics (and thus framebuffer size) match a
    // real window of that scale. An explicit $ATERM_FONT_PX is honoured verbatim
    // (never double-scaled), preserving the env > config > default precedence.
    if headless {
        if force_scale.is_some() && !font_px_explicit && headless_scale > 1.0 {
            let scaled = (FONT_PX * headless_scale as f32)
                .round()
                .clamp(FONT_PX_MIN, FONT_PX_MAX);
            match build_backend(scaled, use_gpu, theme, font_family.as_deref()) {
                Some(b) => {
                    font_px = scaled;
                    backend = b;
                }
                None => eprintln!("aterm-gui: --scale font rebuild failed; keeping {font_px}px"),
            }
        }
        backend.set_pad(pad_for_scale(headless_scale));
    } else {
        // Windowed: seed at 1× now; `resumed()` re-applies the pad (and any font
        // auto-scale) at the chosen scale — the window's scale_factor, or the
        // override when --scale/$ATERM_FORCE_SCALE is set.
        backend.set_pad(pad_for_scale(1.0));
    }
    // Fixed per-glyph cell size (from the font); the `dims` verb multiplies it
    // by the grid to report the framebuffer pixel size the renderer produces.
    let (cell_w, cell_h) = backend.cell_size();
    let cell_size = (cell_w as u32, cell_h as u32);
    // Baseline terminal-identity environment for the spawned child, set
    // UNCONDITIONALLY (the deleted `spawn_env::build_spawn_plan` did this): so
    // programs detect an xterm-256color truecolor terminal named aterm, and a
    // Finder/.app launch — which inherits no locale — still gets a UTF-8 one.
    // The pty seam applies these in `build_child_env` by KEY-MATCH over the
    // pre-built env vector (overwrite an existing slot, else append), in vector
    // order, so the shell-integration vars appended below win on any key collision.
    let mut env_add: Vec<(String, String)> = vec![
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "aterm".to_string()),
        (
            "TERM_PROGRAM_VERSION".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ),
    ];
    // Guarantee the child runs under a UTF-8 LC_CTYPE so locale-respecting programs
    // (emacs/vim/python/tmux) treat terminal I/O as UTF-8 instead of falling back to
    // the ASCII codeset and rendering pasted multibyte text (e.g. box-drawing) as
    // `?`. aterm's parser is UTF-8-only, so a non-UTF-8 inherited locale — LANG=C,
    // bare en_US, LC_ALL=C, or a stray non-UTF-8 LC_CTYPE — is a mismatch the
    // terminal must correct. `resolve_spawn_locale` returns the MINIMAL override
    // (empty when the effective locale is already UTF-8, so an explicit user locale
    // is otherwise left untouched), honoring POSIX precedence LC_ALL > LC_CTYPE >
    // LANG. Proven by the aterm-pty `SpawnLocale` Tier-0 ty model + its conformance
    // tests; it SUPERSEDES the old all-unset LANG default (which missed every
    // present-but-non-UTF-8 case — the emacs `?` bug).
    let lc_all = std::env::var("LC_ALL").ok();
    let lc_ctype = std::env::var("LC_CTYPE").ok();
    let lang = std::env::var("LANG").ok();
    env_add.extend(aterm_pty::resolve_spawn_locale(
        lc_all.as_deref(),
        lc_ctype.as_deref(),
        lang.as_deref(),
    ));
    // Shell integration (OSC 133/633 command blocks for the AI `blocks` verb) is
    // injected when there is no interactive user to surprise — headless — or on
    // explicit $ATERM_SHELL_INTEGRATION; $ATERM_NO_SHELL_INTEGRATION always opts
    // out. The shell sources the user's own rc and adds the marks; its loader
    // vars are appended (per session, with a FRESH per-tab nonce) inside
    // `spawn_session`. No shell integration when `-e` runs a command directly:
    // there is no interactive shell to inject OSC 133/633 marks into.
    let integrate = exec_command.is_none()
        && std::env::var_os("ATERM_NO_SHELL_INTEGRATION").is_none()
        && (headless || std::env::var_os("ATERM_SHELL_INTEGRATION").is_some());
    // SEC-1 + OS-sandbox actuator: gate the single spawn seam on the containment
    // decision. The mode was resolved once at startup (`init_mode_from_env`, see
    // `main`); here we ask the actuator whether the initial shell may spawn for
    // that mode. For `Containment` on macOS the decision carries an SBPL profile
    // (`sbpl`) — the launcher MUST wrap the spawn in `sandbox-exec` to deny network
    // at the OS level; that profile is threaded into the `SessionFactory` so EVERY
    // tab (session 0 + Cmd-T) is wrapped identically. For every other mode `sbpl`
    // is `None` and the spawn is byte-identical to before (no sandbox-exec). The
    // decision also audits the (now honest) OS-sandbox posture. A `Deny` fails
    // closed (no shell). The PTY seam ALSO fails closed if a demanded wrapper is
    // missing (it refuses to spawn an unsandboxed shell) — defence in depth.
    let mode = aterm_containment::mode_or_containment();
    let sandbox_wrap: Option<String> = match aterm_containment::decide_spawn(mode) {
        aterm_containment::SpawnDecision::Permit {
            os_sandbox, sbpl, ..
        } => {
            if os_sandbox {
                eprintln!(
                    "aterm-gui: containment mode {mode}: OS sandbox ACTUATED \
                     (sandbox-exec '(deny network*)' + conservative secret-dir read/write deny \
                     ~/.ssh ~/.aws ~/.gnupg ~/.config/gh ~/.config/aterm ~/.netrc); \
                     general filesystem NOT scoped (follow-up)"
                );
            } else {
                eprintln!(
                    "aterm-gui: containment mode {mode}: OS sandbox NOT actuated \
                     (rlimits + capability gate only); see aterm-containment::actuator"
                );
            }
            // `sbpl` is the per-user owned profile string; take it as-is.
            sbpl
        }
        // Deny — or any future non-exhaustive variant — fails closed: no shell.
        other => {
            debug_assert!(matches!(
                other,
                aterm_containment::SpawnDecision::Deny { .. }
            ));
            eprintln!(
                "aterm-gui: containment mode {mode} denies spawning a shell (fail-closed); \
                 refusing to start an unconfined child"
            );
            std::process::exit(1);
        }
    };
    // The process minting authority is created ONCE here, in the trusted
    // launcher, before any untrusted input is processed. It is the SINGLE
    // `unsafe` root-authority mint in the product (CAP-1): trusted-launcher mint,
    // not a §5.4 sealed-by-reference mint (that is RED roadmap work). It grants
    // the spawn + sandbox capabilities the PTY seam requires.
    // SAFETY: this is the trusted process entry point, called exactly once here
    // before the shell is spawned and before any control-socket/PTY input runs.
    let authority = unsafe { aterm_cap::Authority::root_authority() };
    let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
    let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);

    // Block SIGUSR1 process-wide (in the main thread, BEFORE spawning any thread,
    // so all threads inherit the block) — a dedicated thread sigwait()s it and
    // requests a self-introspection snapshot. Default SIGUSR1 action would kill
    // the process, so blocking is required. This MUST precede `spawn_session`,
    // which spawns the (per-tab) reader + clipboard threads.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, ptr::null_mut());
    }

    let event_loop = EventLoop::<Wake>::with_user_event()
        .build()
        .expect("event loop");
    let proxy: EventLoopProxy<Wake> = event_loop.create_proxy();

    // Live config hot-reload: a lightweight thread `stat`s the config file every
    // ~500 ms and posts `Wake::ConfigReload` when its mtime changes, so an edit to
    // `~/.config/aterm/aterm.toml` re-applies font/theme/engine settings to every
    // live session without a restart (see `App::reload_config`). No-op when there
    // is no config PATH (no `$XDG_CONFIG_HOME` and no `$HOME`) — the no-config
    // startup path is unchanged. The file need not exist yet; creating it later
    // also fires a reload.
    config_watcher::spawn(config_path(), event_loop.create_proxy());

    // Silent background update check: off the event loop, on its own thread. It
    // talks to the private GitHub Release, verifies a notarized + Team-ID-pinned
    // newer build, and stages it for the NEXT launch (the staged build is applied
    // by aterm_update::apply_staged_if_ready at the top of main). A no-op for dev
    // builds, when the updater is disabled/unpinned, or when no update token is
    // provisioned. Skipped in headless mode so automated introspection never
    // reaches the network.
    if !headless {
        aterm_update::spawn_background_check(
            build_info::BUILD_NUMBER.parse::<u64>().unwrap_or(0),
            build_info::VERSION,
        );
    }

    // Latency self-introspection state (see App::trace_latency). The epoch is a
    // shared monotonic origin so each tab's reader thread and the UI thread
    // produce comparable nanosecond stamps.
    let trace_latency = std::env::var_os("ATERM_TRACE_LATENCY").is_some();
    let lat_epoch = Instant::now();
    let last_output_ns = Arc::new(AtomicU64::new(0));

    // Desktop-notification delivery (OSC 9/99/777). One delivery thread for the
    // whole app; the UI thread keeps `notify_suppress` current (the active-tab id of
    // every focused window) so the thread suppresses a notification only when its
    // session is the active tab of some focused window. Seeded with {0} to match the
    // initial App state (focused window, tab 0 / session 0 active). `notify_tx` is
    // cloned into each tab's engine callbacks via the factory.
    let notify_suppress = Arc::new(Mutex::new(std::collections::HashSet::from([0u64])));
    let notify_tx = notify::spawn_delivery(notify_suppress.clone());

    // The session factory captures everything a NEW tab's `spawn_session` needs
    // (the by-reference spawn/sandbox caps from the SINGLE root authority above,
    // the baseline env, the engine config, the shell-integration decision, the
    // cwd, and the latency state). `spawn_session` stands up the PTY + engine +
    // policy + OSC52 + reader thread per tab; session 0 is created the same way so
    // the single-session startup is byte-identical to the old inline code.
    let session_factory = SessionFactory {
        spawn_cap,
        sandbox_cap,
        env_add,
        exec_command,
        cwd,
        sandbox_wrap,
        // Always Some: pins the engine default fg/bg to the theme so unstyled cells
        // paint the themed background, not spec-black (see `applied_terminal_config`).
        terminal_config: Some(config.applied_terminal_config()),
        integrate,
        lat_epoch,
        last_output_ns: last_output_ns.clone(),
        notify_tx,
    };

    // Session 0: the first tab. A spawn failure here is fatal (no shell to show);
    // a Cmd-T failure later is logged and ignored (the existing tabs survive).
    let session0 = spawn_session(0, WindowId(0), rows, cols, &session_factory, &proxy)
        .unwrap_or_else(|e| {
            eprintln!("aterm-gui: spawn failed: {e}");
            std::process::exit(1);
        });
    let term = session0.term.clone();
    let master = session0.master;
    let app_sink = session0.ctx.sink.clone();

    // SIGUSR1 listener -> Wake::Snapshot (introspect the live screen to PNG+txt).
    {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe {
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGUSR1);
            }
            loop {
                let mut sig: libc::c_int = 0;
                if unsafe { libc::sigwait(&set, &mut sig) } != 0 {
                    break;
                }
                if proxy.send_event(Wake::Snapshot).is_err() {
                    break;
                }
            }
        });
    }

    // Live introspection control socket: a thread binds a Unix socket and serves
    // newline-delimited requests against this SAME running terminal (read the
    // screen, drive the shell, snapshot pixels, resize). Access control is
    // DEFAULT-ON (see `control_auth`): the socket lives in a per-user 0700 dir
    // ($XDG_RUNTIME_DIR/aterm, else ~/Library/Application Support/aterm), is
    // chmod 0600, accepts only same-uid peers, and requires the per-launch
    // capability token before any verb. Each instance binds its own
    // aterm-<pid>.sock (+ matching token) behind an atomically-updated
    // aterm.sock symlink, so concurrent instances never collide.
    // $ATERM_CONTROL_SOCK overrides the path, or disables the socket entirely
    // with `0`/`off` ($ATERM_NO_CONTROL_SOCK=1 works too).
    //
    // TABS: the control socket FOLLOWS THE ACTIVE TAB. It holds a shared
    // `ActiveHandle` (active session's `term` + `master`) that `sync_active_session`
    // updates on every tab switch / open / close; each request resolves the current
    // target, so text/drive/scroll verbs act on whatever tab is active and never
    // break when an earlier tab (incl. tab 0) closes. `image`/`dims` already render
    // the active tab via the shared renderer. The socket's auth (peer-uid + per-launch
    // token) is unchanged — only the target session follows the UI.
    let active_handle: control::ActiveHandle = Arc::new(Mutex::new(control::ActiveSession {
        term: term.clone(),
        master,
        id: 0,
        ctx: session0.ctx.clone(),
    }));
    // P1.1: the process-wide session registry. Register the startup session (tab 0)
    // — the root of the family tree (no parent) — before the control thread starts,
    // so a `@<selector>` is resolvable from the first request.
    let store = session_store::new_store();
    App::register_session(&store, &session0, None);
    // P1.3: the process-wide subscriber registry. A `subscribe` connection
    // registers here; the GUI's `Wake::Output` hook notifies it. Created BEFORE
    // the control thread so a subscribe is serviceable from the first request.
    let subscribers = subscribe::new_registry();
    let image_queue: control::ImageQueue = Arc::new(Mutex::new(VecDeque::new()));
    let sock_plan = match control_auth::resolve_socket_plan() {
        control_auth::SocketResolution::Enabled(plan) => {
            control::spawn(
                active_handle.clone(),
                store.clone(),
                subscribers.clone(),
                event_loop.create_proxy(),
                image_queue.clone(),
                plan.clone(),
                cell_size,
                // Publish the root graph entry from inside spawn, AFTER bind, so it
                // never races the stale sweep (vs. writing it here pre-bind).
                Some((session0.ctx.self_id.clone(), session0.ctx.nonce)),
            );
            Some(plan)
        }
        control_auth::SocketResolution::Disabled => {
            eprintln!("aterm-gui: control socket disabled by environment");
            None
        }
        control_auth::SocketResolution::NoDir => {
            eprintln!(
                "aterm-gui: no per-user runtime dir (set XDG_RUNTIME_DIR, HOME, or \
                 ATERM_CONTROL_SOCK); control socket disabled"
            );
            None
        }
    };

    // Recursion discovery (Item 5b): the root session's graph entry is published
    // by `control::spawn` AFTER it binds (so it never races the stale sweep — see
    // the `root_identity` arg above). We retain `root_sid` here only for the
    // graceful-exit cleanup below (session0 is moved into the pool shortly).
    let root_sid = session0.ctx.self_id.clone();

    // The ONE logical window that always exists, from construction. Its OS window
    // + present target attach later in `resumed` (never in headless). The active
    // session at startup is tab 0 (`id: 0`; see the `ActiveSession` above).
    let ws0 = WindowState::new(
        term.clone(),
        master,
        app_sink.clone(),
        0,
        rows,
        cols,
        TabIndex::new(0, 1),
        // One pane tree for the startup tab: a single leaf on session 0 (the exact
        // one-session-per-tab layout). Cmd-D / Cmd-Shift-D split the focused pane.
        vec![pane::PaneTree::new(0)],
    );
    // The pool OWNS session 0 with one view (this window's single tab).
    let mut pool = SessionPool::default();
    pool.insert(session0);

    let mut app = App {
        pool,
        next_session_id: 1,
        hold,
        session_factory,
        proxy: Some(proxy.clone()),
        active_handle,
        store,
        subscribers,
        backend,
        introspect_gpu: aterm_gpu::WindowGpu::new(),
        font_px,
        default_font_px: font_px,
        font_px_explicit,
        use_gpu,
        theme,
        // GLOBAL config (window-uniform): font family, Option-as-Meta, keybindings.
        font_family,
        option_as_meta: config.option_as_meta_or_default(),
        keybindings: keybinding::Keybindings::from_config(config.keybindings.as_ref()),
        windows: {
            let mut m = BTreeMap::new();
            m.insert(WindowId(0), ws0);
            m
        },
        frontmost_window: Some(WindowId(0)),
        focus_order: Vec::new(),
        next_window_id: 1,
        winit_to_window: HashMap::new(),
        headless,
        bell_beep: BellRateLimiter::new(BELL_BEEP_INTERVAL),
        image_queue,
        trace_latency,
        lat_epoch,
        last_output_ns,
        notify_suppress,
        search_history_lines: config
            .search_history_lines
            .map_or(MAX_SEARCH_HISTORY, |n| n.min(i32::MAX as u32) as i32),
        // Installed in `resumed` once the window exists (non-headless macOS only).
        _menu: None,
        // Native window toolbars, keyed per window; installed in `attach_os_window`.
        _toolbars: BTreeMap::new(),
        // GLOBAL tab-strip config (per-frame `tab_segments` live in WindowState).
        tab_strip_rows,
    };
    event_loop.run_app(&mut app).expect("run");
    // Graceful-exit cleanup: this instance's socket + token, and the `latest`
    // symlink only while it still points at us (a newer instance may own it).
    // Crash exits are covered by the stale sweep at the next spawn.
    if let Some(plan) = &sock_plan {
        control_auth::cleanup_socket(plan);
        // Un-publish our recursion discovery entry so a parent does not dial a
        // dead socket (crash exits are swept at the next spawn — see S1 sweep).
        proxy::remove_graph_entry(&control_auth::dir_of_socket(&plan.sock_path), &root_sid);
    }
    // QUIT-HANG FIX (final-exit path). Do NOT fall off `main` and let `app` drop
    // normally: that would run every `Session::drop` on THIS (main) thread, and
    // `close(master)` racing a reader still parked in `read(master)` is exactly
    // the macOS hang the stackshot caught (`close` wedged ~49 s in `lck_mtx_sleep`
    // on the tty lock). After the socket cleanup above (the only teardown that has
    // user-visible filesystem effects), exit the process immediately: the OS
    // reclaims every fd and SIGHUPs the children via controlling-tty teardown, so
    // no blocking Drop chain runs at exit. The mid-run close path (Cmd-W / pane
    // close) stays non-blocking via `Session::drop`'s hang-up-then-off-thread-close.
    // `forget` `app` for good measure so even a future early `return` can't trip
    // the blocking Drop; `exit(0)` already skips it.
    std::mem::forget(app);
    std::process::exit(0);
}

/// Build a minimal in-memory stub [`Session`] (id `id`): a real `Terminal` +
/// `SessionCtx`, but a sentinel `master = -1` so `Session::drop` is a no-op (it
/// only `close()`s a real fd). Lets the multi-window test stand up windows + tabs
/// with NO PTY, exercising the pool/window/frontmost bookkeeping in isolation.
/// (Mirrors `session_pool_tests::test_session`; shared here for `headless_for_test`.)
#[cfg(test)]
fn stub_session(id: u64) -> Session {
    let ctx = Arc::new(SessionCtx {
        sink: Arc::new(SinkWriter::new(-1)),
        edges: std::sync::Mutex::new(EdgeTable::new()),
        self_id: SessionId::generate(),
        nonce: LaunchNonce::generate(),
        cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
            80, 24,
        ))),
        temporal: Arc::new(std::sync::Mutex::new(
            crate::temporal::TemporalRecorder::new(),
        )),
        byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
    });
    Session {
        id,
        term: Arc::new(Mutex::new(Terminal::new(24, 80))),
        master: -1,
        pid: -1,
        ctx,
        child_proxy_sid: None,
    }
}

#[cfg(test)]
mod multi_window_tests {
    use super::{App, CloseOutcome, WindowId, stub_session};

    /// The oracle for the multi-window flip: from one headless window, create a
    /// 2nd (real `create`/`install` bookkeeping, stub session), confirm it lands as
    /// a strictly-greater frontmost window with its own pooled session and intact
    /// structural invariants; then close the NON-frontmost (survives, `Stay`,
    /// frontmost re-points to a live window) and finally the LAST (`Exit`, empty).
    #[test]
    fn multi_window_lifecycle() {
        let mut app = App::headless_for_test();
        // Start: exactly one window (WindowId(0)), one pooled session (id 0).
        assert_eq!(app.windows.len(), 1);
        assert_eq!(app.frontmost_window, Some(WindowId(0)));
        assert_eq!(app.pool.iter().count(), 1);
        assert!(app.structural_invariants_ok());

        // --- Create a 2nd window. The stub session's id MUST match the App's next
        // session id (insert_logical_window asserts this), mirroring how
        // create_window_logical pairs the minted session id with the spawned tab.
        let sid1 = app.next_session_id; // 1
        let wid1 = app.insert_logical_window(stub_session(sid1), 24, 80);
        assert_eq!(app.windows.len(), 2, "a 2nd window now exists");
        assert_eq!(
            app.frontmost_window,
            Some(wid1),
            "the new window is frontmost"
        );
        assert!(wid1 > WindowId(0), "minted a strictly-increasing WindowId");
        assert_eq!(app.pool.iter().count(), 2, "the pool owns both sessions");
        assert!(app.structural_invariants_ok());

        // A 3rd window mints a STRICTLY GREATER id again (never reused), even though
        // we will close one below — ids are monotonic, not dense.
        let sid2 = app.next_session_id; // 2
        let wid2 = app.insert_logical_window(stub_session(sid2), 24, 80);
        assert!(
            wid2 > wid1,
            "each create mints a strictly-increasing WindowId"
        );
        assert_eq!(app.windows.len(), 3);
        assert_eq!(app.frontmost_window, Some(wid2));
        assert_eq!(app.pool.iter().count(), 3);

        // --- Close the NON-frontmost window (WindowId(0)). It must survive the rest,
        // the pool drops only its session, and frontmost is untouched (still wid2,
        // which was not the one closed) — but it must always name a LIVE window.
        let outcome = app.close_window_logical(WindowId(0));
        assert_eq!(
            outcome,
            CloseOutcome::Stay,
            "closing a non-last window keeps the app"
        );
        assert_eq!(app.windows.len(), 2, "the closed window is gone");
        assert!(!app.windows.contains_key(&WindowId(0)));
        assert_eq!(
            app.pool.iter().count(),
            2,
            "only the closed window's session dropped"
        );
        assert_eq!(
            app.frontmost_window,
            Some(wid2),
            "frontmost untouched (closed a sibling)"
        );
        assert!(app.structural_invariants_ok());

        // --- Close the CURRENT frontmost (wid2): frontmost must RE-POINT to the one
        // remaining live window (wid1), still Stay, invariants hold.
        let outcome = app.close_window_logical(wid2);
        assert_eq!(outcome, CloseOutcome::Stay, "a sibling remains → app stays");
        assert_eq!(app.windows.len(), 1);
        assert_eq!(
            app.frontmost_window,
            Some(wid1),
            "frontmost re-points to the surviving window",
        );
        assert!(
            app.frontmost_window
                .is_some_and(|f| app.windows.contains_key(&f))
        );
        assert!(app.structural_invariants_ok());

        // --- Close the LAST window: now the app must exit and no windows remain.
        let outcome = app.close_window_logical(wid1);
        assert_eq!(
            outcome,
            CloseOutcome::Exit,
            "closing the last window exits the app"
        );
        assert!(app.windows.is_empty(), "no windows remain");
        assert_eq!(app.frontmost_window, None, "frontmost is None once empty");
        assert_eq!(app.pool.iter().count(), 0, "every session detached");

        // Closing an already-gone / unknown WindowId is a fail-closed no-op (Stay),
        // never a panic — the stale-event discipline the routing relies on.
        assert_eq!(app.close_window_logical(WindowId(999)), CloseOutcome::Stay);
    }

    /// "Move Tab to New Window" (Step 10a): detaching the front window's active tab
    /// MOVES the view out into a brand-new window — the source loses the tab (active
    /// clamped), a fresh frontmost window holds EXACTLY that tab, and the pool's
    /// session count is UNCHANGED (the view moved, not duplicated or dropped). A
    /// single-tab source is a no-op.
    #[test]
    fn detach_active_tab_moves_view_to_new_window() {
        let mut app = App::headless_for_test();
        // Stage a 2-tab front window: window 0 starts with tab (session 0); push a
        // 2nd tab (session 1) and switch to it (now the active tab).
        let sid1 = app.next_session_id; // 1
        app.push_stub_tab(WindowId(0), stub_session(sid1));
        // Front window now has 2 tabs [0, 1], active = index 1 (session 1).
        assert_eq!(app.windows.len(), 1);
        let pool_before = app.pool.iter().count();
        assert_eq!(pool_before, 2, "two pooled sessions before the detach");
        {
            let Some(ws0) = app.windows.get(&WindowId(0)) else {
                panic!("window 0 exists")
            };
            assert_eq!(
                ws0.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                vec![0, sid1]
            );
            assert_eq!(ws0.tabs.active, 1);
            assert_eq!(ws0.active_id, sid1, "the appended tab is active");
        }
        assert!(app.structural_invariants_ok());

        // --- Detach the active tab (session 1) of the front window. The LOGICAL
        // half is headless-testable (no OS window attach); it returns the new id.
        let wid_b = app
            .detach_active_tab_logical()
            .expect("a 2-tab source detaches its active tab");

        // The new window is frontmost and is strictly-greater than the source.
        assert!(wid_b > WindowId(0), "minted a strictly-increasing WindowId");
        assert_eq!(
            app.frontmost_window,
            Some(wid_b),
            "the new window is frontmost"
        );
        assert_eq!(app.windows.len(), 2, "windows grew by one");

        // The source window LOST the moved tab: len-1, active clamped into range.
        {
            let Some(ws0) = app.windows.get(&WindowId(0)) else {
                panic!("source survives")
            };
            assert_eq!(
                ws0.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                vec![0],
                "source lost the moved tab"
            );
            assert_eq!(ws0.tabs.count, 1);
            assert_eq!(ws0.tabs.active, 0, "source active clamped into range");
            assert_eq!(
                ws0.active_id, 0,
                "source re-mirrored onto its surviving tab"
            );
        }

        // The new window holds EXACTLY the moved tab, and displays it.
        {
            let Some(ws_b) = app.windows.get(&wid_b) else {
                panic!("the new window exists")
            };
            assert_eq!(
                ws_b.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                vec![sid1],
                "new window holds exactly the moved tab"
            );
            assert_eq!(ws_b.tabs.count, 1);
            assert_eq!(ws_b.tabs.active, 0);
            assert_eq!(ws_b.active_id, sid1, "new window displays the moved tab");
        }

        // The view MOVED: the pool session count is UNCHANGED (not duplicated, not
        // dropped) — the moved session keeps its single view.
        assert_eq!(
            app.pool.iter().count(),
            pool_before,
            "detach moves the view; the pool's session count is unchanged",
        );
        assert!(
            app.pool.get(sid1).is_some(),
            "the moved session is still pooled"
        );
        assert!(app.structural_invariants_ok());

        // --- Detaching when the (now front) window has a SINGLE tab is a no-op:
        // window B holds only `sid1`, so detach is refused and nothing changes.
        let windows_before = app.windows.len();
        let front_before = app.frontmost_window;
        assert_eq!(
            app.detach_active_tab_logical(),
            None,
            "a single-tab source refuses the detach",
        );
        assert_eq!(
            app.windows.len(),
            windows_before,
            "no window minted on a no-op"
        );
        assert_eq!(
            app.frontmost_window, front_before,
            "frontmost untouched on a no-op"
        );
        assert_eq!(
            app.pool.iter().count(),
            pool_before,
            "pool untouched on a no-op"
        );
        assert!(app.structural_invariants_ok());
    }

    /// "Move Tab to Next Window" (Step 10c): move the frontmost window's active tab
    /// into the NEXT EXISTING window (wrapping). Two cases are exercised:
    ///   1. SOURCE SURVIVES — the front window has TWO tabs; the active one MOVES to
    ///      the next window (which gains it as ITS active tab), the source keeps its
    ///      other tab, `windows.len()` is UNCHANGED, the pool's session count is
    ///      UNCHANGED (a pure view-move — no spawn, drop, or duplicate), and the
    ///      destination becomes frontmost.
    ///   2. SOURCE EMPTIED → CLOSED — a single-tab front window's only tab moves to
    ///      the next window; the now-empty source window is CLOSED (`windows.len()`
    ///      drops by one), the moved view is NOT double-detached (pool count still
    ///      unchanged), and the destination becomes frontmost.
    ///
    /// A <2-window app is a no-op (nowhere to move the tab).
    #[test]
    fn migrate_active_tab_moves_view_to_next_window() {
        // --- No-op with ONE window: nothing to move it to. ------------------------
        {
            let mut app = App::headless_for_test();
            assert_eq!(app.windows.len(), 1);
            let pool_before = app.pool.iter().count();
            let front_before = app.frontmost_window;
            app.migrate_active_tab_to_next_window();
            assert_eq!(app.windows.len(), 1, "single window: migrate is a no-op");
            assert_eq!(
                app.frontmost_window, front_before,
                "frontmost untouched on a no-op"
            );
            assert_eq!(
                app.pool.iter().count(),
                pool_before,
                "pool untouched on a no-op"
            );
            assert!(app.structural_invariants_ok());
        }

        // --- Case 1: SOURCE SURVIVES (front window has two tabs). -----------------
        {
            let mut app = App::headless_for_test();
            // A 2nd existing window (WindowId(1), session 1). `insert_logical_window`
            // makes it frontmost; we re-focus WindowId(0) below.
            let sid1 = app.next_session_id; // 1
            let wid_b = app.insert_logical_window(stub_session(sid1), 24, 80);
            assert_eq!(wid_b, WindowId(1));
            // Stage WindowId(0) with TWO tabs [0, sid2], its active = the appended one.
            let sid2 = app.next_session_id; // 2
            app.push_stub_tab(WindowId(0), stub_session(sid2));
            // Re-focus the (lower-id) front window so it is the migration SOURCE; the
            // NEXT window after it in id order is WindowId(1).
            app.frontmost_window = Some(WindowId(0));
            app.sync_active_session();
            assert_eq!(app.windows.len(), 2);
            let pool_before = app.pool.iter().count();
            assert_eq!(pool_before, 3, "three pooled sessions before the migrate");
            {
                let Some(ws0) = app.windows.get(&WindowId(0)) else {
                    panic!("window 0 exists")
                };
                assert_eq!(
                    ws0.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                    vec![0, sid2]
                );
                assert_eq!(ws0.active_id, sid2, "the appended tab is active");
            }
            assert!(app.structural_invariants_ok());

            // Move WindowId(0)'s active tab (sid2) into the NEXT window (WindowId(1)).
            app.migrate_active_tab_to_next_window();

            // The move did NOT change the window count; the destination is frontmost.
            assert_eq!(
                app.windows.len(),
                2,
                "a surviving source keeps both windows"
            );
            assert_eq!(
                app.frontmost_window,
                Some(wid_b),
                "focus follows the moved tab to B"
            );

            // The source LOST the moved tab; active clamped onto its surviving tab.
            {
                let Some(ws0) = app.windows.get(&WindowId(0)) else {
                    panic!("source survives")
                };
                assert_eq!(
                    ws0.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                    vec![0],
                    "source lost the moved tab"
                );
                assert_eq!(ws0.tabs.count, 1);
                assert_eq!(ws0.tabs.active, 0, "source active clamped into range");
                assert_eq!(
                    ws0.active_id, 0,
                    "source re-mirrored onto its surviving tab"
                );
            }
            // The destination GAINED the moved tab as its active (last) tab.
            {
                let Some(ws_b) = app.windows.get(&wid_b) else {
                    panic!("destination exists")
                };
                assert_eq!(
                    ws_b.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                    vec![sid1, sid2],
                    "moved tab appended to B"
                );
                assert_eq!(ws_b.tabs.count, 2);
                assert_eq!(ws_b.tabs.active, 1, "B's active points at the moved tab");
                assert_eq!(ws_b.active_id, sid2, "B displays the moved tab");
            }
            // PURE view-move: the pool's session count is UNCHANGED (not dropped or
            // duplicated); the moved session is still pooled.
            assert_eq!(
                app.pool.iter().count(),
                pool_before,
                "migrate moves the view; the pool's session count is unchanged",
            );
            assert!(
                app.pool.get(sid2).is_some(),
                "the moved session is still pooled"
            );
            assert!(app.structural_invariants_ok());
        }

        // --- Case 2: SOURCE EMPTIED → CLOSED (front window has one tab). -----------
        {
            let mut app = App::headless_for_test();
            // A 2nd existing window; WindowId(0) keeps its single tab (session 0).
            let sid1 = app.next_session_id; // 1
            let wid_b = app.insert_logical_window(stub_session(sid1), 24, 80);
            assert_eq!(wid_b, WindowId(1));
            // Re-focus the single-tab front window as the migration SOURCE.
            app.frontmost_window = Some(WindowId(0));
            app.sync_active_session();
            assert_eq!(app.windows.len(), 2);
            let pool_before = app.pool.iter().count();
            assert_eq!(pool_before, 2, "two pooled sessions before the migrate");

            // Move WindowId(0)'s only tab (session 0) into the NEXT window. Window 0
            // becomes empty and is CLOSED.
            app.migrate_active_tab_to_next_window();

            // The emptied source window was closed: only the destination remains.
            assert_eq!(app.windows.len(), 1, "the emptied source window is closed");
            assert!(
                !app.windows.contains_key(&WindowId(0)),
                "source window removed"
            );
            assert_eq!(
                app.frontmost_window,
                Some(wid_b),
                "the destination is frontmost"
            );
            // The destination holds both its own tab and the moved one (moved last/active).
            {
                let Some(ws_b) = app.windows.get(&wid_b) else {
                    panic!("destination exists")
                };
                assert_eq!(
                    ws_b.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                    vec![sid1, 0],
                    "moved tab appended to B"
                );
                assert_eq!(ws_b.tabs.count, 2);
                assert_eq!(ws_b.tabs.active, 1, "B's active points at the moved tab");
                assert_eq!(ws_b.active_id, 0, "B displays the moved tab");
            }
            // PURE view-move with NO double-detach when the source is closed: the pool's
            // session count is UNCHANGED and the moved session is still pooled.
            assert_eq!(
                app.pool.iter().count(),
                pool_before,
                "closing the emptied source does NOT detach the already-moved view",
            );
            assert!(
                app.pool.get(0).is_some(),
                "the moved session is still pooled"
            );
            assert!(app.structural_invariants_ok());
        }
    }

    /// "Open Active Session in New Window" (Step 10b): showing the front window's
    /// active session in a SECOND window ADDS a view — both windows display the SAME
    /// pooled session (the pool keeps EXACTLY ONE session, its view-count 1→2), so the
    /// live grid is visible in two windows at once. Output fan-out (`windows_displaying`)
    /// then yields BOTH windows. Closing one viewer leaves the session alive (views
    /// 2→1, the PTY survives); closing the last viewer drops it (views→0, PTY closes)
    /// and, being the last window, exits the app. This is the attach/detach balance:
    /// one `attach` on open, one `detach` per window-close of a viewing tab.
    #[test]
    fn open_active_session_in_new_window_shares_one_session_across_two_windows() {
        let mut app = App::headless_for_test();
        // Start: exactly one window (WindowId(0)) showing session 0, one view.
        assert_eq!(app.windows.len(), 1);
        assert_eq!(app.frontmost_window, Some(WindowId(0)));
        assert_eq!(app.pool.iter().count(), 1, "one pooled session at start");
        assert_eq!(
            app.pool.views(0),
            Some(1),
            "session 0 has a single view at start"
        );
        {
            let Some(ws0) = app.windows.get(&WindowId(0)) else {
                panic!("window 0 exists")
            };
            assert_eq!(ws0.active_id, 0, "window 0 displays session 0");
        }
        assert!(app.structural_invariants_ok());

        // --- Open the active session in a NEW window (the logical half is
        // headless-testable: no OS attach). It returns the new window's id.
        let new_wid = app
            .open_active_session_in_new_window_logical()
            .expect("the front window has an active session to open in a new window");

        // A NEW window exists, strictly-greater, and is now frontmost.
        assert!(
            new_wid > WindowId(0),
            "minted a strictly-increasing WindowId"
        );
        assert_eq!(
            app.frontmost_window,
            Some(new_wid),
            "the new window is frontmost"
        );
        assert_eq!(app.windows.len(), 2, "windows grew by one");

        // BOTH windows display the SAME session 0 (the same live grid in two windows).
        {
            let Some(ws0) = app.windows.get(&WindowId(0)) else {
                panic!("source survives")
            };
            assert_eq!(
                ws0.active_id, 0,
                "the original window still shows session 0"
            );
        }
        {
            let Some(ws_b) = app.windows.get(&new_wid) else {
                panic!("the new window exists")
            };
            assert_eq!(
                ws_b.layouts.iter().map(|t| t.focus()).collect::<Vec<_>>(),
                vec![0],
                "the new window views exactly session 0"
            );
            assert_eq!(ws_b.active_id, 0, "the new window also shows session 0");
        }

        // The pool still has EXACTLY ONE session (id 0) — NOT duplicated — but its
        // view-count went 1→2 (a second window now displays it).
        assert_eq!(
            app.pool.iter().count(),
            1,
            "a SHARED view, not a second session"
        );
        assert!(app.pool.get(0).is_some(), "session 0 is still pooled");
        assert_eq!(app.pool.views(0), Some(2), "two windows now view session 0");

        // The output fan-out now yields BOTH viewing windows: a shared session's
        // output repaints every window that can see it.
        {
            let mut displaying: Vec<WindowId> = app.windows_displaying(0).collect();
            displaying.sort();
            let mut expected = vec![WindowId(0), new_wid];
            expected.sort();
            assert_eq!(displaying, expected, "both windows display session 0");
        }
        assert!(app.structural_invariants_ok());

        // --- Close ONE viewer (the new window). Its single tab detaches one view, so
        // the session SURVIVES (views 2→1) and the original window still shows it.
        assert_eq!(
            app.close_window_logical(new_wid),
            CloseOutcome::Stay,
            "closing one of two windows keeps the app alive",
        );
        assert_eq!(app.windows.len(), 1, "back to one window");
        assert!(
            app.pool.get(0).is_some(),
            "the shared session survives one viewer leaving"
        );
        assert_eq!(
            app.pool.views(0),
            Some(1),
            "views 2→1 — the PTY is NOT closed yet"
        );
        {
            let Some(ws0) = app.windows.get(&WindowId(0)) else {
                panic!("original survives")
            };
            assert_eq!(
                ws0.active_id, 0,
                "the original window still shows session 0"
            );
        }
        // Now only the original window displays session 0.
        assert_eq!(
            app.windows_displaying(0).collect::<Vec<_>>(),
            vec![WindowId(0)],
            "only the surviving viewer displays session 0",
        );
        assert!(app.structural_invariants_ok());

        // --- Close the LAST viewer (the original window). The last view leaves
        // (views→0), the session is dropped (its PTY would close), and — being the
        // last window — the app exits.
        assert_eq!(
            app.close_window_logical(WindowId(0)),
            CloseOutcome::Exit,
            "closing the last window exits the app",
        );
        assert!(app.windows.is_empty(), "no windows remain");
        assert_eq!(app.frontmost_window, None, "frontmost is None once empty");
        assert!(
            app.pool.get(0).is_none(),
            "the last viewer leaving drops the session"
        );
        assert_eq!(app.pool.views(0), None, "the session is gone from the pool");
        assert_eq!(app.pool.iter().count(), 0, "the pool is empty");
    }

    /// REGRESSION (audit): a CO-VIEWED (Cmd-Shift-O) session has ONE reader thread,
    /// so its shell exit emits exactly ONE `Wake::Exit`. The handler must close it
    /// in EVERY window that views it — the old `.find()`-the-first-owner logic left
    /// the OTHER window pinned to a dead, still-pooled `Exited` pane forever.
    /// `exit_session_logical` (the el-free core the arm wraps) must release ALL views.
    #[test]
    fn shared_session_exit_closes_every_viewer_not_just_the_first() {
        let mut app = App::headless_for_test();
        // Share session 0 into a 2nd window (Cmd-Shift-O): views 1→2, both display it.
        let new_wid = app
            .open_active_session_in_new_window_logical()
            .expect("the front window has an active session to open in a new window");
        assert_eq!(
            app.pool.views(0),
            Some(2),
            "the share gives session 0 two views"
        );
        {
            let mut viewers: Vec<WindowId> = app.windows_displaying(0).collect();
            viewers.sort();
            let mut expected = vec![WindowId(0), new_wid];
            expected.sort();
            assert_eq!(
                viewers, expected,
                "both windows (original + the share) display the session"
            );
        }

        // The shell exits ONCE. The logical core marks it Exited and closes it in
        // BOTH viewers; each window's single tab was the shared session, so BOTH
        // report a last-tab close (the old code returned only the lowest-id owner).
        let to_close = app.exit_session_logical(0);
        assert_eq!(
            to_close.len(),
            2,
            "BOTH viewers' last tab closed on the shared exit — not just the first owner",
        );
        // Escalate each (the el-free twin of the arm's `close_window`/`el.exit()`).
        for wid in &to_close {
            app.close_window_logical(*wid);
        }

        // No viewer is left pinned to the dead session: every view released (refcount
        // drained to 0, so the pool dropped + the registry deregistered it), and no
        // window still shows it.
        assert_eq!(
            app.pool.views(0),
            None,
            "every view of the exited session was released"
        );
        assert!(
            app.pool.get(0).is_none(),
            "the fully-exited session is dropped from the pool"
        );
        assert_eq!(
            app.windows_displaying(0).count(),
            0,
            "no window still displays the exited session"
        );
        assert!(
            app.windows.is_empty(),
            "both single-session windows closed on the shared exit"
        );
    }

    /// Closing the FRONTMOST window re-points to the most-recently-FOCUSED survivor
    /// (the window the OS raises), not blindly the lowest WindowId — but with NO
    /// focus history (headless) it falls back to the lowest live id, the
    /// deterministic choice automation relies on. Both halves in one test.
    #[test]
    fn close_front_repoints_to_mru_survivor_else_lowest_id() {
        // --- No focus history (headless): deterministic lowest-id fallback. ---
        let mut app = App::headless_for_test();
        let s1 = app.next_session_id;
        let _w1 = app.insert_logical_window(stub_session(s1), 24, 80);
        let s2 = app.next_session_id;
        let w2 = app.insert_logical_window(stub_session(s2), 24, 80);
        assert_eq!(
            app.frontmost_window,
            Some(w2),
            "the newest window is frontmost"
        );
        assert!(
            app.focus_order.is_empty(),
            "no OS focus events fired in headless"
        );
        // Close the front (w2). With no focus history, the survivor is the LOWEST id.
        assert_eq!(app.close_window_logical(w2), CloseOutcome::Stay);
        assert_eq!(
            app.frontmost_window,
            Some(WindowId(0)),
            "no focus history → lowest live id survivor (deterministic)",
        );

        // --- With focus history: re-point follows the MRU survivor. ---
        let mut app = App::headless_for_test();
        let s1 = app.next_session_id;
        let w1 = app.insert_logical_window(stub_session(s1), 24, 80);
        let s2 = app.next_session_id;
        let w2 = app.insert_logical_window(stub_session(s2), 24, 80);
        // The OS focuses windows 0, then 1, then 2 (2 ends up front). MRU tail = w2.
        app.note_window_focused(WindowId(0));
        app.note_window_focused(w1);
        app.note_window_focused(w2);
        assert_eq!(app.frontmost_window, Some(w2));
        // Close the front (w2). The most-recently-focused SURVIVOR is w1 (focused
        // after window 0), so frontmost re-points to w1 — NOT the lowest id (0).
        assert_eq!(app.close_window_logical(w2), CloseOutcome::Stay);
        assert_eq!(
            app.frontmost_window,
            Some(w1),
            "MRU survivor (w1) is chosen over the lowest id (window 0)",
        );
        assert!(
            !app.focus_order.contains(&w2),
            "the closed window left the focus stack"
        );
        assert!(app.structural_invariants_ok());
    }

    /// Closing a window with a SPLIT (multi-pane) tab must detach EVERY pane's view,
    /// not just the focused one — the pool drops by exactly the pane count. (Audit
    /// gap: no test covered split-tab teardown.)
    #[test]
    fn close_window_with_split_tab_detaches_every_pane() {
        let mut app = App::headless_for_test(); // window 0, session 0
        // A 2nd window so closing window 0 is a Stay (not the last-window exit).
        let s1 = app.next_session_id;
        let w1 = app.insert_logical_window(stub_session(s1), 24, 80);
        // Split window 0's single tab into TWO panes (session 0 + a fresh session).
        let split_sid = app.split_active_stub_tab(WindowId(0));
        assert_eq!(
            app.pool.iter().count(),
            3,
            "w0's two split panes + w1's session"
        );
        assert!(app.pool.get(0).is_some() && app.pool.get(split_sid).is_some());

        // Close window 0: BOTH of its split panes' sessions detach (pool drops by 2).
        assert_eq!(app.close_window_logical(WindowId(0)), CloseOutcome::Stay);
        assert!(
            app.pool.get(0).is_none(),
            "split pane A released on window close"
        );
        assert!(
            app.pool.get(split_sid).is_none(),
            "split pane B released on window close"
        );
        assert_eq!(
            app.pool.iter().count(),
            1,
            "pool dropped by EXACTLY the 2 panes"
        );
        assert_eq!(app.frontmost_window, Some(w1));
        assert!(app.structural_invariants_ok());
    }

    /// Migrating a tab into a DIFFERENT-sized window reflows the moved session to the
    /// destination's grid geometry (the SIGWINCH the moved app sees). (Audit gap:
    /// the cross-size resize-on-move branch was untested.)
    #[test]
    fn migrate_to_different_size_window_reflows_moved_session() {
        let mut app = App::headless_for_test(); // window 0 = 24x80, session 0
        // Destination window B at a DIFFERENT size (40x120).
        let s1 = app.next_session_id;
        let wid_b = app.insert_logical_window(stub_session(s1), 40, 120);
        // Give window 0 a 2nd tab (session s2) so the source SURVIVES the move, and
        // make window 0 the migration source.
        let s2 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(s2));
        app.frontmost_window = Some(WindowId(0));
        app.sync_active_session();
        // The stub session starts at 24x80 (the source window's size).
        {
            let s = app.pool.get(s2).expect("s2 pooled");
            let t = super::term_lock(&s.term);
            assert_eq!(
                (t.rows(), t.cols()),
                (24, 80),
                "s2 starts at the source geometry"
            );
        }
        // Move window 0's active tab (s2) into window B (40x120).
        app.migrate_active_tab_to_next_window();
        assert_eq!(
            app.frontmost_window,
            Some(wid_b),
            "focus follows the moved tab"
        );
        // resize_panes(B) reflowed the moved session to B's grid (single-pane = full).
        {
            let s = app.pool.get(s2).expect("s2 still pooled (pure view-move)");
            let t = super::term_lock(&s.term);
            assert_eq!(
                (t.rows(), t.cols()),
                (40, 120),
                "s2 reflowed to the destination geometry"
            );
        }
        assert!(app.structural_invariants_ok());
    }

    /// `close_active_tab` returns the window whose LAST tab it closed (the FRONTMOST),
    /// so the keyboard/menu escalation sets `pending_close` on THAT window — not the
    /// event-stamped one. (Audit gap: the escalation handshake was untested.)
    #[test]
    fn close_active_last_tab_reports_the_frontmost_window() {
        let mut app = App::headless_for_test(); // window 0 (single tab), session 0
        // A 2nd window; window 0 stays single-tab and is made the front.
        let s1 = app.next_session_id;
        let w1 = app.insert_logical_window(stub_session(s1), 24, 80);
        app.frontmost_window = Some(WindowId(0));
        app.sync_active_session();
        // Closing window 0's only tab reports window 0 (the frontmost) for escalation.
        assert_eq!(
            app.close_active_tab(),
            Some(WindowId(0)),
            "the LAST tab of the frontmost window → escalate-close THAT window",
        );
        // A surviving multi-tab window returns None (no escalation).
        let s2 = app.next_session_id;
        app.push_stub_tab(w1, stub_session(s2)); // w1 now has 2 tabs
        app.frontmost_window = Some(w1);
        app.sync_active_session();
        assert_eq!(
            app.close_active_tab(),
            None,
            "closing one of several tabs does not escalate"
        );
    }

    /// REGRESSION (self-audit of 595dfb7): the GPU-attach-failure rollback of "Move
    /// Tab to New Window" must RETURN the moved tab to the source window, NOT
    /// `close_window_logical` the new window — which would detach the moved session's
    /// SOLE view (1→0) and DESTROY the live shell.
    #[test]
    fn detach_rollback_returns_the_moved_session_no_data_loss() {
        let mut app = App::headless_for_test(); // window 0, session 0
        // A 2nd tab on window 0 so a detach is allowed (source needs >1 tab).
        let s1 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(s1)); // tabs [0, s1], active=s1
        let pool_before = app.pool.iter().count();
        assert_eq!(pool_before, 2);

        // Detach the active tab (s1) into a new window B — a PURE view-move (no pool
        // churn): s1 is now B's SOLE view.
        let wid_b = app
            .detach_active_tab_logical()
            .expect("detach moves the active tab to B");
        assert_eq!(app.windows.len(), 2);
        assert!(app.pool.get(s1).is_some(), "moved session still pooled");
        assert_eq!(
            app.pool.iter().count(),
            pool_before,
            "no pool churn on the move"
        );

        // Simulate the GPU-attach failure → rollback.
        app.detach_rollback_logical(Some(WindowId(0)), wid_b);

        // The session SURVIVED (not detached/dropped) and the tab is back in A; B gone.
        assert!(
            app.pool.get(s1).is_some(),
            "rollback PRESERVES the moved session — no data loss"
        );
        assert_eq!(
            app.pool.iter().count(),
            pool_before,
            "rollback detached/dropped nothing"
        );
        assert!(
            !app.windows.contains_key(&wid_b),
            "the failed new window is dropped"
        );
        assert_eq!(app.windows.len(), 1, "back to one window");
        assert!(
            app.windows
                .get(&WindowId(0))
                .is_some_and(|ws| ws.layouts.iter().any(|t| t.contains(s1))),
            "the tab is back in the source window",
        );
        assert_eq!(app.frontmost_window, Some(WindowId(0)));
        assert!(app.structural_invariants_ok());
    }

    /// REGRESSION (self-audit of 595dfb7): a shared session's grid is sized to the min
    /// of its FOREGROUND viewers (active-tab), NOT every window holding it in any tab.
    /// A background-tab viewer must NOT shrink the grid for the foreground viewer.
    #[test]
    fn shared_target_geometry_uses_foreground_viewers_only() {
        let mut app = App::headless_for_test(); // window 0 = 24x80, session 0
        let bwid = app
            .open_active_session_in_new_window_logical()
            .expect("share session 0 into window B");
        assert_eq!(app.pool.views(0), Some(2));
        // Simulate a drag-resize of B to a larger size.
        if let Some(ws) = app.windows.get_mut(&bwid) {
            ws.rows = 60;
            ws.cols = 200;
        }
        // Both windows show S in their ACTIVE tab → min of A(24x80) and B(60x200).
        assert_eq!(
            app.shared_target_geometry(0),
            (24, 80),
            "both foreground → min"
        );

        // BACKGROUND S in window 0 (a new tab there becomes active).
        let s2 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(s2));
        // Only B now shows S in its active tab. The grid must follow B (60x200), NOT
        // be clamped by window 0's 24x80 — window 0 isn't painting S (the regression).
        assert_eq!(
            app.shared_target_geometry(0),
            (60, 200),
            "a background-tab viewer is ignored; the foreground viewer's size wins",
        );
    }

    /// REGRESSION (self-audit of 595dfb7): the last-tab Cmd-W escalation flag is set on
    /// the FRONTMOST window (which `close_active_tab` operates on), so the escalation
    /// must close THAT window — even when it is not the OS-event window.
    #[test]
    fn pending_close_flag_lands_on_the_frontmost_for_escalation() {
        let mut app = App::headless_for_test(); // window 0, single tab
        let w1 = app.insert_logical_window(stub_session(app.next_session_id), 24, 80);
        // Logical frontmost = window 0 (single tab); imagine OS focus lags on w1.
        app.frontmost_window = Some(WindowId(0));
        app.sync_active_session();
        // Cmd-W: close_active_tab closes the FRONTMOST's last tab and reports it; the
        // producer flags THAT window (not the event window).
        let closed = app.close_active_tab().expect("frontmost's last tab closed");
        assert_eq!(closed, WindowId(0));
        if let Some(ws) = app.windows.get_mut(&closed) {
            ws.pending_close = true;
        }
        // The escalation scan (mirrors `escalate_pending_close`) finds the FLAGGED
        // window (0), not the OS-event window — and closing it leaves w1.
        let flagged: Vec<WindowId> = app
            .windows
            .iter()
            .filter(|(_, ws)| ws.pending_close)
            .map(|(w, _)| *w)
            .collect();
        assert_eq!(
            flagged,
            vec![WindowId(0)],
            "flag is on the frontmost that was closed"
        );
        assert_eq!(app.close_window_logical(WindowId(0)), CloseOutcome::Stay);
        assert_eq!(app.windows.keys().copied().collect::<Vec<_>>(), vec![w1]);
    }

    /// `App::move_tab` (the drag-to-reorder model op behind `tab move <from> <to>` and
    /// the native mouse-drag gesture): it permutes the window's `layouts` Vec and FIXES
    /// `tabs.active` so the SAME session stays selected through the move, preserving the
    /// `count == layouts.len()` / `active < count` invariants.
    #[test]
    fn move_tab_reorders_and_tracks_active() {
        let mut app = App::headless_for_test();
        // Window 0 starts with one tab (session 0); push two more → tabs [0,1,2].
        let s1 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(s1));
        let s2 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(s2));
        let order = |app: &App| {
            app.windows[&WindowId(0)]
                .layouts
                .iter()
                .map(|t| t.focus())
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&app), vec![0, s1, s2], "three tabs in append order");
        // After the two pushes, the last tab (session s2, index 2) is active.
        assert_eq!(app.windows[&WindowId(0)].tabs.active, 2);

        // Move the ACTIVE tab (index 2) to the front (index 0): the session follows,
        // so active becomes 0 and the order rotates s2 to the front.
        app.move_tab(WindowId(0), 2, 0);
        assert_eq!(
            order(&app),
            vec![s2, 0, s1],
            "active tab moved to the front"
        );
        assert_eq!(
            app.windows[&WindowId(0)].tabs.active,
            0,
            "active follows the moved tab"
        );
        assert_eq!(
            app.windows[&WindowId(0)].active_id,
            s2,
            "re-mirrored onto the moved tab"
        );

        // Move a tab from AFTER the active to BEFORE it: active shifts up one (the
        // viewed session is unchanged). Order is [s2,0,s1]; move index 2 (s1) to 0.
        app.move_tab(WindowId(0), 2, 0);
        assert_eq!(order(&app), vec![s1, s2, 0]);
        assert_eq!(
            app.windows[&WindowId(0)].tabs.active,
            1,
            "active shifted to follow s2"
        );
        assert_eq!(
            app.windows[&WindowId(0)].active_id,
            s2,
            "still viewing the same session"
        );

        // Out-of-range / identity moves are no-ops.
        let snapshot = order(&app);
        let active_before = app.windows[&WindowId(0)].tabs.active;
        app.move_tab(WindowId(0), 1, 1); // identity
        app.move_tab(WindowId(0), 9, 0); // from out of range
        app.move_tab(WindowId(0), 0, 9); // to out of range
        assert_eq!(order(&app), snapshot, "no-op moves leave order unchanged");
        assert_eq!(app.windows[&WindowId(0)].tabs.active, active_before);
        assert_eq!(
            app.windows[&WindowId(0)].tabs.count,
            3,
            "count preserved (pure permutation)"
        );
        assert!(app.structural_invariants_ok());
    }

    /// REGRESSION (audit, swallow class): the GLOBAL control `ActiveHandle` must
    /// follow the FRONT window's active tab across an APPEND and a CLOSE — not just
    /// the per-window mirror. A stale handle drives control-socket verbs (text/feed/
    /// signal) at the wrong, or a just-closed, session — and `Owner`/aterm-ctl verbs
    /// bypass the per-request edge gate, so they hit whatever it points at.
    #[test]
    fn active_handle_follows_front_tab_append_and_close() {
        let mut app = App::headless_for_test();
        let active = |app: &App| app.active_handle.lock().unwrap().id;
        assert_eq!(active(&app), 0, "starts on session 0");

        // Append a tab on the FRONT window → the global handle follows to it
        // (pre-fix `push_stub_tab` synced only the per-window mirror).
        let s1 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(s1));
        assert_eq!(
            app.windows[&WindowId(0)].tabs.active,
            1,
            "new tab is active"
        );
        assert_eq!(
            active(&app),
            s1,
            "global handle follows the appended front tab"
        );

        // Close the active tab (Cmd-W) → switches to tab 0; the handle follows back
        // (pre-fix `apply_close_outcome` synced only the per-window mirror).
        app.close_active_tab();
        assert_eq!(
            app.windows[&WindowId(0)].tabs.active,
            0,
            "switched back to tab 0"
        );
        assert_eq!(
            active(&app),
            0,
            "global handle follows the close-induced switch"
        );
        assert!(app.structural_invariants_ok());
    }

    /// REGRESSION (audit): closing the FOCUSED pane of a SPLIT tab in the front
    /// window must re-point the global handle at the surviving sibling — the
    /// `Collapsed` branch of `apply_close_outcome` synced only the per-window mirror,
    /// leaving the handle (and its master fd) on the just-closed pane's session.
    #[test]
    fn active_handle_follows_split_pane_close() {
        let mut app = App::headless_for_test();
        // Split the front window's active tab: a new (focused) pane, session s1.
        let s1 = app.split_active_stub_tab(WindowId(0));
        app.sync_active_session(); // establish: the focused split pane is the global active
        assert_eq!(
            app.active_handle.lock().unwrap().id,
            s1,
            "focused split pane is active"
        );

        // Close the focused pane → collapses onto the sibling (session 0), which
        // becomes focused; the global handle must follow it, not stay on closed s1.
        app.close_active_tab();
        assert_eq!(
            app.active_handle.lock().unwrap().id,
            0,
            "handle follows to the surviving sibling, not the closed pane",
        );
        assert!(app.structural_invariants_ok());
    }
}

#[cfg(test)]
mod tab_index_tests {
    use super::TabIndex;

    /// Adding a tab appends it and makes it active (open-and-switch), and the
    /// count grows by one each time.
    #[test]
    fn add_switches_to_new_tab() {
        let mut t = TabIndex::new(0, 1);
        assert_eq!(t.add(), 1); // second tab → active index 1
        assert_eq!(
            t,
            TabIndex {
                active: 1,
                count: 2
            }
        );
        assert_eq!(t.add(), 2); // third tab → active index 2
        assert_eq!(
            t,
            TabIndex {
                active: 2,
                count: 3
            }
        );
    }

    /// Cmd-1..9: switch to an existing tab; out-of-range is a no-op.
    #[test]
    fn switch_to_clamps_to_range() {
        let mut t = TabIndex::new(2, 3); // tabs 0,1,2; active 2
        assert_eq!(t.switch_to(0), 0);
        assert_eq!(t.switch_to(1), 1);
        // Out of range (Cmd-5 in a 3-tab window): no change.
        assert_eq!(t.switch_to(9), 1);
        assert_eq!(t.active, 1);
    }

    /// Cmd-Shift-]/[ cycles with WRAP; single/zero tab is a no-op.
    #[test]
    fn cycle_wraps_both_directions() {
        let mut t = TabIndex::new(0, 3);
        assert_eq!(t.cycle(true), 1);
        assert_eq!(t.cycle(true), 2);
        assert_eq!(t.cycle(true), 0); // wrap forward 2 → 0
        assert_eq!(t.cycle(false), 2); // wrap backward 0 → 2
        assert_eq!(t.cycle(false), 1);
        // One tab: cycling is a no-op in either direction.
        let mut one = TabIndex::new(0, 1);
        assert_eq!(one.cycle(true), 0);
        assert_eq!(one.cycle(false), 0);
        assert_eq!(
            one,
            TabIndex {
                active: 0,
                count: 1
            }
        );
    }

    /// Closing the LAST tab signals exit (returns true) without mutating state.
    #[test]
    fn close_last_tab_signals_exit() {
        let mut t = TabIndex::new(0, 1);
        assert!(t.close(0), "closing the only tab must signal exit");
        // Out-of-range close is a no-op that does NOT signal exit.
        let mut t2 = TabIndex::new(0, 2);
        assert!(!t2.close(5));
        assert_eq!(
            t2,
            TabIndex {
                active: 0,
                count: 2
            }
        );
    }

    /// Closing a tab BEFORE the active one shifts the active index down so it
    /// still points at the same session.
    #[test]
    fn close_before_active_shifts_active_down() {
        let mut t = TabIndex::new(2, 3); // tabs 0,1,2; active 2
        assert!(!t.close(0)); // remove tab 0
        assert_eq!(
            t,
            TabIndex {
                active: 1,
                count: 2
            }
        ); // old tab 2 is now index 1
    }

    /// Closing the ACTIVE tab clamps active into the new range (closing the
    /// last-in-list active tab moves focus to the new last tab).
    #[test]
    fn close_active_clamps_into_range() {
        // Active is the last tab: closing it moves focus to the new last.
        let mut t = TabIndex::new(2, 3);
        assert!(!t.close(2));
        assert_eq!(
            t,
            TabIndex {
                active: 1,
                count: 2
            }
        );
        // Active is a middle tab: closing it keeps the index (now points at the
        // tab that shifted into this slot).
        let mut m = TabIndex::new(1, 3);
        assert!(!m.close(1));
        assert_eq!(
            m,
            TabIndex {
                active: 1,
                count: 2
            }
        );
    }

    /// Closing a tab AFTER the active one leaves the active index unchanged.
    #[test]
    fn close_after_active_keeps_active() {
        let mut t = TabIndex::new(0, 3);
        assert!(!t.close(2));
        assert_eq!(
            t,
            TabIndex {
                active: 0,
                count: 2
            }
        );
    }

    /// Repeated add/close keeps `active < count` (the invariant the renderer
    /// relies on) at every step.
    #[test]
    fn add_close_cycle_keeps_active_in_range() {
        let mut t = TabIndex::new(0, 1);
        t.add(); // 2 tabs, active 1
        t.add(); // 3 tabs, active 2
        t.add(); // 4 tabs, active 3
        assert!(t.active < t.count);
        t.switch_to(1); // active 1 of 4
        assert!(!t.close(0)); // remove tab 0 → active shifts to 0, count 3
        assert!(t.active < t.count, "active {} count {}", t.active, t.count);
        assert!(!t.close(t.active)); // close active → clamp
        assert!(t.active < t.count);
        t.cycle(true);
        assert!(t.active < t.count);
    }

    // ── Tier-1 CONFORMANCE: bind the REAL `TabIndex` to the ty-proven `TabNav` ──
    //
    // The same `tab_nav_model()` that aterm-spec Tier-0 `ty check`s (proves
    // ActiveInRange + CountPositive over the whole bounded space and catches the
    // forgot-to-reclamp bug) is bound here to the genuine shipping `TabIndex`.
    // After EACH real mutator we (1) project the real `(active, count)` onto the
    // model vars and assert both model invariants hold on the projected real state,
    // and (2) assert the real `prev -> next` step is an ADMISSIBLE transition of the
    // matching model action — i.e. the real next-state appears in the model's
    // `successors` fan-out. So the model is about the program that actually
    // compiled, not a parallel re-statement.
    use aterm_spec::derive::{Model, tab_nav_model};
    use std::collections::BTreeMap;

    /// Project a real `TabIndex` onto the model's `{ count, active }` variables.
    fn project(t: &TabIndex) -> BTreeMap<&'static str, i64> {
        let mut s = BTreeMap::new();
        s.insert("count", t.count as i64);
        s.insert("active", t.active as i64);
        s
    }

    /// Assert both model invariants hold on a (projected) state.
    fn assert_invariants(m: &Model, s: &BTreeMap<&'static str, i64>, ctx: &str) {
        assert!(
            m.check_invariant("CountPositive", s),
            "CountPositive violated {ctx}: {s:?}"
        );
        assert!(
            m.check_invariant("ActiveInRange", s),
            "ActiveInRange violated {ctx}: {s:?}"
        );
    }

    /// Assert the real `prev -> next` step is one the model's `action` admits (the
    /// real next-state is in the action's existential fan-out). This is the
    /// transition-level conformance: the real code only ever moves the way the
    /// ty-proven model says it may.
    fn assert_admissible(
        m: &Model,
        action: &str,
        prev: &BTreeMap<&'static str, i64>,
        next: &BTreeMap<&'static str, i64>,
    ) {
        let succ = m.successors(action, prev);
        assert!(
            succ.iter().any(|s| s == next),
            "real {action} step {prev:?} -> {next:?} is NOT an admissible model transition; \
             model admits {succ:?}"
        );
    }

    #[test]
    fn real_tab_index_conforms_to_ty_proven_model() {
        let m = tab_nav_model();

        // Start from a fresh window (one active tab) — the model's init state.
        let mut t = TabIndex::new(0, 1);
        assert_eq!(
            project(&t),
            m.init_state(),
            "real init must match the model init"
        );
        assert_invariants(&m, &project(&t), "at init");

        // A scripted run touching every action: NewTab x3, SelectTab, Cycle, and
        // Close in the worst-case (active-is-last) shape that stresses the re-clamp.
        // After each real mutator: invariants hold AND the step is admissible.

        // NewTab x3 -> 4 tabs (Cap), active follows to the new last each time.
        for _ in 0..3 {
            let prev = project(&t);
            t.add();
            let next = project(&t);
            assert_admissible(&m, "NewTab", &prev, &next);
            assert_invariants(&m, &next, "after NewTab");
        }
        assert_eq!(
            t,
            TabIndex {
                active: 3,
                count: 4
            }
        );

        // SelectTab: jump to an in-range index (Cmd-1..9). The model's SelectTab is
        // nondeterministic (`active' \in 0..count-1`), so the real in-range landing
        // must be ONE admissible successor.
        for i in [0usize, 2, 1, 3] {
            let prev = project(&t);
            t.switch_to(i);
            let next = project(&t);
            assert_admissible(&m, "SelectTab", &prev, &next);
            assert_invariants(&m, &next, "after SelectTab");
        }

        // Cycle forward with wrap (Cmd-Shift-]): from each position incl. the wrap.
        for _ in 0..5 {
            let prev = project(&t);
            t.cycle(true);
            let next = project(&t);
            assert_admissible(&m, "Cycle", &prev, &next);
            assert_invariants(&m, &next, "after Cycle");
        }

        // Close in the worst case for the range invariant: focus the LAST tab, then
        // close it — the active index must re-clamp down to the new last. Repeat
        // until one tab remains (the model's Close is guarded `count > 1`).
        while t.count > 1 {
            t.switch_to(t.count - 1); // active is the last tab
            let prev = project(&t);
            let exit = t.close(t.active);
            assert!(!exit, "closing a non-last tab never signals exit");
            let next = project(&t);
            assert_admissible(&m, "Close", &prev, &next);
            assert_invariants(&m, &next, "after Close(active==last)");
        }
        assert_eq!(
            t,
            TabIndex {
                active: 0,
                count: 1
            }
        );

        // Also exercise Close of an EARLIER tab (active shifts down): build back up,
        // then close index 0 while active is later. The model's `Close` clamp covers
        // this too (active <= count-2 stays in range after the shrink).
        t.add();
        t.add(); // 3 tabs, active 2
        t.switch_to(2);
        let prev = project(&t);
        let exit = t.close(0); // earlier tab: active shifts 2 -> 1
        assert!(!exit);
        let next = project(&t);
        assert_eq!(
            t,
            TabIndex {
                active: 1,
                count: 2
            }
        );
        assert_admissible(&m, "Close", &prev, &next);
        assert_invariants(&m, &next, "after Close(earlier tab)");

        // ── NEGATIVE CONTROL (a pass is never vacuous) ──────────────────────────
        // 1. The model's `ActiveInRange` invariant REJECTS an out-of-range state —
        //    if it accepted everything, every assert above would be vacuous.
        let mut bad = project(&t);
        bad.insert("active", bad["count"]); // active == count: one past the last tab
        assert!(
            !m.check_invariant("ActiveInRange", &bad),
            "ActiveInRange must REJECT active==count (else the conformance is vacuous)"
        );

        // 2. The defect the model catches IS the defect the real `close` avoids:
        //    reproduce the buggy "forgot to re-clamp" Close by hand (decrement count
        //    but leave active at the old last index) and show it lands EXACTLY on
        //    the state the model's invariant rejects — and is NOT an admissible
        //    `Close` of the correct model (so the real clamp is load-bearing).
        let last_focused = TabIndex {
            active: 2,
            count: 3,
        }; // active is the last tab
        let prev = project(&last_focused);
        let buggy_next = {
            let mut s = prev.clone();
            s.insert("count", 2); // count-- , but active NOT re-clamped (stays 2)
            s
        };
        assert!(
            !m.check_invariant("ActiveInRange", &buggy_next),
            "the forgot-to-reclamp result MUST violate ActiveInRange"
        );
        assert!(
            !m.successors("Close", &prev)
                .iter()
                .any(|s| s == &buggy_next),
            "the buggy unclamped Close is NOT an admissible transition of the correct model"
        );
        // The REAL close on the same state re-clamps and DOES conform.
        let mut real = last_focused;
        assert!(!real.close(2));
        let real_next = project(&real);
        assert_admissible(&m, "Close", &prev, &real_next);
        assert_invariants(&m, &real_next, "real close re-clamps (no bug)");
    }
}

#[cfg(test)]
mod early_out_tests {
    use super::{RepaintKey, SelectionFingerprint};
    use crate::app_render::should_repaint;
    use aterm_core::terminal::{CursorStyle, Terminal};

    /// Build the `RepaintKey` for the current frame exactly as `redraw()` does:
    /// observe the damage epoch, the selection, and the supplied visual-only
    /// state. Returns the key; the caller decides whether to "present" (which in
    /// `redraw()` consumes the damage via `take_damage`).
    fn frame_key(
        term: &mut Terminal,
        blink_phase: bool,
        invert: bool,
        cursor_override: Option<CursorStyle>,
    ) -> RepaintKey {
        RepaintKey {
            damage_epoch: term.damage_epoch(),
            blink_phase,
            invert,
            cursor_override,
            selection: SelectionFingerprint::of(term.text_selection()),
            // No tab strip in these unit frames (the disabled-strip sentinel), so the
            // key matches the byte-identical no-strip path.
            tab_strip: 0,
        }
    }

    /// The first frame always paints (no previous present recorded).
    #[test]
    fn first_frame_always_repaints() {
        let mut term = Terminal::new(4, 10);
        let key = frame_key(&mut term, true, false, None);
        assert!(should_repaint(None, key), "the first frame must repaint");
    }

    /// THE acceptance behavior: after a present with NO subsequent terminal
    /// mutation and unchanged blink/flash/selection/focus, the next redraw
    /// decision is "skip" — i.e. `redraw()` would NOT re-rasterize.
    #[test]
    fn steady_screen_skips_rerasterize_after_present() {
        let mut term = Terminal::new(4, 10);
        term.process(b"steady");

        // Frame 1: present (record the key, consume the damage like redraw does).
        let k1 = frame_key(&mut term, true, false, None);
        assert!(should_repaint(None, k1), "first present");
        term.take_damage();
        let last_present = Some(k1);

        // Frame 2: nothing changed (no write, same blink/flash/selection/focus).
        let k2 = frame_key(&mut term, true, false, None);
        assert!(
            !should_repaint(last_present, k2),
            "an unchanged steady screen must skip the extract + rasterize + present"
        );
    }

    /// A blink-phase flip (the cursor-blink-only wake) STILL repaints — the
    /// regression this early-out must not introduce.
    #[test]
    fn blink_flip_still_repaints() {
        let mut term = Terminal::new(4, 10);
        term.process(b"x");
        let k1 = frame_key(&mut term, true, false, None);
        term.take_damage();
        // Blink toggles off; grid is otherwise unchanged.
        let k2 = frame_key(&mut term, false, false, None);
        assert!(should_repaint(Some(k1), k2), "a blink flip must repaint");
    }

    /// A bell-flash toggle, a focus change (cursor override), a selection change,
    /// a scroll, and a write each STILL repaint.
    #[test]
    fn visual_and_grid_changes_still_repaint() {
        let mut term = Terminal::new(4, 10);
        for _ in 0..8 {
            term.process(b"row\r\n");
        }
        let base = frame_key(&mut term, true, false, None);
        term.take_damage();

        // Bell flash on.
        let flash = frame_key(&mut term, true, true, None);
        assert!(
            should_repaint(Some(base), flash),
            "a bell flash must repaint"
        );

        // Unfocused: hollow cursor override.
        let unfocused = frame_key(&mut term, true, false, Some(CursorStyle::HollowBlock));
        assert!(
            should_repaint(Some(base), unfocused),
            "a focus change must repaint"
        );

        // Selection change (mutates the selection, NOT the grid damage tracker).
        term.text_selection_mut().start_selection(
            0,
            0,
            aterm_core::selection::SelectionSide::Left,
            aterm_core::selection::SelectionType::Simple,
        );
        let sel = frame_key(&mut term, true, false, None);
        assert!(
            should_repaint(Some(base), sel),
            "a selection change must repaint"
        );
        term.text_selection_mut().clear();
        term.take_damage();

        // A scroll (damages the grid => advances the epoch).
        let pre_scroll = frame_key(&mut term, true, false, None);
        term.take_damage();
        term.grid_mut().scroll_display(2);
        let scrolled = frame_key(&mut term, true, false, None);
        assert!(
            should_repaint(Some(pre_scroll), scrolled),
            "a scroll must repaint"
        );
        term.take_damage();

        // A write (damages the grid => advances the epoch).
        let pre_write = frame_key(&mut term, true, false, None);
        term.take_damage();
        term.process(b"more");
        let written = frame_key(&mut term, true, false, None);
        assert!(
            should_repaint(Some(pre_write), written),
            "a write must repaint"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{find_url_span, is_safe_url};
    use crate::app_config::Config;

    fn url_at(line: &str, col: usize) -> Option<String> {
        let chars: Vec<char> = line.chars().collect();
        find_url_span(&chars, col).map(|(u, _, _)| u)
    }

    #[test]
    fn config_parsing() {
        // Full config.
        let c: Config =
            toml::from_str("font_px = 24.0\ngpu = true\nscrollback_lines = 5000").unwrap();
        assert_eq!(c.font_px, Some(24.0));
        assert_eq!(c.gpu, Some(true));
        assert_eq!(c.scrollback_lines, Some(5000));
        // scrollback maps into the engine config: N -> cap, 0 -> unlimited (None).
        assert_eq!(c.terminal_config().unwrap().scrollback_limit, Some(5000));
        let unlimited: Config = toml::from_str("scrollback_lines = 0").unwrap();
        assert_eq!(unlimited.terminal_config().unwrap().scrollback_limit, None);
        // No engine-side keys -> no TerminalConfig to apply.
        assert!(
            toml::from_str::<Config>("font_px = 18.0")
                .unwrap()
                .terminal_config()
                .is_none()
        );
        // Cursor: shape + blink -> CursorStyle variant.
        use aterm_core::terminal::CursorStyle;
        let c: Config = toml::from_str("cursor_style = \"bar\"\ncursor_blink = false").unwrap();
        assert_eq!(
            c.terminal_config().unwrap().cursor_style,
            CursorStyle::SteadyBar
        );
        let c: Config = toml::from_str("cursor_style = \"underline\"").unwrap();
        assert_eq!(
            c.terminal_config().unwrap().cursor_style,
            CursorStyle::BlinkingUnderline
        );
        // Bad shape falls back (doesn't crash).
        let c: Config = toml::from_str("cursor_style = \"weird\"").unwrap();
        assert_eq!(
            c.terminal_config().unwrap().cursor_style,
            CursorStyle::BlinkingBlock
        );
        // Partial + UNKNOWN keys/tables ignored (forward-compatible).
        let c: Config =
            toml::from_str("gpu = false\nfuture_key = \"x\"\n[unknown]\nk = 1").unwrap();
        assert_eq!(c.font_px, None);
        assert_eq!(c.gpu, Some(false));
        // Empty → all defaults (None).
        let c: Config = toml::from_str("").unwrap();
        assert!(c.font_px.is_none() && c.gpu.is_none());
        // Wrong type → parse error (load_config warns + falls back to defaults).
        assert!(toml::from_str::<Config>("font_px = \"big\"").is_err());
        // Initial size.
        let c: Config = toml::from_str("columns = 120\nlines = 40").unwrap();
        assert_eq!((c.columns, c.lines), (Some(120), Some(40)));
        // Search depth: parses; the App clamp maps None -> default, Some(n) -> n
        // (saturated to i32::MAX) — mirror that clamp here.
        let depth = |cfg: &Config| {
            cfg.search_history_lines
                .map_or(super::MAX_SEARCH_HISTORY, |n| n.min(i32::MAX as u32) as i32)
        };
        assert_eq!(depth(&Config::default()), super::MAX_SEARCH_HISTORY); // unset → default
        let c: Config = toml::from_str("search_history_lines = 50000").unwrap();
        assert_eq!((c.search_history_lines, depth(&c)), (Some(50_000), 50_000));
        let c: Config = toml::from_str("search_history_lines = 0").unwrap();
        assert_eq!(depth(&c), 0); // 0 → live screen only
        let c: Config = toml::from_str("search_history_lines = 5000000").unwrap();
        assert_eq!(depth(&c), 5_000_000); // well within i32::MAX, no saturation
        // Tab strip rows: parses as a u16; default (unset) resolves to 1 (VISIBLE
        // tabs out of the box); an over-large value is clamped to MAX_TAB_STRIP_ROWS;
        // 0 hides the strip. (Asserted without setting the env var so the test stays
        // deterministic — env precedence is exercised by `resolve_tab_strip_rows`.)
        let c: Config = toml::from_str("tab_strip_rows = 2").unwrap();
        assert_eq!(c.tab_strip_rows, Some(2));
        assert_eq!(Config::default().tab_strip_rows, None); // unset
        // The clamp matches `resolve_tab_strip_rows` (default 0 — the in-grid strip is
        // OFF; tabs live in the native window toolbar — capped at MAX).
        let clamp = |n: Option<u16>| {
            n.unwrap_or(crate::app_config::DEFAULT_TAB_STRIP_ROWS)
                .min(crate::app_config::MAX_TAB_STRIP_ROWS)
        };
        assert_eq!(clamp(None), 0); // default → 0 (no in-grid strip)
        assert_eq!(clamp(Some(1)), 1); // opt back in to the in-grid strip
        assert_eq!(clamp(Some(0)), 0); // explicitly off
        assert_eq!(clamp(Some(99)), crate::app_config::MAX_TAB_STRIP_ROWS); // over-large clamped
        // Theme colours: hex parses; bad hex warns + is skipped (no crash).
        use crate::app_config::parse_hex_color;
        assert_eq!(
            parse_hex_color("#FF8800"),
            aterm_core::terminal::Rgb::new(0xFF, 0x88, 0x00).into()
        );
        assert_eq!(
            parse_hex_color("102030"),
            aterm_core::terminal::Rgb::new(0x10, 0x20, 0x30).into()
        );
        assert!(parse_hex_color("#xyz").is_none() && parse_hex_color("#12345").is_none());
        // Renderer theme from config: hex → Theme u32 (0x00RRGGBB).
        let c: Config = toml::from_str(
            "foreground = \"#102030\"\nbackground = \"#405060\"\nselection_color = \"#708090\"",
        )
        .unwrap();
        let th = c.theme();
        assert_eq!(
            (th.fg, th.bg, th.selection),
            (0x0010_2030, 0x0040_5060, 0x0070_8090)
        );
        // Unset → built-in defaults preserved.
        assert_eq!(
            Config::default().theme().selection,
            super::Theme::default().selection
        );
    }

    /// `option_as_meta` defaults to `true` (the current ESC-prefix behavior) when
    /// the key is absent — so no config is byte-identical. An explicit value wins.
    #[test]
    fn option_as_meta_defaults_true() {
        // Absent → true (today's Meta behavior; no regression).
        assert!(Config::default().option_as_meta_or_default());
        assert!(
            toml::from_str::<Config>("font_px = 18.0")
                .unwrap()
                .option_as_meta_or_default()
        );
        // Explicit false opts into composed characters; explicit true is honored.
        assert!(
            !toml::from_str::<Config>("option_as_meta = false")
                .unwrap()
                .option_as_meta_or_default()
        );
        assert!(
            toml::from_str::<Config>("option_as_meta = true")
                .unwrap()
                .option_as_meta_or_default()
        );
    }

    /// `font_family` parses as an optional string; absent is `None` (the default
    /// `$ATERM_FONT` → built-in chain), present is carried through verbatim.
    #[test]
    fn font_family_parses() {
        assert_eq!(Config::default().font_family, None);
        let c: Config = toml::from_str("font_family = \"JetBrains Mono\"").unwrap();
        assert_eq!(c.font_family.as_deref(), Some("JetBrains Mono"));
    }

    /// A `[keybindings]` table parses into the chord → action map, and an absent
    /// table yields an empty (no-op) map — so the no-config path reaches the
    /// hardcoded `on_key` matches unchanged.
    #[test]
    fn keybindings_table_parses() {
        // Absent → empty map (the hardcoded path is reached unchanged).
        let kb =
            crate::keybinding::Keybindings::from_config(Config::default().keybindings.as_ref());
        assert!(kb.is_empty());
        // A populated table resolves its chords to actions.
        let c: Config =
            toml::from_str("[keybindings]\n\"cmd+shift+n\" = \"new_tab\"\n\"ctrl+a\" = \"find\"\n")
                .unwrap();
        let kb = crate::keybinding::Keybindings::from_config(c.keybindings.as_ref());
        assert!(!kb.is_empty());
        use winit::keyboard::{Key as WK, ModifiersState as MS, SmolStr};
        assert_eq!(
            kb.lookup(&WK::Character(SmolStr::new("n")), MS::SUPER | MS::SHIFT),
            Some(crate::keybinding::Action::NewTab)
        );
        assert_eq!(
            kb.lookup(&WK::Character(SmolStr::new("a")), MS::CONTROL),
            Some(crate::keybinding::Action::Find)
        );
        // An UNBOUND chord misses → on_key falls through to the hardcoded match
        // (the no-regression precedence: config overrides, a miss is unchanged).
        assert_eq!(
            kb.lookup(&WK::Character(SmolStr::new("t")), MS::SUPER),
            None
        );
    }

    /// The real proof themes work: a config colour flows config → engine →
    /// the rendered `RenderCell.fg/bg` (no renderer change involved).
    #[test]
    fn theme_colors_reach_rendercell() {
        use aterm_core::terminal::Terminal;
        let c: Config =
            toml::from_str("foreground = \"#FF8800\"\nbackground = \"#102030\"").unwrap();
        let mut t = Terminal::new(4, 10);
        t.apply_config(&c.terminal_config().unwrap());
        t.process(b"X"); // a default-styled glyph uses the configured theme colours
        let cells = t.render_row(0);
        assert_eq!(cells[0].ch, 'X');
        assert_eq!(cells[0].fg, [0xFF, 0x88, 0x00]);
        assert_eq!(cells[0].bg, [0x10, 0x20, 0x30]);
    }

    /// The palette flows config → engine → rendered cell too: SGR 31 (ANSI index 1)
    /// resolves to the configured palette colour.
    #[test]
    fn palette_colors_reach_rendercell() {
        use aterm_core::terminal::Terminal;
        let c: Config = toml::from_str("palette = [\"#000000\", \"#AB12CD\"]").unwrap();
        let mut t = Terminal::new(4, 10);
        t.apply_config(&c.terminal_config().unwrap());
        t.process(b"\x1b[31mR"); // SGR fg = ANSI 1 (red) → palette index 1
        let cells = t.render_row(0);
        assert_eq!(cells[0].ch, 'R');
        assert_eq!(cells[0].fg, [0xAB, 0x12, 0xCD]);
    }

    /// A named `theme` flows config → engine → rendered cells: it sets the default
    /// fg/bg AND the full ANSI palette (incl. bold-to-bright), and the renderer Theme
    /// chrome. A per-key override still wins over the theme (last-wins precedence).
    #[test]
    fn named_theme_reaches_rendercell_and_chrome() {
        use aterm_core::terminal::Terminal;
        // Dracula: fg #f8f8f2, bg #282a36; ansi[1]=#ff5555 (red), ansi[12]=#d6acff
        // (bright blue, reached by bold-blue 1;34).
        let c: Config = toml::from_str("theme = \"Dracula\"").unwrap();
        // Renderer chrome comes from the scheme.
        let th = c.theme();
        assert_eq!(th.fg, 0x00f8_f8f2, "theme fg reaches the renderer Theme");
        assert_eq!(th.bg, 0x0028_2a36, "theme bg reaches the renderer Theme");
        // A named theme is engine-relevant, so the delta is Some (N1).
        assert!(
            c.terminal_config().is_some(),
            "a named theme makes terminal_config() Some"
        );
        // Engine default + palette reach rendered cells.
        let mut t = Terminal::new(4, 12);
        t.apply_config(&c.applied_terminal_config());
        t.process(b"A\x1b[31mR\x1b[0m\x1b[1;34mB");
        let cells = t.render_row(0);
        assert_eq!(cells[0].ch, 'A');
        assert_eq!(cells[0].fg, [0xf8, 0xf8, 0xf2], "default fg = Dracula fg");
        assert_eq!(cells[0].bg, [0x28, 0x2a, 0x36], "default bg = Dracula bg");
        assert_eq!(cells[1].fg, [0xff, 0x55, 0x55], "SGR 31 → Dracula ansi[1]");
        assert_eq!(
            cells[2].fg,
            [0xd6, 0xac, 0xff],
            "bold SGR 34 → Dracula ansi[12] (bright)"
        );

        // A per-key override beats the theme.
        let c2: Config = toml::from_str("theme = \"Dracula\"\nbackground = \"#102030\"").unwrap();
        assert_eq!(
            c2.theme().bg,
            0x0010_2030,
            "background override beats the theme"
        );
        let mut t2 = Terminal::new(2, 4);
        t2.apply_config(&c2.applied_terminal_config());
        t2.process(b"X");
        assert_eq!(t2.render_row(0)[0].bg, [0x10, 0x20, 0x30]);
    }

    /// A per-key `palette` entry layers OVER a named theme's ANSI slot (last-wins):
    /// the theme seeds all 16 slots, then `palette[i]` overwrites slot `i` (S8).
    #[test]
    fn palette_override_beats_named_theme() {
        use aterm_core::terminal::Terminal;
        // Dracula's ansi[1] (red) is #ff5555; override slot 1 to pure green.
        let c: Config =
            toml::from_str("theme = \"Dracula\"\npalette = [\"#000000\", \"#00FF00\"]").unwrap();
        let mut t = Terminal::new(2, 4);
        t.apply_config(&c.applied_terminal_config());
        t.process(b"\x1b[31mR"); // SGR fg = ANSI 1 → overridden to #00ff00, not Dracula red
        let cells = t.render_row(0);
        assert_eq!(
            cells[0].fg,
            [0x00, 0xff, 0x00],
            "palette[1] override beats the theme's ansi[1]"
        );
        assert_ne!(cells[0].fg, [0xff, 0x55, 0x55], "not Dracula's red");
    }

    /// An unknown `theme` name falls back to Default (never panics) and still makes
    /// the engine delta `Some`. (The one-time diagnostic is emitted but not asserted:
    /// stderr capture would need a dev-dependency — this covers FALLBACK behaviour +
    /// the engine-delta contract only, per N7.)
    #[test]
    fn unknown_theme_falls_back_to_default() {
        let c: Config = toml::from_str("theme = \"no-such-theme\"").unwrap();
        assert_eq!(c.theme().bg, super::Theme::default().bg);
        assert_eq!(c.theme().fg, super::Theme::default().fg);
        assert!(
            c.terminal_config().is_some(),
            "a theme key makes the engine delta Some"
        );
    }

    /// Live config hot-reload preserves the SAME font-size precedence as startup:
    /// `$ATERM_FONT_PX > config > default`, with the env value passed explicitly
    /// (so the test is deterministic, never mutating process-global env). An env
    /// override therefore still wins after a reload — the no-regression guarantee.
    #[test]
    fn font_px_precedence_env_beats_config_beats_default() {
        use super::{FONT_PX, FONT_PX_MAX, FONT_PX_MIN};
        use crate::app_config::resolve_font_px_with;
        // env wins over config.
        assert_eq!(resolve_font_px_with(Some("24"), Some(18.0)), 24.0);
        // No env → config wins over the built-in default.
        assert_eq!(resolve_font_px_with(None, Some(18.0)), 18.0);
        // Neither → built-in default.
        assert_eq!(resolve_font_px_with(None, None), FONT_PX);
        // A garbage env value falls through to the config (matches startup's
        // `.parse().ok().or(config)`), so a reload doesn't regress to default.
        assert_eq!(resolve_font_px_with(Some("big"), Some(18.0)), 18.0);
        // Out-of-range values (env and config) are filtered, falling to default.
        assert_eq!(resolve_font_px_with(Some("9999"), None), FONT_PX);
        assert_eq!(resolve_font_px_with(None, Some(0.0)), FONT_PX);
        // In-range bounds are honoured.
        assert_eq!(resolve_font_px_with(Some("6"), None), FONT_PX_MIN);
        assert_eq!(resolve_font_px_with(None, Some(200.0)), FONT_PX_MAX);
    }

    /// FAIL-SAFE: a malformed / partial mid-edit config must be REJECTED so a
    /// reload never clobbers the running config with parser defaults. This mirrors
    /// the strict re-parse `App::reload_config` does (`toml::from_str` must
    /// succeed before anything is applied) — a parse error means "keep current".
    #[test]
    fn malformed_reload_is_rejected_not_defaulted() {
        // A valid edit parses (would be applied).
        assert!(toml::from_str::<Config>("font_px = 22\nbackground = \"#101010\"").is_ok());
        // A mid-edit truncation (open string / dangling key) is a parse error —
        // reload_config logs + returns WITHOUT touching the live config.
        assert!(toml::from_str::<Config>("background = \"#1010").is_err());
        assert!(toml::from_str::<Config>("font_px = ").is_err());
        // A wrong-typed value is also rejected (not silently coerced to default).
        assert!(toml::from_str::<Config>("font_px = \"huge\"").is_err());
    }

    /// The reload engine path genuinely re-themes a LIVE terminal: applying a new
    /// `applied_terminal_config` recolours already-rendered default cells, and
    /// reverting the key reverts the colour — exactly what `App::reload_config` does
    /// to every session via `apply_config`. Crucially, the revert lands on the THEME
    /// background, never spec-black (the black-backed-text fix).
    #[test]
    fn reload_retheme_then_revert_live_terminal() {
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(4, 10);
        t.process(b"X");
        // Apply a themed config (config → engine), then re-render: the default
        // glyph picks up the new background, proving the live re-apply works.
        let themed: Config = toml::from_str("background = \"#203040\"").unwrap();
        t.apply_config(&themed.applied_terminal_config());
        assert_eq!(t.render_row(0)[0].bg, [0x20, 0x30, 0x40]);
        // Reverting the key: an empty config has no engine *delta* (`terminal_config()`
        // is None), but `applied_terminal_config()` still pins the engine default bg to
        // the THEME bg — so a default-styled cell reverts to the themed window
        // background, NOT the engine's spec-default black.
        let empty: Config = toml::from_str("").unwrap();
        assert!(empty.terminal_config().is_none());
        t.apply_config(&empty.applied_terminal_config());
        let theme_bg = {
            let bg = empty.theme().bg;
            [
                ((bg >> 16) & 0xff) as u8,
                ((bg >> 8) & 0xff) as u8,
                (bg & 0xff) as u8,
            ]
        };
        assert_eq!(
            t.render_row(0)[0].bg,
            theme_bg,
            "revert lands on the theme bg"
        );
        assert_ne!(
            t.render_row(0)[0].bg,
            [0, 0, 0],
            "revert never falls back to spec-black"
        );
        assert_ne!(t.render_row(0)[0].bg, [0x20, 0x30, 0x40]);
    }

    /// The black-backed-text fix, end to end: with NO colour config, an unstyled
    /// cell paints the THEME default fg/bg (`#D0D0D0` on `#111318`) — the same colour
    /// the window clears its margins/padding to — instead of the engine's spec
    /// default (white-on-black) leaking through as a darker text region.
    #[test]
    fn default_config_cell_uses_theme_colors_not_spec_black() {
        use aterm_core::terminal::Terminal;
        let c = Config::default(); // no foreground/background set
        let mut t = Terminal::new(4, 10);
        t.apply_config(&c.applied_terminal_config());
        t.process(b"X");
        let cells = t.render_row(0);
        assert_eq!(cells[0].ch, 'X');
        assert_eq!(
            cells[0].bg,
            [0x11, 0x13, 0x18],
            "unstyled bg = theme bg, not black"
        );
        assert_eq!(
            cells[0].fg,
            [0xD0, 0xD0, 0xD0],
            "unstyled fg = theme fg, not pure white"
        );
        assert_ne!(cells[0].bg, [0, 0, 0]);
    }

    /// A theme change retints the App-level tab strip — BUT only once the strip cache
    /// is invalidated, which `reload_config` does (`last_strip_fp = None` on a theme
    /// change). The cache key is `(fingerprint, cols)`, NOT the theme, so this proves
    /// (a) a stale cache serves the old theme until invalidated, and (b) after the
    /// invalidation `reload_config` performs, the active tab repaints in the new theme.
    #[test]
    fn hot_reload_retints_tab_strip() {
        let mut app = super::App::headless_for_test();
        app.tab_strip_rows = 1;
        let wid = super::WindowId(0);
        let active_bg = |app: &super::App| {
            app.windows[&wid].cached_strip_rows[0][0].bg // active tab's first cell
        };

        // Theme A (Default): paint + cache the strip.
        app.theme = super::Theme::default();
        app.splice_tab_strip_with(wid, 1, vec!["sh".to_string()]);
        let bg_a = active_bg(&app);

        // Switch to Dracula but DON'T invalidate: the cache (key unchanged) stale-serves.
        let tp = aterm_types::scheme::builtin("Dracula")
            .unwrap()
            .to_theme_parts();
        app.theme = super::Theme {
            fg: tp.fg,
            bg: tp.bg,
            cursor: tp.cursor,
            selection: tp.selection,
        };
        app.splice_tab_strip_with(wid, 1, vec!["sh".to_string()]);
        assert_eq!(
            active_bg(&app),
            bg_a,
            "without invalidation the cache serves the old theme"
        );

        // Invalidate exactly as reload_config does on a theme change → strip retints.
        if let Some(ws) = app.windows.get_mut(&wid) {
            ws.last_strip_fp = None;
        }
        app.splice_tab_strip_with(wid, 1, vec!["sh".to_string()]);
        assert_ne!(
            active_bg(&app),
            bg_a,
            "after invalidation the active tab repaints in the new theme"
        );
    }

    /// Cmd-F find core: `find_line_matches` is case-insensitive + non-overlapping,
    /// and highlighting a match via the selection (what `search_apply_current`
    /// does) round-trips to the matched text — so the find genuinely works.
    #[test]
    fn find_matches_and_highlight() {
        use crate::app_search::find_line_matches;
        let lines = vec![
            (0i32, "Hello hello HELLO".to_string()),
            (1, "world".to_string()),
        ];
        assert_eq!(
            find_line_matches(&lines, "hello"),
            vec![(0, 0, 4), (0, 6, 10), (0, 12, 16)]
        );
        assert!(find_line_matches(&lines, "xyz").is_empty());
        assert!(find_line_matches(&lines, "").is_empty());

        use aterm_core::selection::{SelectionSide, SelectionType};
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(4, 20);
        t.process(b"find ME here");
        let row0 = t.get_line_text(0, None).unwrap_or_default();
        let matches = find_line_matches(&[(0i32, row0)], "me");
        assert_eq!(matches.len(), 1);
        let (r, c0, c1) = matches[0];
        let sel = t.text_selection_mut();
        sel.start_selection(r, c0, SelectionSide::Left, SelectionType::Simple);
        sel.update_selection(r, c1, SelectionSide::Right);
        assert_eq!(t.selection_to_string().as_deref(), Some("ME"));
    }

    /// Scrollback find + scroll-to-match (what `search_recompute`/`apply_current`
    /// do): a match in HISTORY has a negative row, and `scroll_to_bottom` +
    /// `scroll_display(-row)` brings it into view (display_offset == -row) with the
    /// selection round-tripping to the matched text.
    #[test]
    fn search_scrollback_scroll_to_match() {
        use super::MAX_SEARCH_HISTORY;
        use crate::app_search::find_line_matches;
        use aterm_core::selection::{SelectionSide, SelectionType};
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(4, 20); // 4 visible rows
        for i in 0..12 {
            t.process(format!("LINE{i:02}\r\n").as_bytes()); // ~8 lines scroll off
        }
        t.scroll_to_bottom();
        // Gather scrollback (oldest→newest) + live, like search_recompute.
        let mut lines: Vec<(i32, String)> = Vec::new();
        let mut r = -1;
        while r >= -MAX_SEARCH_HISTORY {
            match t.get_line_text(r, None) {
                Some(s) => lines.push((r, s)),
                None => break,
            }
            r -= 1;
        }
        lines.reverse();
        for r in 0..4 {
            lines.push((r, t.get_line_text(r, None).unwrap_or_default()));
        }
        let matches = find_line_matches(&lines, "line03");
        assert_eq!(matches.len(), 1);
        let (row, c0, c1) = matches[0];
        assert!(row < 0, "LINE03 must be in scrollback, got row {row}");
        // Apply (mirrors search_apply_current).
        t.scroll_to_bottom();
        let sel = t.text_selection_mut();
        sel.start_selection(row, c0, SelectionSide::Left, SelectionType::Simple);
        sel.update_selection(row, c1, SelectionSide::Right);
        t.scroll_display(-row);
        assert_eq!(
            t.grid().display_offset() as i32,
            -row,
            "scrolled the match into view"
        );
        assert_eq!(t.selection_to_string().as_deref(), Some("LINE03"));
    }

    #[test]
    fn url_detection_in_text() {
        let line = "see (http://example.com/p?q=1). end";
        // "http" starts at col 5; the URL spans into the query.
        assert_eq!(url_at(line, 5).as_deref(), Some("http://example.com/p?q=1"));
        assert_eq!(
            url_at(line, 12).as_deref(),
            Some("http://example.com/p?q=1")
        );
        // Trailing `).` is trimmed, so the close-paren col is NOT in the URL.
        let close_paren = line.find(')').unwrap();
        assert_eq!(url_at(line, close_paren), None);
        // Outside any URL.
        assert_eq!(url_at(line, 0), None);
        assert_eq!(url_at(line, 2), None);
        // https + bare URL, whole-span membership.
        let bare = "https://a.b/c";
        assert_eq!(url_at(bare, 0).as_deref(), Some("https://a.b/c"));
        assert_eq!(
            url_at(bare, bare.len() - 1).as_deref(),
            Some("https://a.b/c")
        );
        // Not a URL scheme.
        assert_eq!(url_at("ftp://x.y not-a-link", 0), None);
    }

    #[test]
    fn safe_url_allowlist() {
        // Allowed: http/https/mailto, case-insensitive.
        for ok in [
            "http://example.com",
            "https://example.com/path?q=1#frag",
            "HTTPS://EXAMPLE.COM",
            "mailto:user@example.com",
            "  https://example.com  ", // surrounding whitespace is trimmed
        ] {
            assert!(is_safe_url(ok), "should allow: {ok:?}");
        }
        // Rejected: filesystem, app/custom schemes, injection, empties.
        for bad in [
            "file:///etc/passwd",
            "x-apple-systempreferences:com.apple.preference",
            "javascript:alert(1)",
            "tel:+15551234",
            "ftp://example.com",
            "http://exa mple.com", // internal whitespace
            "https://e\nvil.com",  // control byte
            "://noscheme",
            "",
            "   ",
        ] {
            assert!(!is_safe_url(bad), "should reject: {bad:?}");
        }
    }
}

#[cfg(test)]
mod session_pool_tests {
    use super::*;

    /// Build a minimal in-memory `Session` for pool tests: a real `Terminal` and
    /// `SessionCtx` but a sentinel `master = -1`, so `Session::drop` is a no-op
    /// (it only `close()`s a real fd). The pool's OWNERSHIP/REFCOUNT semantics are
    /// what we exercise here, not PTY teardown.
    fn test_session(id: u64) -> Session {
        let self_id = SessionId::generate();
        let ctx = Arc::new(SessionCtx {
            sink: Arc::new(SinkWriter::new(-1)),
            edges: std::sync::Mutex::new(EdgeTable::new()),
            self_id,
            nonce: LaunchNonce::generate(),
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
                80, 24,
            ))),
            temporal: Arc::new(std::sync::Mutex::new(
                crate::temporal::TemporalRecorder::new(),
            )),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        Session {
            id,
            term: Arc::new(Mutex::new(Terminal::new(24, 80))),
            master: -1,
            pid: -1,
            ctx,
            child_proxy_sid: None,
        }
    }

    #[test]
    fn insert_starts_at_one_view_and_is_resolvable() {
        let mut pool = SessionPool::default();
        assert_eq!(pool.insert(test_session(7)), 7);
        assert!(pool.get(7).is_some(), "inserted session resolves by id");
        assert_eq!(pool.iter().count(), 1);
        // An unknown id is fail-closed None (mirrors the registry discipline).
        assert!(pool.get(999).is_none());
    }

    #[test]
    fn detach_drops_session_exactly_when_last_view_leaves() {
        let mut pool = SessionPool::default();
        pool.insert(test_session(1));
        // A second window now views the one session (views = 2).
        pool.attach(1);
        // First detach: a view remains, so the session is NOT dropped — the
        // precondition for same-session-in-two-windows / detach with zero PTY churn.
        assert!(
            !pool.detach(1),
            "detach with a remaining view must not drop"
        );
        assert!(
            pool.get(1).is_some(),
            "session still owned while a view remains"
        );
        // Last detach: refcount hits 0, the session is dropped (its PTY would close).
        assert!(pool.detach(1), "the last detach drops the session");
        assert!(
            pool.get(1).is_none(),
            "a dropped session no longer resolves"
        );
        // Detaching an already-gone id is a fail-closed no-op, not a panic.
        assert!(!pool.detach(1));
        assert_eq!(pool.iter().count(), 0);
    }

    #[test]
    fn single_view_detach_drops_immediately() {
        let mut pool = SessionPool::default();
        pool.insert(test_session(3));
        assert!(
            pool.detach(3),
            "a single-view session drops on its only detach"
        );
        assert!(pool.get(3).is_none());
    }
}

/// Tier-1 trace conformance: bind the REAL `App` window lifecycle to the DERIVED,
/// ty-proven `window_routing_model()`.
///
/// `window_routing_model()` is model-checked in the abstract at Tier-0
/// (`aterm-spec/tests/derived_ring_ty.rs`) — that proves the *design* of the
/// create/close→exit + never-reuse routing sound, but nothing ties it to the
/// `App` code that actually runs. This test closes that gap: it drives the genuine
/// shipping `App` window seams (`insert_logical_window` = a real `CreateWindow`,
/// `close_window_logical` = a real `CloseWindow`, with the production
/// windows/pool/frontmost/`CloseOutcome` bookkeeping), projects each reachable
/// `App` state onto the spec variables `<<win_count, frontmost, next_id, exited>>`,
/// and asks the real `ty` binary to confirm every observed transition is one the
/// spec's `Next` admits. This is the "Tier 1" layer of
/// `docs/RFC-ty-embed-derived-tla.md`: model <-> executable.
///
/// SINGLE SOURCE — the spec here is NOT hand-written: it is generated from
/// `aterm_spec::derive::window_routing_model()`, the very same model Tier-0
/// exhaustively `ty check`s. One Rust source feeds both the exhaustive check and
/// this conformance binding, so the spec cannot drift from the model.
///
/// METHOD — strict per-transition validation, exactly as
/// `aterm-buffer/tests/conformance_eventlog.rs`: the derived spec's `Init` is
/// parameterized (`Model::transition_spec`, CONSTANTS `<var>_init`) and pinned to
/// `prev`, and a two-step trace `[prev, next]` is `ty trace validate`d. `ty`
/// strictly enforces `Init` and the FIRST transition against `Next`, so a
/// corrupted `next` is reliably rejected — which the negative controls assert, so a
/// pass is never vacuous.
///
/// `ty` is located by the SAME fixed canonical path search the eventlog test uses;
/// VERIFICATION GATE (honesty ratchet, batteries-on, see [`aterm_spec::verify`]):
/// verification is always required — an absent Trust `ty` FAILS the test with a build
/// hint (`cargo build --release -p tla-cli` in ~/trust/first-party/ty).
#[cfg(test)]
mod window_routing_conformance {
    use super::*;
    use aterm_spec::derive::window_routing_model;
    use aterm_spec::verify::ty;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;

    /// The scalar projection of the real `App` onto the spec variables
    /// `[win_count, frontmost, next_id, exited]`.
    ///
    /// The **+1 remap on `frontmost`/`next_id` is load-bearing**: `App` `WindowId`s
    /// are 0-based, but the model reserves `0` for "no frontmost window" (the empty
    /// set). So a live `WindowId(n)` projects to `n + 1` (always `> 0`), and the
    /// model's `frontmost = 0` means none — exactly what `FrontmostLive`
    /// (`frontmost=0 <=> win_count=0`) and `FrontmostAllocated`
    /// (`frontmost = 0 \/ frontmost < next_id`) reason about. `next_id` is likewise
    /// `next_window_id + 1` so the allocation frontier stays one above the live ids.
    fn project(app: &App, exited: bool) -> [i64; 4] {
        let win_count = app.windows.len() as i64;
        let frontmost = app.frontmost_window.map_or(0, |WindowId(n)| n as i64 + 1);
        let next_id = app.next_window_id as i64 + 1;
        [win_count, frontmost, next_id, exited as i64]
    }

    /// A two-step `ty` trace listing ALL FOUR variables in BOTH steps: `prev` (must
    /// match `Init`) then `next` (must match `action_name`). Module name is
    /// `WindowRouting` — it must match the derived spec's `---- MODULE WindowRouting ----`.
    fn transition_trace(action_name: &str, prev: [i64; 4], next: [i64; 4]) -> String {
        let st = |s: [i64; 4]| {
            format!(
                "{{\"win_count\":{{\"type\":\"int\",\"value\":{}}},\
                 \"frontmost\":{{\"type\":\"int\",\"value\":{}}},\
                 \"next_id\":{{\"type\":\"int\",\"value\":{}}},\
                 \"exited\":{{\"type\":\"int\",\"value\":{}}}}}",
                s[0], s[1], s[2], s[3]
            )
        };
        format!(
            "{{\"version\":\"1\",\"module\":\"WindowRouting\",\
             \"variables\":[\"win_count\",\"frontmost\",\"next_id\",\"exited\"],\"steps\":[\
             {{\"index\":0,\"state\":{}}},\
             {{\"index\":1,\"state\":{},\"action\":{{\"name\":\"{}\"}}}}\
             ]}}",
            st(prev),
            st(next),
            action_name
        )
    }

    /// Run `ty trace validate` for one real transition; returns (conforms, output).
    /// The spec + cfg are DERIVED from the SAME `window_routing_model()` that Tier-0
    /// exhaustively checks. `transition_spec()` parameterizes `Init`; the cfg pins
    /// it to `prev` and overrides `MaxWin`/`MaxId` to LARGE bounds so a real
    /// multi-window run's guards never spuriously reject — `Buggy` stays 0 (the
    /// committed, correct close→exit discipline the real `App` implements).
    fn validate_transition(
        ty: &Path,
        dir: &Path,
        action_name: &str,
        prev: [i64; 4],
        next: [i64; 4],
    ) -> (bool, String) {
        let m = window_routing_model();
        let spec = dir.join("WindowRouting.tla");
        let cfg = dir.join("WindowRouting.cfg");
        let trace = dir.join("t.json");
        let init: BTreeMap<&'static str, i64> = [
            ("win_count", prev[0]),
            ("frontmost", prev[1]),
            ("next_id", prev[2]),
            ("exited", prev[3]),
        ]
        .into_iter()
        .collect();
        std::fs::write(&spec, m.transition_spec()).expect("write spec");
        std::fs::write(
            &cfg,
            m.transition_cfg(&init, &[("MaxWin", 1_000_000), ("MaxId", 1_000_000_000)]),
        )
        .expect("write cfg");
        std::fs::write(&trace, transition_trace(action_name, prev, next)).expect("write trace");
        let out = Command::new(ty)
            .arg("trace")
            .arg("validate")
            .arg(&trace)
            .arg("--spec")
            .arg(&spec)
            .arg("--config")
            .arg(&cfg)
            .output()
            .expect("run ty trace validate");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    }

    #[test]
    fn real_app_window_routing_conforms() {
        let ty = ty("App window-routing conformance");
        run_conformance(&ty);
    }

    /// The window-routing Tier-1 conformance body, factored out so the
    /// `spec_xref_gate` can RUN it directly (TRUST_VACUITY_GATE §2.3 / finding 3): the
    /// gate's "window_routing Tier-1 already green" claim becomes TRUE — the gate
    /// invokes this, and if the real `App` close→exit routing is made to diverge from
    /// `WindowRouting.Next`, this fails and the gate fails with it. Takes the already-
    /// located `ty` so the caller owns the honesty ratchet.
    pub(crate) fn run_conformance(ty: &Path) {
        // Unique per-CALL tempdir: `run_conformance` is invoked by BOTH the standalone
        // `real_app_window_routing_conforms` test AND the `spec_xref_gate` (finding 3),
        // which run concurrently in the same test binary (same `process::id()`). A
        // per-process dir would race on the shared spec/cfg/trace files; a monotonic
        // counter makes each invocation's working dir distinct.
        use std::sync::atomic::{AtomicU64, Ordering};
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aterm-winroute-conf-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mk tempdir");

        // PROJECTION-DRIFT GUARD: the initial projection of the real headless App
        // (exited=false) MUST equal the model's `init_state` projected to the same
        // tuple (win_count=1, frontmost=1, next_id=2, exited=0). If the +1 remap or
        // the field reads drift, this fails before any ty call.
        let app0 = App::headless_for_test();
        let init = window_routing_model().init_state();
        let model_init = [
            init["win_count"],
            init["frontmost"],
            init["next_id"],
            init["exited"],
        ];
        assert_eq!(
            project(&app0, false),
            model_init,
            "headless App initial projection must equal window_routing_model().init_state() \
             — got {:?}, model init {:?} (the +1 frontmost/next_id remap is load-bearing)",
            project(&app0, false),
            model_init
        );
        assert_eq!(
            model_init,
            [1, 1, 2, 0],
            "sanity: model init is the documented tuple"
        );
        drop(app0);

        // Drive a REAL create/close sequence through the production seams and
        // strictly validate each transition against the derived `Next`.
        let mut app = App::headless_for_test();
        let mut exited = false;
        let mut validated = 0usize;

        // --- CreateWindow: a 2nd window (real wid/session minting + install).
        let prev = project(&app, exited);
        let sid1 = app.next_session_id;
        let _wid1 = app.insert_logical_window(stub_session(sid1), 24, 80);
        assert!(
            app.structural_invariants_ok(),
            "invariants hold after 1st create"
        );
        let next = project(&app, false);
        let (ok, out) = validate_transition(ty, &dir, "CreateWindow", prev, next);
        assert!(
            ok,
            "real CreateWindow {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );
        validated += 1;

        // --- CreateWindow: a 3rd window (a strictly-greater id; never reused).
        let prev = next;
        let sid2 = app.next_session_id;
        let _wid2 = app.insert_logical_window(stub_session(sid2), 24, 80);
        assert!(
            app.structural_invariants_ok(),
            "invariants hold after 2nd create"
        );
        let next = project(&app, false);
        let (ok, out) = validate_transition(ty, &dir, "CreateWindow", prev, next);
        assert!(
            ok,
            "real CreateWindow {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );
        validated += 1;

        // --- CloseWindow: a NON-last window (WindowId(0)). A survivor remains, so
        // the app stays and exited is unchanged (still false).
        let prev = next;
        let outcome = app.close_window_logical(WindowId(0));
        assert_eq!(
            outcome,
            CloseOutcome::Stay,
            "closing a non-last window keeps the app"
        );
        assert!(
            app.structural_invariants_ok(),
            "invariants hold after a non-last close"
        );
        let next = project(&app, false);
        let (ok, out) = validate_transition(ty, &dir, "CloseWindow", prev, next);
        assert!(
            ok,
            "real CloseWindow (non-last) {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );
        validated += 1;

        // --- CloseWindow: the FRONTMOST window (highest id) while a LOWER-id survivor
        // remains. The real app re-points frontmost to the LOWEST live WindowId; the
        // model admits this via its NONDETERMINISTIC re-point (`frontmost' \in
        // 1..next_id-1`), and ty validates that the real next.frontmost lands in that
        // range (FrontmostLive / FrontmostAllocated hold for every value in it). This
        // is a transition the lowest-id-first teardown below never reaches, so without
        // this step that branch of the spec was conformance-vacuous.
        if app.windows.len() >= 2 {
            let prev = project(&app, exited);
            let front = app
                .frontmost_window
                .expect("a frontmost window exists with >=2 windows");
            let outcome = app.close_window_logical(front);
            assert_eq!(
                outcome,
                CloseOutcome::Stay,
                "closing the frontmost with a survivor keeps the app"
            );
            assert!(
                app.structural_invariants_ok(),
                "invariants hold after a frontmost close"
            );
            let next = project(&app, exited);
            let (ok, out) = validate_transition(ty, &dir, "CloseWindow", prev, next);
            assert!(
                ok,
                "real CloseWindow (frontmost, survivor remains) {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
            );
            validated += 1;
        }

        // --- CloseWindow down to the LAST: keep closing the lowest live WindowId;
        // when `close_window_logical` reports `Exit`, the last window is gone — set
        // `exited = true` so the projection tracks the real exit. (Once the window
        // set is empty, `structural_invariants_ok` can no longer be called — there
        // is no frontmost — so it is asserted only after each create, not here.)
        while let Some(&wid) = app.windows.keys().next() {
            let prev = project(&app, exited);
            let outcome = app.close_window_logical(wid);
            if matches!(outcome, CloseOutcome::Exit) {
                exited = true;
            }
            let next = project(&app, exited);
            let (ok, out) = validate_transition(ty, &dir, "CloseWindow", prev, next);
            assert!(
                ok,
                "real CloseWindow {prev:?} -> {next:?} (outcome {outcome:?}) must conform\n--- ty ---\n{out}"
            );
            validated += 1;
        }
        assert!(app.windows.is_empty(), "every window closed");
        assert!(exited, "closing the last window set exited (ExitIffEmpty)");

        // NEGATIVE CONTROLS (non-vacuity) — each MUST be ty-REJECTED. If either is
        // accepted, the binding is meaningless: a corrupted close transition would
        // sail through and the conformance pass would prove nothing.
        //
        // (a) MISSED EXIT — a CloseWindow from a single live window (win_count=1) to
        // an empty set (win_count=0) that WRONGLY holds `exited=0`. This is exactly
        // the `Buggy=1` defect (no windows left but the app still running); the
        // committed model (`Buggy=0`) forbids it.
        let prev_missed = [1, 1, 2, 0];
        let next_missed = [0, 0, 2, 0]; // win_count 1->0 but exited stays 0
        let (ok, o) = validate_transition(ty, &dir, "CloseWindow", prev_missed, next_missed);
        assert!(
            !ok,
            "NEGATIVE CONTROL (missed exit) {prev_missed:?} -> {next_missed:?} MUST be rejected \
             — a CloseWindow to an empty set without exiting is the Buggy defect\n--- ty ---\n{o}"
        );

        // (b) EARLY EXIT — a CloseWindow that sets `exited=1` while a window still
        // remains (win_count>0). `ExitIffEmpty` forbids exiting with a live window.
        let prev_early = [2, 1, 3, 0];
        let next_early = [1, 1, 3, 1]; // win_count 2->1 (survivor) but exited flipped to 1
        let (ok, o) = validate_transition(ty, &dir, "CloseWindow", prev_early, next_early);
        assert!(
            !ok,
            "NEGATIVE CONTROL (early exit) {prev_early:?} -> {next_early:?} MUST be rejected \
             — exiting while a window remains violates ExitIffEmpty\n--- ty ---\n{o}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        eprintln!(
            "App window-routing Tier-1 conformance: {validated} real transitions \
             (2 CreateWindow + the close-down-to-exit chain) strictly validated against the \
             WindowRouting spec; negative controls (missed exit, early exit) both rejected."
        );
    }
}

/// Tier-1 trace conformance for the split-pane tree (`PaneTree`): bind the REAL
/// `pane::PaneTree` split/close mutators to the ty-proven `pane_tree_model()`. The
/// model is the SAME one Tier-0 exhaustively `ty check`s; here each real Split /
/// Close transition is projected to `<<leaf_count, focused>>` and `ty trace
/// validate`d against the derived `Next`, so a real re-point regression (a Close that
/// leaves `focused` past the shrunk end — the dangling-focus defect) fails this test.
///
/// VERIFICATION GATE (honesty ratchet, batteries-on): verification is always required
/// — an absent Trust `ty` FAILS the test with a build hint.
#[cfg(test)]
mod pane_tree_conformance {
    use super::*;
    use aterm_spec::derive::pane_tree_model;
    use aterm_spec::verify::ty;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;

    /// Project a real `PaneTree` onto the spec variables `[leaf_count, focused]`:
    /// `leaf_count` is the live leaf count; `focused` is the POSITION of the focused
    /// session id within `sessions()` (left-to-right tree order) — the renderer's
    /// 0-based pane index. `focus()` is always a live leaf, so `position` never fails.
    fn project(tree: &pane::PaneTree) -> [i64; 2] {
        let leaf_count = tree.len() as i64;
        let sessions = tree.sessions();
        let focused = sessions
            .iter()
            .position(|&s| s == tree.focus())
            .expect("focused session is always a live leaf") as i64;
        [leaf_count, focused]
    }

    /// Two-step `ty` trace (`prev` must match `Init`, then `next` under `action`).
    /// Module name `PaneTree` matches the derived spec's `---- MODULE PaneTree ----`.
    fn transition_trace(action_name: &str, prev: [i64; 2], next: [i64; 2]) -> String {
        let st = |s: [i64; 2]| {
            format!(
                "{{\"leaf_count\":{{\"type\":\"int\",\"value\":{}}},\
                 \"focused\":{{\"type\":\"int\",\"value\":{}}}}}",
                s[0], s[1]
            )
        };
        format!(
            "{{\"version\":\"1\",\"module\":\"PaneTree\",\
             \"variables\":[\"leaf_count\",\"focused\"],\"steps\":[\
             {{\"index\":0,\"state\":{}}},\
             {{\"index\":1,\"state\":{},\"action\":{{\"name\":\"{}\"}}}}\
             ]}}",
            st(prev),
            st(next),
            action_name
        )
    }

    /// Run `ty trace validate` for one real transition; returns (conforms, output).
    /// Spec + cfg are DERIVED from the SAME `pane_tree_model()` Tier-0 checks; `Cap`
    /// is overridden LARGE so a real split depth never spuriously trips the bound.
    fn validate_transition(
        ty: &Path,
        dir: &Path,
        action_name: &str,
        prev: [i64; 2],
        next: [i64; 2],
    ) -> (bool, String) {
        let m = pane_tree_model();
        let spec = dir.join("PaneTree.tla");
        let cfg = dir.join("PaneTree.cfg");
        let trace = dir.join("t.json");
        let init: BTreeMap<&'static str, i64> = [("leaf_count", prev[0]), ("focused", prev[1])]
            .into_iter()
            .collect();
        std::fs::write(&spec, m.transition_spec()).expect("write spec");
        std::fs::write(&cfg, m.transition_cfg(&init, &[("Cap", 1_000_000)])).expect("write cfg");
        std::fs::write(&trace, transition_trace(action_name, prev, next)).expect("write trace");
        let out = Command::new(ty)
            .arg("trace")
            .arg("validate")
            .arg(&trace)
            .arg("--spec")
            .arg(&spec)
            .arg("--config")
            .arg(&cfg)
            .output()
            .expect("run ty trace validate");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    }

    #[test]
    fn real_app_pane_tree_conforms() {
        let ty = ty("PaneTree split/close conformance");
        run_conformance(&ty);
    }

    /// The PaneTree Tier-1 body, factored out so `spec_xref_gate` can RUN it — so the
    /// gate's "pane_tree actively-bound" claim is backed by a real trace check, not a
    /// disconnected test: if the real split/close re-point logic diverges from
    /// `PaneTree.Next`, this fails and the gate fails with it.
    pub(crate) fn run_conformance(ty: &Path) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aterm-panetree-conf-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mk tempdir");

        // PROJECTION-DRIFT GUARD: a fresh single-pane tab projects to the model's Init.
        let t0 = pane::PaneTree::new(0);
        let init = pane_tree_model().init_state();
        let model_init = [init["leaf_count"], init["focused"]];
        assert_eq!(
            project(&t0),
            model_init,
            "fresh PaneTree projection must equal pane_tree_model().init_state() — got {:?}, model {:?}",
            project(&t0),
            model_init
        );
        assert_eq!(model_init, [1, 0], "sanity: model init is one focused leaf");

        // Drive a REAL split/close sequence and strictly validate each transition.
        let mut t = pane::PaneTree::new(0);

        // Split: split the lone focused leaf -> 2 leaves, focus the new (last) one.
        let prev = project(&t);
        assert!(t.split_focused(pane::SplitDir::Vertical, 1));
        let next = project(&t);
        let (ok, out) = validate_transition(ty, &dir, "Split", prev, next);
        assert!(
            ok,
            "real Split {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // Split again on the new focused pane -> 3 leaves.
        let prev = next;
        assert!(t.split_focused(pane::SplitDir::Horizontal, 2));
        let next = project(&t);
        let (ok, out) = validate_transition(ty, &dir, "Split", prev, next);
        assert!(
            ok,
            "real Split {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // Close a non-focused leaf (re-focus the first pane, close it): the parent
        // collapses into the sibling and focus re-seats on a surviving leaf, in range.
        assert!(t.set_focus(0));
        let prev = project(&t);
        assert!(matches!(
            t.close_pane(0),
            pane::CloseOutcome::Collapsed { .. }
        ));
        let next = project(&t);
        let (ok, out) = validate_transition(ty, &dir, "Close", prev, next);
        assert!(
            ok,
            "real Close (collapse) {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // Close the FOCUSED last leaf -> shrink to 1; focus re-points to the survivor.
        let prev = project(&t);
        assert!(matches!(
            t.close_focused(),
            pane::CloseOutcome::Collapsed { .. }
        ));
        let next = project(&t);
        let (ok, out) = validate_transition(ty, &dir, "Close", prev, next);
        assert!(
            ok,
            "real Close (focused last) {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // NON-VACUOUS NEGATIVE CONTROL: a Close that leaves `focused` past the shrunk
        // end (the dangling-focus defect) MUST be ty-REJECTED. From a 2-leaf tree,
        // Close admits only `focused' = 0` at `leaf_count' = 1`; the corrupted
        // `[1, 1]` is outside `Next`, so ty must reject it — else this check is vacuous.
        let (bad_ok, _bad) = validate_transition(ty, &dir, "Close", [2, 1], [1, 1]);
        assert!(
            !bad_ok,
            "corrupted Close [2,1] -> [1,1] (dangling focus) MUST be ty-REJECTED — \
             the conformance would be vacuous otherwise"
        );

        let _ = std::fs::remove_dir_all(&dir);
        eprintln!(
            "pane_tree Tier-1 conformance: real split/close transitions conform AND the \
             dangling-focus negative control was rejected by ty."
        );
    }
}

/// Tier-1 trace conformance for the session pool (`SessionPool`): bind the REAL
/// view-count accounting to the ty-proven `session_pool_model()`. NON-VACUOUS by
/// construction — `refcount` is projected from the INDEPENDENT real display count
/// (`windows_displaying`, recomputed from the live window/tab structures) and `closed`
/// from pool membership, so a pool that retires a session while a window still
/// displays it (premature close → use-after-free on the pooled `Session`) projects to
/// `[refcount>0, closed=1]`, which ty rejects.
///
/// VERIFICATION GATE (honesty ratchet, batteries-on): verification is always required
/// — an absent Trust `ty` FAILS the test with a build hint.
#[cfg(test)]
mod session_pool_conformance {
    use super::*;
    use aterm_spec::derive::session_pool_model;
    use aterm_spec::verify::ty;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;

    /// Project the pool state for session `sid` onto `[refcount, closed]` from TWO
    /// INDEPENDENT signals: `refcount` = the count of windows actually displaying the
    /// session (`windows_displaying`, from the live window/tab structures); `closed` =
    /// whether the pool has retired the entry (`views(sid).is_none()`). Their
    /// independence is exactly what makes `ClosedIffEmpty` non-vacuous at Tier-1 — a
    /// retire-while-still-displayed desync projects to `[>0, 1]`, outside `Next`.
    fn project(app: &App, sid: u64) -> [i64; 2] {
        let refcount = app.windows_displaying(sid).count() as i64;
        let closed = i64::from(app.pool.views(sid).is_none());
        [refcount, closed]
    }

    /// Two-step `ty` trace (`prev` must match `Init`, then `next` under `action`).
    /// Module name `SessionPool` matches the derived spec's `---- MODULE SessionPool ----`.
    fn transition_trace(action_name: &str, prev: [i64; 2], next: [i64; 2]) -> String {
        let st = |s: [i64; 2]| {
            format!(
                "{{\"refcount\":{{\"type\":\"int\",\"value\":{}}},\
                 \"closed\":{{\"type\":\"int\",\"value\":{}}}}}",
                s[0], s[1]
            )
        };
        format!(
            "{{\"version\":\"1\",\"module\":\"SessionPool\",\
             \"variables\":[\"refcount\",\"closed\"],\"steps\":[\
             {{\"index\":0,\"state\":{}}},\
             {{\"index\":1,\"state\":{},\"action\":{{\"name\":\"{}\"}}}}\
             ]}}",
            st(prev),
            st(next),
            action_name
        )
    }

    /// Run `ty trace validate` for one real transition; returns (conforms, output).
    /// Spec + cfg DERIVED from the SAME `session_pool_model()` Tier-0 checks; `Cap`
    /// is overridden LARGE so a real co-view fan-out never trips the model bound.
    fn validate_transition(
        ty: &Path,
        dir: &Path,
        action_name: &str,
        prev: [i64; 2],
        next: [i64; 2],
    ) -> (bool, String) {
        let m = session_pool_model();
        let spec = dir.join("SessionPool.tla");
        let cfg = dir.join("SessionPool.cfg");
        let trace = dir.join("t.json");
        let init: BTreeMap<&'static str, i64> = [("refcount", prev[0]), ("closed", prev[1])]
            .into_iter()
            .collect();
        std::fs::write(&spec, m.transition_spec()).expect("write spec");
        std::fs::write(&cfg, m.transition_cfg(&init, &[("Cap", 1_000_000)])).expect("write cfg");
        std::fs::write(&trace, transition_trace(action_name, prev, next)).expect("write trace");
        let out = Command::new(ty)
            .arg("trace")
            .arg("validate")
            .arg(&trace)
            .arg("--spec")
            .arg(&spec)
            .arg("--config")
            .arg(&cfg)
            .output()
            .expect("run ty trace validate");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    }

    #[test]
    fn real_app_session_pool_conforms() {
        let ty = ty("SessionPool refcount conformance");
        run_conformance(&ty);
    }

    /// The SessionPool Tier-1 body, factored out so `spec_xref_gate` can RUN it — so
    /// the gate's "session_pool actively-bound" claim is backed by a real trace check:
    /// if a real attach/detach diverges from `SessionPool.Next` (retires while a
    /// viewer remains, or leaks a fully-detached entry), this fails and the gate fails.
    pub(crate) fn run_conformance(ty: &Path) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aterm-sesspool-conf-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mk tempdir");

        // PROJECTION-DRIFT GUARD: a fresh headless App views session 0 in one window.
        let app0 = App::headless_for_test();
        let init = session_pool_model().init_state();
        let model_init = [init["refcount"], init["closed"]];
        assert_eq!(
            project(&app0, 0),
            model_init,
            "fresh App pool projection must equal session_pool_model().init_state() — got {:?}, model {:?}",
            project(&app0, 0),
            model_init
        );
        assert_eq!(
            model_init,
            [1, 0],
            "sanity: model init is one viewer, not closed"
        );
        drop(app0);

        let mut app = App::headless_for_test();

        // Acquire: open the active session in a SECOND window (views 1 -> 2, the
        // Cmd-Shift-O share). Both windows now display session 0.
        let prev = project(&app, 0);
        let new_wid = app
            .open_active_session_in_new_window_logical()
            .expect("the front window has an active session to open in a new window");
        let next = project(&app, 0);
        let (ok, out) = validate_transition(ty, &dir, "Acquire", prev, next);
        assert!(
            ok,
            "real Acquire {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // Release (non-retiring): close ONE viewer (views 2 -> 1, the session survives).
        let prev = next;
        assert_eq!(
            app.close_window_logical(new_wid),
            CloseOutcome::Stay,
            "closing one of two viewers keeps the app + session"
        );
        let next = project(&app, 0);
        let (ok, out) = validate_transition(ty, &dir, "Release", prev, next);
        assert!(
            ok,
            "real Release (survive) {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // Release (retiring): close the LAST viewer (views 1 -> 0). The pool retires
        // session 0 (drops the Session, closing its PTY); being the last window, exit.
        let prev = next;
        assert_eq!(
            app.close_window_logical(WindowId(0)),
            CloseOutcome::Exit,
            "closing the last viewer exits the app"
        );
        let next = project(&app, 0);
        let (ok, out) = validate_transition(ty, &dir, "Release", prev, next);
        assert!(
            ok,
            "real Release (retire) {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );
        assert_eq!(
            next,
            [0, 1],
            "after the last viewer leaves, session 0 is retired (refcount 0, closed)"
        );

        // NON-VACUOUS NEGATIVE CONTROL: a Release that RETIRES the session while a
        // viewer remains (premature close — the use-after-free hazard) MUST be
        // ty-REJECTED. From `[2,0]`, Release admits only `[1,0]`; the corrupted
        // `[1,1]` (entry gone, a window still displays it) is outside `Next`.
        let (bad_ok, _bad) = validate_transition(ty, &dir, "Release", [2, 0], [1, 1]);
        assert!(
            !bad_ok,
            "corrupted Release [2,0] -> [1,1] (retire-while-viewed) MUST be ty-REJECTED — \
             the conformance would be vacuous otherwise"
        );

        let _ = std::fs::remove_dir_all(&dir);
        eprintln!(
            "session_pool Tier-1 conformance: real open-in-new-window / close-window \
             attach/detach transitions conform AND the retire-while-viewed negative \
             control was rejected by ty."
        );
    }
}

/// Tier-1 trace conformance for the native tab strip (`TabStrip`): bind the REAL tab
/// mutators to the ty-proven `tab_strip_model()`, projecting BOTH the truth lane
/// `(count, active)` from `ws.tabs` AND the strip lane `(seg_count, selected)` from
/// `WindowState::strip_shadow` — the faithful record of what `refresh_window_tabs` last
/// pushed to the native `NSSegmentedControl`. This makes the conformance NON-VACUOUS in
/// a headless test (no real toolbar): a tab mutation that forgets to re-sync a window's
/// strip leaves the shadow stale, so the projection desyncs and `ty` rejects it. The
/// load-bearing case is closing a tab in a NON-FRONT window — `close_tab_at` must
/// re-sync THAT window (not just the front), or its strip keeps a phantom segment.
///
/// VERIFICATION GATE (honesty ratchet, batteries-on): verification is always required
/// — an absent Trust `ty` FAILS the test with a build hint.
#[cfg(test)]
mod tab_strip_conformance {
    use super::*;
    use aterm_spec::derive::tab_strip_model;
    use aterm_spec::verify::ty;
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::process::Command;

    /// Project window `wid` onto the spec variables `[count, active, seg_count,
    /// selected]` from TWO sources: the TRUTH lane `(count, active)` from `ws.tabs`
    /// (the real tab model) and the STRIP lane `(seg_count, selected)` from
    /// `ws.strip_shadow` (the last push to the native control). Their potential
    /// disagreement after a missed re-sync is exactly what `StripMirrorsTruth` forbids.
    pub(crate) fn project(app: &App, wid: WindowId) -> [i64; 4] {
        let Some(ws) = app.windows.get(&wid) else {
            return [0, 0, 0, 0];
        };
        let (seg_count, selected) = ws.strip_shadow.get();
        [
            ws.tabs.count as i64,
            ws.tabs.active as i64,
            seg_count as i64,
            selected as i64,
        ]
    }

    /// Two-step `ty` trace (`prev` must match `Init`, then `next` under `action`).
    /// Module name `TabStrip` matches the derived spec's `---- MODULE TabStrip ----`.
    fn transition_trace(action_name: &str, prev: [i64; 4], next: [i64; 4]) -> String {
        let st = |s: [i64; 4]| {
            format!(
                "{{\"count\":{{\"type\":\"int\",\"value\":{}}},\
                 \"active\":{{\"type\":\"int\",\"value\":{}}},\
                 \"seg_count\":{{\"type\":\"int\",\"value\":{}}},\
                 \"selected\":{{\"type\":\"int\",\"value\":{}}}}}",
                s[0], s[1], s[2], s[3]
            )
        };
        format!(
            "{{\"version\":\"1\",\"module\":\"TabStrip\",\
             \"variables\":[\"count\",\"active\",\"seg_count\",\"selected\"],\"steps\":[\
             {{\"index\":0,\"state\":{}}},\
             {{\"index\":1,\"state\":{},\"action\":{{\"name\":\"{}\"}}}}\
             ]}}",
            st(prev),
            st(next),
            action_name
        )
    }

    /// Run `ty trace validate` for one real transition; returns (conforms, output).
    /// Spec + cfg DERIVED from the SAME `tab_strip_model()` Tier-0 checks; `Cap`
    /// overridden LARGE so real tab depth never trips the model bound.
    fn validate_transition(
        ty: &Path,
        dir: &Path,
        action_name: &str,
        prev: [i64; 4],
        next: [i64; 4],
    ) -> (bool, String) {
        let m = tab_strip_model();
        let spec = dir.join("TabStrip.tla");
        let cfg = dir.join("TabStrip.cfg");
        let trace = dir.join("t.json");
        let init: BTreeMap<&'static str, i64> = [
            ("count", prev[0]),
            ("active", prev[1]),
            ("seg_count", prev[2]),
            ("selected", prev[3]),
        ]
        .into_iter()
        .collect();
        std::fs::write(&spec, m.transition_spec()).expect("write spec");
        std::fs::write(&cfg, m.transition_cfg(&init, &[("Cap", 1_000_000)])).expect("write cfg");
        std::fs::write(&trace, transition_trace(action_name, prev, next)).expect("write trace");
        let out = Command::new(ty)
            .arg("trace")
            .arg("validate")
            .arg(&trace)
            .arg("--spec")
            .arg(&spec)
            .arg("--config")
            .arg(&cfg)
            .output()
            .expect("run ty trace validate");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    }

    #[test]
    fn real_app_tab_strip_conforms() {
        let ty = ty("TabStrip parity conformance");
        run_conformance(&ty);
    }

    /// The TabStrip Tier-1 body, factored out so `spec_xref_gate` can RUN it — so the
    /// gate's "tab_strip actively-bound" claim is backed by a real trace check: if a
    /// tab mutation leaves a window's native strip stale (the desync `StripMirrorsTruth`
    /// forbids), this fails and the gate fails with it.
    pub(crate) fn run_conformance(ty: &Path) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NONCE: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aterm-tabstrip-conf-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("mk tempdir");

        // PROJECTION-DRIFT GUARD: a fresh window is one tab, selected, strip mirrored.
        let app0 = App::headless_for_test();
        let init = tab_strip_model().init_state();
        let model_init = [
            init["count"],
            init["active"],
            init["seg_count"],
            init["selected"],
        ];
        assert_eq!(
            project(&app0, WindowId(0)),
            model_init,
            "fresh window projection must equal tab_strip_model().init_state() — got {:?}, model {:?}",
            project(&app0, WindowId(0)),
            model_init
        );
        assert_eq!(
            model_init,
            [1, 0, 1, 0],
            "sanity: one tab, selected, strip mirrored"
        );
        drop(app0);

        let mut app = App::headless_for_test();

        // NewTab: append a 2nd tab to the FRONT window; refresh re-syncs the strip.
        // (push_stub_tab is the headless stand-in for open_tab_in — same layouts.push +
        // tabs.add + owner sync; open_tab_in carries the #[refines(NewTab)] anchor.)
        let prev = project(&app, WindowId(0));
        let sid1 = app.next_session_id;
        app.push_stub_tab(WindowId(0), stub_session(sid1));
        let next = project(&app, WindowId(0));
        let (ok, out) = validate_transition(ty, &dir, "NewTab", prev, next);
        assert!(
            ok,
            "real NewTab {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // SelectTab: cycle the front window's active tab (the deterministic wrap the
        // model's SelectTab encodes); sync re-syncs the strip selection in lockstep.
        let prev = next;
        app.cycle_tab(true);
        let next = project(&app, WindowId(0));
        let (ok, out) = validate_transition(ty, &dir, "SelectTab", prev, next);
        assert!(
            ok,
            "real SelectTab {prev:?} -> {next:?} must conform\n--- ty ---\n{out}"
        );

        // Close in a NON-FRONT window — THE load-bearing case. Add a 2nd window (now
        // frontmost), leaving WindowId(0) a non-front window with 2 tabs, then close one
        // of its tabs. `close_tab_at` must re-sync WindowId(0)'s OWN strip (the fix); a
        // front-only sync would leave its shadow stale and this transition would desync.
        let sid_b = app.next_session_id;
        let _b = app.insert_logical_window(stub_session(sid_b), 24, 80);
        assert_ne!(
            app.frontmost_window,
            Some(WindowId(0)),
            "WindowId(0) is now non-front"
        );
        let prev = project(&app, WindowId(0));
        assert!(
            !app.close_tab_at(WindowId(0), 1),
            "closing a non-last tab does not signal window-close"
        );
        let next = project(&app, WindowId(0));
        let (ok, out) = validate_transition(ty, &dir, "Close", prev, next);
        assert!(
            ok,
            "real Close in a NON-FRONT window {prev:?} -> {next:?} must conform — a stale strip \
             (seg_count past the new tab count) would FAIL here\n--- ty ---\n{out}"
        );
        assert_eq!(
            next[0], next[2],
            "after a non-front close, seg_count must mirror the tab count"
        );

        // NON-VACUOUS NEGATIVE CONTROL: a Close that shrinks the tab count but leaves
        // the strip stale (seg_count/selected unchanged — the missed-refresh desync)
        // MUST be ty-REJECTED. From `[2,1,2,1]`, Close admits only `[1,0,1,0]`; the
        // corrupted `[1,0,2,1]` (a phantom 2nd segment) is outside `Next`.
        let (bad_ok, _bad) = validate_transition(ty, &dir, "Close", [2, 1, 2, 1], [1, 0, 2, 1]);
        assert!(
            !bad_ok,
            "corrupted Close [2,1,2,1] -> [1,0,2,1] (stale strip) MUST be ty-REJECTED — \
             the conformance would be vacuous otherwise"
        );

        let _ = std::fs::remove_dir_all(&dir);
        eprintln!(
            "tab_strip Tier-1 conformance: real NewTab/SelectTab/Close (incl. a NON-FRONT \
             close) keep the native strip in lockstep with the tab model AND the stale-strip \
             negative control was rejected by ty."
        );
    }
}

/// Tier-1 trace conformance: bind the REAL control-socket image-path confinement to
/// the external `PathConfine.tla` design spec (TRUST_NATIVE_TLA Phase 2,
/// control-socket CONFINEMENT family).
///
/// `PathConfine.tla` is model-checked in the abstract by aterm-spec-models'
/// `model_check.rs` (Tier-0: `WriteWithinSubdir` / `EscapeRejected` — a committed
/// write only ever lands INSIDE the root, and a request resolving OUTSIDE is rejected
/// with no write; the symlink-escape bug fails at `Buggy=TRUE`). This test ties that
/// to the code that runs: it drives the genuine `control_auth::confine_image_path`
/// (the control-thread check) plus `snapshot_path::write_private_at` (the main-thread
/// writer) over BOTH an honest request and a planted final-component symlink that
/// re-points OUTSIDE the root, projects each onto the spec variables `<<linkOutside,
/// decided, committed, target>>`, and asks the real `ty` binary to confirm each
/// `Init -> Confine` transition is one the committed `PathConfine.tla`'s `Next`
/// admits.
///
/// METHOD — `Confine` is the spec's ONLY action and fires once from `Init`, so each
/// real transition IS the strict first transition `ty trace validate --spec` checks.
/// We therefore validate directly against the COMMITTED spec (no parameterized
/// variant). A NEGATIVE control (an escape that COMMITS to an OUTSIDE target — the
/// confused-deputy bug) MUST be ty-REJECTED, so a pass is never vacuous.
///
/// `ty` is located by the same fixed canonical path search; absent `ty` the test
/// FAILS (honesty ratchet, no skip path).
#[cfg(test)]
mod path_confine_conformance {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use aterm_spec::verify::ty;

    use crate::control_auth::confine_image_path;
    use crate::snapshot_path::write_private_at;

    // VERIFICATION GATE (honesty ratchet, batteries-on) in `aterm_spec::verify`:
    // verification is always required — an absent Trust `ty` FAILS the test with a
    // build hint (`cargo build --release -p tla-cli` in ~/trust/first-party/ty).

    fn spec_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("aterm-spec-models/specs")
            .join(name)
    }

    /// A two-step `ty` trace `[Init, Confine]`: step 0 (decided/committed FALSE,
    /// target "none") must match `Init`; step 1 (the projected outcome) must match
    /// `Confine`. `link_outside` selects the spec's `Init` `linkOutside`.
    fn confine_trace(link_outside: bool, committed: bool, target: &str) -> String {
        format!(
            "{{\"version\":\"1\",\"module\":\"PathConfine\",\
             \"variables\":[\"linkOutside\",\"decided\",\"committed\",\"target\"],\"steps\":[\
             {{\"index\":0,\"state\":{{\"linkOutside\":{{\"type\":\"bool\",\"value\":{lo}}},\
             \"decided\":{{\"type\":\"bool\",\"value\":false}},\
             \"committed\":{{\"type\":\"bool\",\"value\":false}},\
             \"target\":{{\"type\":\"string\",\"value\":\"none\"}}}}}},\
             {{\"index\":1,\"state\":{{\"linkOutside\":{{\"type\":\"bool\",\"value\":{lo}}},\
             \"decided\":{{\"type\":\"bool\",\"value\":true}},\
             \"committed\":{{\"type\":\"bool\",\"value\":{c}}},\
             \"target\":{{\"type\":\"string\",\"value\":\"{t}\"}}}},\"action\":{{\"name\":\"Confine\"}}}}\
             ]}}",
            lo = link_outside,
            c = committed,
            t = target,
        )
    }

    fn validate(
        ty: &Path,
        dir: &Path,
        link_outside: bool,
        committed: bool,
        target: &str,
    ) -> (bool, String) {
        let spec = spec_path("PathConfine.tla");
        let cfg = spec_path("PathConfine.cfg");
        let trace = dir.join("t.json");
        std::fs::write(&trace, confine_trace(link_outside, committed, target))
            .expect("write trace");
        let out = Command::new(ty)
            .arg("trace")
            .arg("validate")
            .arg(&trace)
            .arg("--spec")
            .arg(&spec)
            .arg("--config")
            .arg(&cfg)
            .output()
            .expect("run ty trace validate");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    }

    #[test]
    fn real_path_confine_conforms_to_pathconfine_spec() {
        let ty = ty("PathConfine conformance");
        let dir =
            std::env::temp_dir().join(format!("aterm-pathconfine-conf-{}", std::process::id()));
        let tydir =
            std::env::temp_dir().join(format!("aterm-pathconfine-ty-{}", std::process::id()));
        std::fs::create_dir_all(&tydir).expect("mk ty tempdir");
        // A fresh sock_dir with an images/ subdir (created 0700 by confine).
        std::fs::create_dir_all(&dir).expect("mk sock_dir");

        // --- HONEST request (linkOutside = FALSE): confine returns Some, the writer
        // COMMITS inside the root → committed=TRUE, target="inside".
        let confined = confine_image_path(&dir, "shot.png").expect("honest request must confine");
        write_private_at(&confined.dir, &confined.file_name, b"\x89PNG\r\n\x1a\nstub")
            .expect("write inside the root must succeed");
        // The committed path is inside the CANONICAL images root (on macOS `/tmp` is a
        // symlink to `/private/tmp`, so compare against the canonicalized dir).
        let canon_images = std::fs::canonicalize(dir.join("images")).expect("canon images");
        assert!(
            confined.display_path().starts_with(&canon_images),
            "committed path {:?} is inside the canonical images root {:?}",
            confined.display_path(),
            canon_images
        );
        let (ok, out) = validate(&ty, &tydir, false, true, "inside");
        assert!(
            ok,
            "real honest Confine (committed inside) must conform\n--- ty ---\n{out}"
        );

        // --- ESCAPE request (linkOutside = TRUE): plant a final-component symlink
        // images/evil.png -> a file OUTSIDE the root. confine MUST reject (None) →
        // committed=FALSE, target="none" (the symlink escape is never written).
        use std::os::unix::fs::symlink;
        let images = confined.dir.clone();
        let victim = dir.join("victim_outside.txt");
        std::fs::write(&victim, b"original").unwrap();
        let _ = std::fs::remove_file(images.join("evil.png"));
        symlink(&victim, images.join("evil.png")).unwrap();
        let escape = confine_image_path(&dir, "evil.png");
        assert!(
            escape.is_none(),
            "a final-component symlink escape MUST be rejected by confine"
        );
        // Project the rejection: no write committed, target none.
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"original",
            "the victim outside the root was NOT clobbered"
        );
        let (ok, out) = validate(&ty, &tydir, true, false, "none");
        assert!(
            ok,
            "real escape Confine (rejected, no write) must conform\n--- ty ---\n{out}"
        );

        // NEGATIVE CONTROL — the confused-deputy BUG: an escape request that COMMITS
        // to an OUTSIDE target (the pre-fix writer following the symlink).
        // `WriteWithinSubdir` / `EscapeRejected` forbid it; ty MUST reject.
        let (bad_ok, o) = validate(&ty, &tydir, true, true, "outside");
        assert!(
            !bad_ok,
            "NEGATIVE CONTROL (escape that commits OUTSIDE the root — the symlink-escape bug) \
             MUST be rejected\n--- ty ---\n{o}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&tydir);
        eprintln!(
            "PathConfine Tier-1 conformance: real honest (committed inside) + escape (planted \
             symlink, rejected, victim intact) Confine transitions strictly validated against \
             committed PathConfine.tla; outside-commit negative control rejected."
        );
    }
}

/// `spec_xref_closure` — the source↔spec cross-reference GATE (TRUST_NATIVE_TLA
/// §2.2), RELOCATED here in Phase 2 from aterm-core's `src/terminal/spec_xref_gate.rs`.
///
/// WHY HERE (the cross-crate collection fix). The ISOLATION-family anchors of Phase 2
/// live in aterm-sandbox / aterm-pty / aterm-gpu / aterm-core (and aterm-gui itself).
/// `inventory` only collects `submit!`s from object code LINKED into the running test
/// binary, so a gate that wants to SEE every machine's anchors must live in a test
/// build that links ALL the anchor-bearing crates. aterm-gui is that crate: it
/// depends (transitively) on aterm-core/sandbox/pty/gpu and hosts `path_confine`
/// itself, so this `#[cfg(test)]` module's binary links the whole set. Each
/// anchor-bearing dependency is pulled in with its `spec-anchors` feature ON via
/// aterm-gui's `[dev-dependencies]` (see Cargo.toml), so the otherwise-`feature`-gated
/// anchors expand into the linked libraries and `inventory` collects them here. The
/// aterm-core gate's collection was scoped to aterm-core's OWN unit-test build (only
/// `terminal_modes`); this gate's scope is the FULL ISOLATION + terminal_modes set.
///
/// WHAT IT ENFORCES (the obligations expressible aterm-local, per §2.2):
///   1. Action exists — every anchor names a real definition in its machine.
///   3. Coverage — every ACTIVELY-BOUND machine has every ACTION (the `Next`
///      disjuncts, for external `.tla`) bound-or-waived (`ratio == 1.0`); the rest
///      are REPORTED (`aterm_spec::xref::check_closure`).
///   4. Machine exists — every named `machine` resolves to a registered `SpecModule`
///      (embedded `Model` OR external `.tla` parsed from aterm-spec-models).
///
/// (Obligation 2 — symbol resolves to a live DefId — is Phase 3 / `trust-ir`.)
#[cfg(test)]
mod spec_xref_gate {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::process::Command;

    use aterm_spec::tla_check::TlaSpec;
    use aterm_spec::verify::{trust_ir, ty};
    use aterm_spec::xref::{self, SpecModule};

    // VERIFICATION GATE (honesty ratchet, batteries-on) in `aterm_spec::verify`, for
    // BOTH `ty` and `trust-ir`: verification is always required — an absent Trust
    // `ty`/`trust-ir` FAILS the test with a build hint (`cargo build --release -p
    // tla-cli` in ~/trust/first-party/ty). Phase-3's TRUST-native certification is
    // mandatory, never a silent degrade to the in-Rust closure alone.

    /// Run `trust-ir spec-link <module>` and return (success, combined-report). The
    /// report (stdout+stderr) is the per-machine coverage + any violation lines. When
    /// `manifest` is `Some`, also pass `--harness-manifest <m> --require-manifest` so
    /// L1 (proof_name resolution) is enforced — and is PROMOTED to a hard error if any
    /// proof binding is present but the manifest is missing (TRUST_VACUITY_GATE §2.1).
    fn run_spec_link(
        trust_ir: &std::path::Path,
        module: &std::path::Path,
        manifest: Option<&std::path::Path>,
    ) -> (bool, String) {
        let mut cmd = Command::new(trust_ir);
        cmd.arg("spec-link").arg(module);
        if let Some(m) = manifest {
            cmd.arg("--harness-manifest")
                .arg(m)
                .arg("--require-manifest");
        }
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("failed to run {trust_ir:?} spec-link: {e}"));
        let mut report = String::from_utf8_lossy(&out.stdout).into_owned();
        let err = String::from_utf8_lossy(&out.stderr);
        if !err.trim().is_empty() {
            report.push_str(&err);
        }
        (out.status.success(), report)
    }

    /// Generate the harness manifest the L1 resolution needs by invoking the always-run
    /// `xtask harness-manifest` node (TRUST_VACUITY_GATE §2.1 / finding 1a) — the SAME
    /// generator the build-graph node uses, so the gate and the xtask resolve proof
    /// names against an identical manifest. Returns the path to the written JSON.
    fn harness_manifest() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace root
            .expect("aterm-gui manifest dir has a workspace-root grandparent")
            .to_path_buf();
        let status = Command::new("cargo")
            .current_dir(&root)
            .arg("run")
            .arg("-q")
            .arg("-p")
            .arg("xtask")
            .arg("--")
            .arg("harness-manifest")
            .status()
            .expect("run `cargo run -p xtask -- harness-manifest`");
        assert!(status.success(), "xtask harness-manifest failed");
        let path = root
            .join("target")
            .join("trust")
            .join("harness-manifest.json");
        assert!(path.exists(), "xtask did not write {path:?}");
        path
    }

    /// The registered `SpecModule` set: every embedded `Model` plus every active
    /// external `.tla` (the ISOLATION family in aterm-spec-models's `specs/`; the
    /// `legacy/` quarantine is excluded).
    fn registered_modules() -> Vec<SpecModule> {
        let mut modules: Vec<SpecModule> = xref::model_registry()
            .into_iter()
            .map(SpecModule::Embedded)
            .collect();
        let dir = aterm_spec_models::specs_dir();
        let mut external = 0usize;
        for entry in std::fs::read_dir(&dir).expect("read aterm-spec-models specs/") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                continue; // skip legacy/
            }
            if path.extension().and_then(|e| e.to_str()) != Some("tla") {
                continue;
            }
            let spec = TlaSpec::parse_file(&path)
                .unwrap_or_else(|e| panic!("failed to parse external spec {path:?}: {e}"));
            modules.push(SpecModule::External(spec));
            external += 1;
        }
        assert!(
            external > 0,
            "no external ISOLATION .tla parsed from {dir:?} — Phase-1 quarantine wrong?"
        );
        modules
    }

    /// The six ISOLATION machines Phase 2 activates, with the action count the
    /// closure gate must find bound-or-waived (the `Next` disjuncts of each spec).
    /// `fork_exec` is anchored-but-not-Tier-1 (its child branch can't be driven
    /// in-process); the rest carry Tier-1 conformance in their own crates' tests.
    const ISOLATION: &[(&str, usize)] = &[
        ("sandbox", 1),      // Apply
        ("path_confine", 1), // Confine
        ("fork_exec", 6),    // Fork, Setrlimit, Chdir, CloseMaster, UnsafeEnvOp, Exec
        ("write_all", 2),    // Progress, Interrupted
        ("alt_screen", 4),   // WriteMain, Enter, Scribble, Leave
        ("gpu_encode", 2),   // Append, Encode
    ];

    #[test]
    fn spec_xref_closure() {
        // Honesty ratchet: bind a green gate to a real `ty` run of the machine the
        // Phase-0 anchors point at (terminal_modes), so green always means checked.
        let ty = ty("terminal_modes (spec_xref_closure)");

        // ---- Proof #1: inventory ACTUALLY collected the anchors, cross-crate. ----
        let refs: Vec<_> = xref::refinements().collect();
        let waivers: Vec<_> = xref::waivers().collect();

        // Per-machine collected counts (the cross-crate collection evidence).
        let machines: BTreeSet<&str> = refs
            .iter()
            .map(|r| r.machine)
            .chain(
                waivers
                    .iter()
                    .filter(|w| !w.machine.is_empty())
                    .map(|w| w.machine),
            )
            .collect();
        eprintln!(
            "spec_xref_closure (relocated to aterm-gui): collected {} refinement anchor(s) + \
             {} waiver(s) across {} machine(s):",
            refs.len(),
            waivers.len(),
            machines.len()
        );
        for m in &machines {
            let r = refs.iter().filter(|x| x.machine == *m).count();
            let w = waivers.iter().filter(|x| x.machine == *m).count();
            eprintln!("    {m:<16} refinements={r:<3} waivers={w}");
        }

        // Phase 0 NOT regressed: the terminal_modes anchors (from aterm-core, now
        // visible via its `spec-anchors` feature) must still be collected here.
        let tm = refs
            .iter()
            .filter(|r| r.machine == "terminal_modes")
            .count();
        assert!(
            tm >= 26,
            "expected >= 26 `terminal_modes` refinement anchors collected cross-crate from \
             aterm-core (Phase 0 must not regress), found {tm}"
        );

        // Phase 2: every ISOLATION machine's anchors collected here (cross-crate).
        for (machine, _) in ISOLATION {
            let r = refs.iter().filter(|x| x.machine == *machine).count();
            let w = waivers.iter().filter(|x| x.machine == *machine).count();
            assert!(
                r + w > 0,
                "ISOLATION machine `{machine}` collected ZERO anchors — the cross-crate \
                 `inventory` collection (spec-anchors feature) did not link. The gate would be \
                 vacuously green for it."
            );
        }

        // ---- Build the registered SpecModule set (embedded + external ISOLATION) ----
        let modules = registered_modules();
        eprintln!(
            "spec_xref_closure: {} registered SpecModule(s) ({} embedded + external ISOLATION)",
            modules.len(),
            xref::model_registry().len()
        );

        // ---- Run the obligations (1, 3, 4) ----
        let report = xref::check_closure(&modules);

        eprintln!("spec_xref_closure: per-machine coverage ledger:");
        for c in &report.coverage {
            eprintln!(
                "  {:<16} ratio={:.3} bound={:<3} waived={:<3} actions={:<3} {}{}",
                c.machine,
                c.ratio(),
                c.bound.len(),
                c.waived.len(),
                c.total_actions,
                if c.active {
                    "[ACTIVE]"
                } else {
                    "[report-only]"
                },
                if c.active && !c.uncovered.is_empty() {
                    format!(" UNCOVERED={:?}", c.uncovered)
                } else {
                    String::new()
                },
            );
        }

        // ---- Proof #2: GREEN (no obligation violations) ----
        assert!(
            report.is_ok(),
            "spec_xref_closure FAILED — source↔spec obligations violated:\n{}",
            report
                .violations
                .iter()
                .map(|v| format!("  [obligation {}] {}", v.obligation, v.message))
                .collect::<Vec<_>>()
                .join("\n")
        );

        // ---- Proof #3: every ISOLATION machine is ACTIVE and fully covered (==1.0) ----
        for (machine, want_actions) in ISOLATION {
            let c = report
                .coverage
                .iter()
                .find(|c| aterm_spec::xref::machine_matches(machine, &c.machine))
                .unwrap_or_else(|| {
                    panic!("ISOLATION machine `{machine}` not in the coverage ledger")
                });
            assert!(
                c.active,
                "ISOLATION machine `{machine}` must be ACTIVE (>= 1 refinement)"
            );
            assert_eq!(
                c.ratio(),
                1.0,
                "ISOLATION machine `{machine}` must be fully bound-or-waived (ratio == 1.0); \
                 uncovered = {:?}",
                c.uncovered
            );
            assert_eq!(
                c.total_actions, *want_actions,
                "ISOLATION machine `{machine}` expected {want_actions} actions (its Next \
                 disjuncts), found {}",
                c.total_actions
            );
        }

        // ---- Proof #3a (TRUST_VACUITY_GATE §2.3 / finding 3): window_routing is now
        // ACTIVELY-BOUND and the gate RUNS its Tier-1 conformance. ----
        // The `#[refines]` on the real `App` seams (`insert_logical_window` /
        // `close_window_logical`) make `window_routing` an active, coverage-gated
        // machine (was report-only). Assert that, THEN actually drive the real App
        // window-routing conformance from inside the gate — so the "window_routing
        // Tier-1 already green" claim is no longer a conflation of two disconnected
        // tests: if the real close→exit routing diverges from `WindowRouting.Next`,
        // `run_conformance` fails and this gate fails with it.
        {
            let wr = report
                .coverage
                .iter()
                .find(|c| aterm_spec::xref::machine_matches("window_routing", &c.machine))
                .expect("window_routing must be in the coverage ledger");
            assert!(
                wr.active,
                "window_routing must be ACTIVELY-BOUND (>= 1 refinement on the real App seams) — \
                 finding 3 requires it be gated, not report-only"
            );
            assert_eq!(
                wr.ratio(),
                1.0,
                "window_routing must be fully bound (CreateWindow + CloseWindow both anchored); \
                 uncovered = {:?}",
                wr.uncovered
            );
            assert_eq!(
                wr.total_actions, 2,
                "window_routing has 2 actions (CreateWindow, CloseWindow), found {}",
                wr.total_actions
            );
        }
        // RUN it (the gate now proves window_routing Tier-1, not merely claims it).
        super::window_routing_conformance::run_conformance(&ty);
        eprintln!(
            "spec_xref_closure: window_routing is actively-bound AND its Tier-1 conformance \
             (real App create/close→exit routing) was RUN by the gate (finding 3)."
        );

        // The `#[refines]` on the real `PaneTree::split_focused` / `close_pane` seams
        // make `pane_tree` an active, coverage-gated machine; assert that, THEN drive
        // the real split/close conformance from inside the gate — so a dangling-focus
        // re-point regression fails here, not just in a disconnected test.
        {
            let pt = report
                .coverage
                .iter()
                .find(|c| aterm_spec::xref::machine_matches("pane_tree", &c.machine))
                .expect("pane_tree must be in the coverage ledger");
            assert!(
                pt.active,
                "pane_tree must be ACTIVELY-BOUND (>= 1 refinement on the real PaneTree seams)"
            );
            assert_eq!(
                pt.ratio(),
                1.0,
                "pane_tree must be fully bound (Split + Close both anchored); uncovered = {:?}",
                pt.uncovered
            );
            assert_eq!(
                pt.total_actions, 2,
                "pane_tree has 2 actions (Split, Close), found {}",
                pt.total_actions
            );
        }
        super::pane_tree_conformance::run_conformance(&ty);
        eprintln!(
            "spec_xref_closure: pane_tree is actively-bound AND its Tier-1 conformance \
             (real split/close re-point) was RUN by the gate."
        );

        // The `#[refines]` on the real `SessionPool::attach` / `detach` seams make
        // `session_pool` an active, coverage-gated machine; assert that, THEN drive
        // the real open-in-new-window / close-window refcount conformance from inside
        // the gate — so a retire-while-still-viewed regression fails here.
        {
            let sp = report
                .coverage
                .iter()
                .find(|c| aterm_spec::xref::machine_matches("session_pool", &c.machine))
                .expect("session_pool must be in the coverage ledger");
            assert!(
                sp.active,
                "session_pool must be ACTIVELY-BOUND (>= 1 refinement on the real pool seams)"
            );
            assert_eq!(
                sp.ratio(),
                1.0,
                "session_pool must be fully bound (Acquire + Release both anchored); uncovered = {:?}",
                sp.uncovered
            );
            assert_eq!(
                sp.total_actions, 2,
                "session_pool has 2 actions (Acquire, Release), found {}",
                sp.total_actions
            );
        }
        super::session_pool_conformance::run_conformance(&ty);
        eprintln!(
            "spec_xref_closure: session_pool is actively-bound AND its Tier-1 conformance \
             (real attach/detach refcount accounting) was RUN by the gate."
        );

        // The `#[refines]` on the real `open_tab_in` / `cycle_tab` / `close_tab_at`
        // seams make `tab_strip` an active, coverage-gated machine; assert that, THEN
        // drive the real NewTab/SelectTab/Close conformance — incl. the non-front close
        // whose stale-strip desync `StripMirrorsTruth` forbids — from inside the gate.
        {
            let ts = report
                .coverage
                .iter()
                .find(|c| aterm_spec::xref::machine_matches("tab_strip", &c.machine))
                .expect("tab_strip must be in the coverage ledger");
            assert!(
                ts.active,
                "tab_strip must be ACTIVELY-BOUND (>= 1 refinement on the real tab seams)"
            );
            assert_eq!(
                ts.ratio(),
                1.0,
                "tab_strip must be fully bound (NewTab + SelectTab + Close all anchored); uncovered = {:?}",
                ts.uncovered
            );
            assert_eq!(
                ts.total_actions, 3,
                "tab_strip has 3 actions (NewTab, SelectTab, Close), found {}",
                ts.total_actions
            );
        }
        super::tab_strip_conformance::run_conformance(&ty);
        eprintln!(
            "spec_xref_closure: tab_strip is actively-bound AND its Tier-1 conformance \
             (native strip parity, incl. a non-front-window close) was RUN by the gate."
        );

        // ---- Proof #3b (Phase 4): the UNIFIED VERIFIER LEDGER over ty + kani. ----
        // Collect the `proof_anchor!`'d kani harnesses (cross-crate: scrollback ring/evict
        // + grid ring, linked here with `spec-anchors` ON) and emit ONE per-(machine,
        // action) ledger spanning the temporal (`ty`) and bounded-local (`kani`) verifiers.
        // `check_closure` (Proof #2 above) ALREADY asserted every proof anchor's
        // (machine, action) resolves (Ob.1 action-exists + Ob.4 machine-resolves) via the
        // SAME closure logic refinements use — a bogus proof_anchor action fails the gate.
        let proofs: Vec<_> = aterm_spec::xref::proof_anchors().collect();
        assert!(
            !proofs.is_empty(),
            "ZERO proof anchors collected — the cross-crate `inventory` collection of \
             `proof_anchor!`s (aterm-scrollback / aterm-grid with `spec-anchors` ON) did not \
             link. The unified verifier ledger would be vacuously ty-only."
        );
        let ledger = aterm_spec::xref::verifier_ledger(&modules);
        eprintln!(
            "spec_xref_closure: UNIFIED VERIFIER LEDGER (Phase 4) — {} proof anchor(s) over the \
             kani half, {} (machine, action) rows total:",
            proofs.len(),
            ledger.len()
        );
        // Print ONLY the rows discharged by at least one verifier (the rest are unbound
        // model actions, already reported in the coverage ledger above) so the ledger
        // reads as the cross-verifier picture, not a wall of `ty=–  kani=–`.
        for e in ledger.iter().filter(|e| e.ty || e.kani) {
            let detail = if e.proofs.is_empty() {
                String::new()
            } else {
                format!(
                    "    [{}]",
                    e.proofs.iter().cloned().collect::<Vec<_>>().join(", ")
                )
            };
            eprintln!("  {}{}", e.render(), detail);
        }
        // Non-vacuity: every collected proof anchor MUST land on a real ledger row with
        // `kani` set — i.e. the (machine, action) resolved and the ledger registered it.
        for p in &proofs {
            let row = ledger.iter().find(|e| {
                aterm_spec::xref::machine_matches(p.machine, &e.machine) && e.action == p.action
            });
            let row = row.unwrap_or_else(|| {
                panic!(
                    "proof anchor `{}` -> {}::{} did not resolve to a ledger row (Ob.1/Ob.4 \
                     should have already failed in Proof #2)",
                    p.proof_name, p.machine, p.action
                )
            });
            assert!(
                row.kani,
                "ledger row {}::{} must be kani-discharged (proof `{}` anchors it)",
                row.machine, row.action, p.proof_name
            );
        }
        // The ledger must show GENUINE cross-verifier coverage in ONE report: both a
        // `ty`-discharged action (the temporal/conformance half — terminal_modes + the 6
        // ISOLATION machines) AND a `kani`-discharged action (the bounded-local half —
        // the ring/eviction harnesses). They are deliberately DISJOINT here, which is the
        // design point (§4): kani proves local bounded properties, `ty` proves temporal
        // protocol properties — different obligations, JOINED by the same anchor namespace
        // into one ledger, not merged. (A single both-verifiers row is not forced: it would
        // require an artificial Ring `#[refines]` not present in shipping code.)
        assert!(
            ledger.iter().any(|e| e.ty),
            "no ty-discharged (machine, action) in the ledger — the temporal half is empty."
        );
        assert!(
            ledger.iter().any(|e| e.kani && !e.ty),
            "no kani-only (machine, action) — the bounded-local half (ring/eviction harnesses) \
             did not register, so the ledger would be ty-only. Ledger:\n{}",
            ledger
                .iter()
                .filter(|e| e.ty || e.kani)
                .map(|e| e.render())
                .collect::<Vec<_>>()
                .join("\n")
        );
        eprintln!(
            "spec_xref_closure: ledger non-vacuous — {} row(s) ty+kani, {} kani-only, {} ty-only.",
            ledger.iter().filter(|e| e.ty && e.kani).count(),
            ledger.iter().filter(|e| e.kani && !e.ty).count(),
            ledger.iter().filter(|e| e.ty && !e.kani).count(),
        );

        // ---- Proof #4 (non-vacuity / Tier-0 under the ARMED ty): every embedded
        // derived model is `ty check --strict-vacuity`ed — the now-STRICTER Trust
        // (TRUST_VACUITY_GATE §1.A) reports VACUOUS (exit 3) on an empty Init, a
        // never-fired ANCHORED action, or a vacuously-true invariant. The gate must be
        // GREEN under it. ----
        //
        // The ONE audited exception is the `Buggy`-variant negative-control models: a
        // model whose `Buggy`-guarded defect action (`Transact.BuggyCommit`) is
        // legitimately DEAD in the committed `Buggy=0` config — that action exists ONLY
        // to let ty PROVE the defect is excluded, and FIRES under `Buggy=1` (verified
        // by the conformance/model-check suites). For those, `--allow-vacuous=dead-action`
        // downgrades the dead-action verdict to an AUDITED warning (printed, exit 0) —
        // the spec is NOT vacuous, the dead action is the intended negative control.
        let dir = std::env::temp_dir().join(format!("aterm_spec_xref_gui_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mk tmp dir");
        // Models whose committed config has a deliberately-dead `Buggy`-guarded action
        // (the negative control); the rest must be strictly non-vacuous with NO relax.
        const DEAD_ACTION_NEG_CONTROLS: &[&str] = &["Transact"];
        for m in aterm_spec::xref::model_registry() {
            let tla = dir.join(format!("{}.tla", m.name));
            let cfg = dir.join(format!("{}.cfg", m.name));
            std::fs::write(&tla, m.to_tla()).expect("write tla");
            std::fs::write(&cfg, m.to_cfg()).expect("write cfg");
            let mut cmd = Command::new(&ty);
            cmd.arg("check")
                .arg(&tla)
                .arg("--config")
                .arg(&cfg)
                .arg("--strict-vacuity");
            let neg_control = DEAD_ACTION_NEG_CONTROLS.contains(&m.name);
            if neg_control {
                cmd.arg("--allow-vacuous=dead-action");
            }
            let out = cmd
                .output()
                .unwrap_or_else(|e| panic!("failed to run {ty:?}: {e}"));
            assert!(
                out.status.success(),
                "ty check --strict-vacuity FAILED (VACUOUS or property violation) for derived \
                 model `{}`{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                m.name,
                if neg_control {
                    " (even with --allow-vacuous=dead-action)"
                } else {
                    ""
                },
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!(
            "spec_xref_closure: all {} embedded models pass `ty check --strict-vacuity` (armed \
             Trust) — Transact's Buggy=0 dead `BuggyCommit` is the audited negative control \
             (--allow-vacuous=dead-action); every other model is strictly non-vacuous.",
            aterm_spec::xref::model_registry().len()
        );

        // ---- Proof #5 (Phase 3 + TRUST_VACUITY_GATE §2.1/§2.2): TRUST independently
        // certifies the SAME obligations PLUS the now-armed integrity teeth. Lower the
        // registered modules + collected anchors/waivers + PROOF anchors to a
        // byte-conforming `.trust_ir`, generate the harness manifest, then run
        // `trust-ir spec-link --harness-manifest … --require-manifest` and assert exit 0:
        //   * Ob.1/Ob.3/Ob.4 (unchanged);
        //   * L2 — every anchor on an actively-anchored machine now carries a non-empty
        //     `project` (the finding-2 fix; an inert `project=""` would fail here);
        //   * L1 — every lowered `proof` line's `proof_name` resolves against the
        //     manifest (the finding-1 fix; a typo'd proof name would fail here).
        let trust_ir = trust_ir("spec_xref_closure (Phase-3 Trust-native spec-link)");
        let module_txt =
            aterm_spec::ir::lower_to_ir("aterm_spec_xref", &modules, &refs, &waivers, &proofs);
        let lowered = aterm_spec::ir::lowered_machine_names(&modules, &refs);
        eprintln!(
            "spec_xref_closure: assembled .trust_ir — {} bytes, {} SpecModule block(s), {} \
             actively-lowered machine(s): {:?}; {} proof line(s) lowered",
            module_txt.len(),
            modules.len(),
            lowered.len(),
            lowered,
            proofs.len(),
        );

        let ir_dir = std::env::temp_dir().join(format!("aterm_spec_ir_{}", std::process::id()));
        std::fs::create_dir_all(&ir_dir).expect("mk ir tmp dir");
        let ir_path = ir_dir.join("aterm_spec_xref.trust_ir");
        std::fs::write(&ir_path, &module_txt).expect("write .trust_ir");

        // The harness manifest the L1 resolution needs (generated by the always-run
        // xtask node — the SAME generator the build-graph spec-link uses).
        let manifest = harness_manifest();
        let (ok, report) = run_spec_link(&trust_ir, &ir_path, Some(&manifest));
        eprintln!(
            "--- trust-ir spec-link report (REAL assembled module, L1+L2 armed) ---\n{report}"
        );
        assert!(
            ok,
            "trust-ir spec-link FAILED on the REAL assembled module — TRUST does NOT agree with \
             aterm's in-Rust closure (Ob.1/Ob.3/Ob.4 + L1 proof-name + L2 projection). \
             Report:\n{report}\n--- module ({} bytes) ---\n{module_txt}",
            module_txt.len(),
        );
        // L1 evidence: the report must show the manifest was consulted and every proof
        // binding resolved (not silently skipped).
        assert!(
            report.contains("harness manifest") && report.contains("proof binding"),
            "trust-ir report must show the harness manifest was consulted for L1; report:\n{report}"
        );
        // The TRUST report must reference each ISOLATION machine by its canonical
        // (CamelCase) name — proof the lowering canonicalized lower_snake anchors to
        // the SpecModule.name trust-ir resolves by exact match (Ob.4).
        for canon in [
            "TerminalModes",
            "Sandbox",
            "PathConfine",
            "ForkExec",
            "WriteAll",
            "AltScreen",
            "GpuEncode",
        ] {
            assert!(
                report.contains(canon),
                "trust-ir report should mention canonical machine `{canon}`; report:\n{report}"
            );
        }
        let _ = std::fs::remove_dir_all(&ir_dir);

        eprintln!(
            "spec_xref_closure: GREEN — obligations 1/3/4 hold for terminal_modes + 6 ISOLATION \
             machines; terminal_modes ty-checked (Tier-0); TRUST `trust-ir spec-link` \
             independently certified the SAME obligations on the lowered module (Phase 3)."
        );
    }

    /// PROOF THAT TRUST HAS TEETH, driven FROM ATERM (TRUST_NATIVE_TLA, Phase 3,
    /// item 4). The companion to `spec_xref_closure`'s Proof #5: that gate proves the
    /// REAL assembled module spec-links GREEN; this test proves `trust-ir spec-link`
    /// genuinely REJECTS a violating module — so the green is non-vacuous and TRUST is
    /// really enforcing, not rubber-stamping.
    ///
    /// We assemble a module with a deliberately BOGUS anchor (an action the machine
    /// does not declare), run `trust-ir spec-link`, and assert it EXITS 1 with the
    /// `[Ob.1]` (action-exists) violation — the same obligation aterm's in-Rust
    /// `check_closure` would flag. Then we assemble the same module WITHOUT the bogus
    /// anchor and confirm it spec-links exit 0, so the failure is attributable solely
    /// to the bad anchor (a controlled negative/positive pair).
    #[test]
    fn trust_ir_has_teeth() {
        use aterm_spec::xref::{RefinementAnchor, SpecModule};

        let trust_ir = trust_ir("trust_ir_has_teeth (Phase-3 negative control)");

        // A single embedded model (the ring) is enough to demonstrate the obligation.
        let modules = vec![SpecModule::Embedded(aterm_spec::derive::ring_model())];

        // A VALID anchor (Push exists in Ring) + a waiver covering the rest of the
        // model's actions, so the GOOD module is fully covered (Ob.3) and exit-0s.
        let good_anchor = RefinementAnchor {
            machine: "ring",
            action: "Push",
            rust_method: "aterm_buffer::Ring::push",
            location: "crates/aterm-buffer/src/ring.rs:1:1",
            project: "aterm_buffer::Ring::project",
        };

        // The BOGUS anchor: `MeltDown` is NOT an action of the Ring model. trust-ir's
        // Ob.1 (action-exists) must reject it.
        let bogus_anchor = RefinementAnchor {
            machine: "ring",
            action: "MeltDown",
            rust_method: "aterm_buffer::Ring::melt_down",
            location: "crates/aterm-buffer/src/ring.rs:9:9",
            project: "",
        };

        let dir = std::env::temp_dir().join(format!("aterm_spec_teeth_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mk teeth tmp dir");

        // ---- Negative: the BAD module must be REJECTED (exit 1) with [Ob.1] ----
        let bad_txt = aterm_spec::ir::lower_to_ir(
            "aterm_teeth_bad",
            &modules,
            &[&good_anchor, &bogus_anchor],
            &[],
            &[],
        );
        let bad_path = dir.join("bad.trust_ir");
        std::fs::write(&bad_path, &bad_txt).expect("write bad module");
        // Sanity: the lowering actually emitted the bogus anchor against canonical "Ring".
        assert!(
            bad_txt.contains("anchor machine \"Ring\" action \"MeltDown\""),
            "bogus anchor must be present in the lowered module:\n{bad_txt}"
        );

        let (bad_ok, bad_report) = run_spec_link(&trust_ir, &bad_path, None);
        eprintln!("--- trust-ir spec-link report (BOGUS module) ---\n{bad_report}");
        assert!(
            !bad_ok,
            "trust-ir spec-link must REJECT a module with an undeclared anchor action \
             (exit 1) — TRUST has no teeth otherwise. Report:\n{bad_report}"
        );
        assert!(
            bad_report.contains("[Ob.1") && bad_report.contains("MeltDown"),
            "trust-ir must report the [Ob.1 action-exists] violation naming `MeltDown`; \
             report:\n{bad_report}"
        );

        // ---- Positive: drop the bogus anchor -> the SAME module spec-links GREEN ----
        // Cover the remaining action (Ring has only Push) — already covered by the
        // good anchor, so no waiver needed; the good module is fully bound.
        let good_txt =
            aterm_spec::ir::lower_to_ir("aterm_teeth_good", &modules, &[&good_anchor], &[], &[]);
        let good_path = dir.join("good.trust_ir");
        std::fs::write(&good_path, &good_txt).expect("write good module");
        let (good_ok, good_report) = run_spec_link(&trust_ir, &good_path, None);
        eprintln!("--- trust-ir spec-link report (CONTROL good module) ---\n{good_report}");
        assert!(
            good_ok,
            "the same module WITHOUT the bogus anchor must spec-link exit 0 — so the \
             rejection is attributable solely to the undeclared action. Report:\n{good_report}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        eprintln!(
            "trust_ir_has_teeth: TRUST rejected the bogus anchor with [Ob.1] (exit 1) and \
             accepted the controlled good module (exit 0) — `trust-ir spec-link` genuinely \
             enforces the cross-reference."
        );
    }
}

/// SPLIT-PANE frame COMPOSITION (the GUI-level blit that builds one window frame
/// from several panes' snapshots). Pure `RenderInput` manipulation — no window,
/// PTY, or event loop — so the divider fill + per-pane blit geometry is testable
/// headlessly, like the pane-tree math in `pane.rs`.
#[cfg(test)]
mod compose_tests {
    use crate::app_render::{blit_pane_into, divider_cell, fill_divider_grid};
    use aterm_core::terminal::Terminal;
    use aterm_render::{RenderInput, Theme};

    /// A pane snapshot of `text` rendered into the top row of a `rows`x`cols` grid.
    fn pane_snapshot(text: &str, rows: usize, cols: usize) -> RenderInput {
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(text.as_bytes());
        let mut snap = RenderInput::empty();
        term.cell_frame_into(&mut snap, rows, cols);
        snap
    }

    /// `fill_divider_grid` produces a full `rows`x`cols` grid of identical seam
    /// cells, no cursor, single-width rows.
    #[test]
    fn divider_grid_is_uniform_seam() {
        let theme = Theme::default();
        let seam = divider_cell(theme);
        let mut dst = RenderInput::empty();
        fill_divider_grid(&mut dst, 3, 5, theme);
        assert_eq!(dst.rows, 3);
        assert_eq!(dst.cols, 5);
        assert_eq!(dst.cells.len(), 3);
        for row in &dst.cells {
            assert_eq!(row.len(), 5);
            assert!(
                row.iter().all(|c| *c == seam),
                "every cell is the seam colour"
            );
        }
        assert!(!dst.cursor_visible);
    }

    /// `blit_pane_into` places a pane's cells at the given offset and leaves the
    /// surrounding divider cells untouched — the 2x1 composite seam stays a seam.
    #[test]
    fn blit_places_pane_and_keeps_seam() {
        let theme = Theme::default();
        let seam = divider_cell(theme);
        // 1x5 window: [pane A (cols 0..2)] [divider col 2] [pane B (cols 3..5)].
        let mut dst = RenderInput::empty();
        fill_divider_grid(&mut dst, 1, 5, theme);
        let left = pane_snapshot("AB", 1, 2); // 'A','B'
        let right = pane_snapshot("CD", 1, 2); // 'C','D'
        blit_pane_into(&mut dst, &left, 0, 0);
        blit_pane_into(&mut dst, &right, 0, 3);
        let row = &dst.cells[0];
        assert_eq!(row[0].ch, 'A');
        assert_eq!(row[1].ch, 'B');
        assert_eq!(row[2], seam, "the divider column is left as a seam cell");
        assert_eq!(row[3].ch, 'C');
        assert_eq!(row[4].ch, 'D');
    }

    /// A blit that would overflow the composite (a degenerate too-small window) is
    /// bounds-checked: it writes only the cells that fit, never past the row.
    #[test]
    fn blit_is_bounds_checked() {
        let theme = Theme::default();
        let mut dst = RenderInput::empty();
        fill_divider_grid(&mut dst, 1, 3, theme);
        // A 1x5 pane blitted at col_off 1 into a 3-wide composite: only cols 1,2 fit.
        let wide = pane_snapshot("VWXYZ", 1, 5);
        blit_pane_into(&mut dst, &wide, 0, 1);
        assert_eq!(dst.cells[0].len(), 3, "the composite row is not grown");
        assert_eq!(dst.cells[0][1].ch, 'V');
        assert_eq!(dst.cells[0][2].ch, 'W');
    }
}

#[cfg(test)]
mod tab_strip_math_tests {
    use crate::app_render::{pixel_to_term_cell, prepend_strip_rows, strip_col_for_pixel};
    use crate::tab_bar;
    use aterm_core::terminal::Terminal;
    use aterm_render::RenderInput;

    /// With the strip DISABLED (`strip_rows == 0`), pixel→cell mapping is the exact
    /// pre-strip math: `y / ch` with no offset (the byte-identical path).
    #[test]
    fn pixel_to_cell_no_strip_is_unshifted() {
        // 8x16 cells, 24x80 grid, no strip, no pad.
        assert_eq!(pixel_to_term_cell(0.0, 0.0, 8, 16, 24, 80, 0, 0), (0, 0));
        // y = 32px → row 2 (32/16), x = 24px → col 3 (24/8).
        assert_eq!(pixel_to_term_cell(24.0, 32.0, 8, 16, 24, 80, 0, 0), (2, 3));
    }

    /// With a 1-row strip, the terminal region is shifted DOWN by one cell height:
    /// a pixel at the strip's bottom edge maps to terminal row 0, and a click deeper
    /// maps to the right terminal row (window row minus the strip).
    #[test]
    fn pixel_to_cell_with_strip_subtracts_offset() {
        let (cw, ch) = (8usize, 16usize);
        // y just past the 1-row strip (16px) → terminal row 0.
        assert_eq!(pixel_to_term_cell(0.0, 16.0, cw, ch, 24, 80, 1, 0), (0, 0));
        // y = 16 + 32 = 48px (window row 3) with a 1-row strip → terminal row 2.
        assert_eq!(pixel_to_term_cell(0.0, 48.0, cw, ch, 24, 80, 1, 0), (2, 0));
        // A 2-row strip: window row 5 (y=80px) → terminal row 3 (5 - 2).
        assert_eq!(pixel_to_term_cell(0.0, 80.0, cw, ch, 24, 80, 2, 0), (3, 0));
    }

    /// A pixel INSIDE the strip region clamps to terminal row 0 (the caller
    /// intercepts strip clicks via `strip_col_for_pixel` first, so this clamp is the
    /// safety net, not the routing).
    #[test]
    fn pixel_inside_strip_clamps_to_row_zero() {
        // y = 8px is inside the 1-row (16px) strip → terminal row 0.
        assert_eq!(pixel_to_term_cell(0.0, 8.0, 8, 16, 24, 80, 1, 0), (0, 0));
    }

    /// `strip_col_for_pixel`: a pixel in the strip's pixel band returns its column;
    /// a pixel below the band (terminal region) returns `None`.
    #[test]
    fn strip_col_hit_band() {
        let (cw, ch) = (8usize, 16usize);
        // y = 0 (inside the 1-row 16px strip), x = 24px → strip col 3.
        assert_eq!(strip_col_for_pixel(24.0, 0.0, cw, ch, 80, 1, 0), Some(3));
        // y = 16px (exactly at the strip's bottom = terminal region) → None.
        assert_eq!(strip_col_for_pixel(0.0, 16.0, cw, ch, 80, 1, 0), None);
        // A 2-row strip: y = 20px is still inside (< 32px) → Some.
        assert_eq!(strip_col_for_pixel(0.0, 20.0, cw, ch, 80, 2, 0), Some(0));
        // The column clamps to the last grid column.
        assert_eq!(
            strip_col_for_pixel(10_000.0, 0.0, cw, ch, 80, 1, 0),
            Some(79)
        );
    }

    /// PADDING composes with the strip: the interior `pad` border around the whole
    /// window is stripped from BOTH axes BEFORE the strip-row offset, so a click in
    /// the top/left pad band clamps to the strip / row 0, and a terminal click lands
    /// on the right cell once the pad is removed. This is the tab-strip ⊗ padding
    /// merge: the inset wraps the strip too.
    #[test]
    fn pad_composes_with_strip() {
        let (cw, ch, pad) = (8usize, 16usize, 8usize);
        // 1-row strip + 8px pad. The strip band is [pad, pad + ch) = [8, 24) in y.
        // y = 0 (top pad, over the strip) → strip column (gx = x - pad).
        assert_eq!(
            strip_col_for_pixel(8.0 + 24.0, 0.0, cw, ch, 80, 1, pad),
            Some(3)
        );
        // y = pad (top of the strip), x = pad → strip col 0.
        assert_eq!(strip_col_for_pixel(8.0, 8.0, cw, ch, 80, 1, pad), Some(0));
        // y = pad + ch (strip bottom = terminal region begins) → None.
        assert_eq!(strip_col_for_pixel(0.0, 24.0, cw, ch, 80, 1, pad), None);
        // Terminal cell mapping: the FIRST terminal cell (window row under the
        // strip) sits at y = pad + strip*ch = 8 + 16 = 24; x = pad + 0 = 8 → (0, 0).
        assert_eq!(
            pixel_to_term_cell(8.0, 24.0, cw, ch, 24, 80, 1, pad),
            (0, 0)
        );
        // One cell deeper + 3 cols right: y = 24 + ch = 40, x = pad + 3*cw = 32.
        assert_eq!(
            pixel_to_term_cell(32.0, 40.0, cw, ch, 24, 80, 1, pad),
            (1, 3)
        );
        // A click in the top-left pad corner clamps to the strip col 0 (not None).
        assert_eq!(strip_col_for_pixel(0.0, 0.0, cw, ch, 80, 1, pad), Some(0));
    }

    /// `prepend_strip_rows` shifts a terminal frame DOWN by the strip rows: the
    /// terminal content lands `strip` rows lower, the cursor moves down with it, the
    /// row count grows, and every per-row vector stays aligned (cells/clusters/
    /// combining/images/line_sizes all gain `strip` leading rows).
    #[test]
    fn prepend_shifts_content_and_cursor_down() {
        // A 2x4 terminal frame with a cursor at row 1.
        let mut term = Terminal::new(2, 4);
        term.process(b"AB\r\nCD");
        let mut frame = RenderInput::empty();
        term.cell_frame_into(&mut frame, 2, 4);
        let before_rows = frame.rows;
        let before_cursor = frame.cursor_row;
        // Build one strip row and splice it on top.
        let theme = aterm_render::Theme::default();
        let strip_row = vec![tab_bar::blank_cell(theme); 4];
        prepend_strip_rows(&mut frame, vec![strip_row]);
        // The frame grew by one row, the terminal content shifted down by one, and
        // the cursor followed.
        assert_eq!(frame.rows, before_rows + 1);
        assert_eq!(frame.cells.len(), before_rows + 1);
        assert_eq!(frame.cursor_row, before_cursor + 1);
        // Row 0 is now the strip; the original first terminal row ('A','B') is at row 1.
        assert_eq!(frame.cells[1][0].ch, 'A');
        assert_eq!(frame.cells[1][1].ch, 'B');
        assert_eq!(frame.cells[2][0].ch, 'C');
        // Per-row vectors stayed aligned with `cells` (one leading row each).
        assert_eq!(frame.clusters.len(), frame.cells.len());
        assert_eq!(frame.combining.len(), frame.cells.len());
        assert_eq!(frame.images.len(), frame.cells.len());
        assert_eq!(frame.line_sizes.len(), frame.cells.len());
    }

    /// End-to-end composition (as `splice_tab_strip` does it): lay out the strip,
    /// paint it, and splice it above a terminal frame. The composed frame is
    /// `terminal_rows + 1` tall, the strip row carries a tab title + the `+`, and the
    /// terminal content + cursor sit one row lower.
    #[test]
    fn end_to_end_strip_above_terminal() {
        let theme = aterm_render::Theme::default();
        let cols = 40usize;
        // Terminal frame: 3x40 with some text + a cursor.
        let mut term = Terminal::new(3, cols as u16);
        term.process(b"prompt$ ");
        let mut frame = RenderInput::empty();
        term.cell_frame_into(&mut frame, 3, cols);
        let term_cursor_row = frame.cursor_row;
        // Build the strip exactly like splice_tab_strip: 2 tabs, tab 0 active.
        let segments = tab_bar::layout_segments(cols as u16, 2, 0);
        let titles = vec!["zsh".to_string(), "vim".to_string()];
        let mut strip_row = vec![tab_bar::blank_cell(theme); cols];
        tab_bar::paint_strip(&mut strip_row, &segments, &titles, 0, theme);
        prepend_strip_rows(&mut frame, vec![strip_row]);
        // The composed frame is one row taller; the strip is row 0.
        assert_eq!(frame.rows, 4);
        // The active tab's title 'z','s','h' appears in the strip row.
        let t0 = &segments[0];
        let ts = (t0.start_col + 1) as usize;
        assert_eq!(frame.cells[0][ts].ch, 'z');
        // The `+` affordance is present in the strip row.
        let plus = segments.last().unwrap();
        assert_eq!(frame.cells[0][(plus.start_col + 1) as usize].ch, '+');
        // The terminal content shifted down to row 1, and the cursor followed.
        assert_eq!(frame.cells[1][0].ch, 'p'); // "prompt$ "
        assert_eq!(frame.cursor_row, term_cursor_row + 1);
    }

    /// An EMPTY strip (`strip_rows == 0` → no rows to prepend) is a no-op: the frame
    /// is byte-identical (the no-regression contract for `tab_strip_rows == 0`).
    #[test]
    fn prepend_empty_is_noop() {
        let mut term = Terminal::new(2, 4);
        term.process(b"AB\r\nCD");
        let mut frame = RenderInput::empty();
        term.cell_frame_into(&mut frame, 2, 4);
        let snapshot = frame.cells.clone();
        let rows = frame.rows;
        let cursor = frame.cursor_row;
        prepend_strip_rows(&mut frame, Vec::new());
        assert_eq!(frame.cells, snapshot, "no strip → grid unchanged");
        assert_eq!(frame.rows, rows);
        assert_eq!(frame.cursor_row, cursor);
    }
}

#[cfg(test)]
mod recursion_provision_tests {
    use crate::spawn::{
        install_parent_edges, is_valid_session_id, parse_injected_identity,
        provision_child_recursion_env,
    };
    use aterm_session::{EdgeTable, LaunchNonce, Op, SessionId};

    /// A generated session id is accepted; malformed shapes are rejected.
    #[test]
    fn session_id_shape_validation() {
        assert!(is_valid_session_id(SessionId::generate().as_str()));
        assert!(!is_valid_session_id("nope"));
        assert!(!is_valid_session_id("s-xyz"));
        assert!(!is_valid_session_id("s-")); // no hex
        assert!(!is_valid_session_id(&format!("s-{}", "g".repeat(20)))); // non-hex
    }

    /// Identity adoption is FAIL-CLOSED: both a valid id and a parseable nonce are
    /// required; any partial/garbled input yields None (→ fresh identity).
    #[test]
    fn adopt_injected_identity_roundtrips_and_fails_closed() {
        let sid = SessionId::generate();
        let nonce = LaunchNonce::generate();
        let got = parse_injected_identity(Some(sid.as_str()), Some(&nonce.to_hex()))
            .expect("valid pair adopts");
        assert_eq!(got.0.as_str(), sid.as_str());
        assert!(got.1.ct_eq(&nonce));
        // Partial / malformed → None.
        assert!(parse_injected_identity(Some(sid.as_str()), None).is_none());
        assert!(parse_injected_identity(None, Some(&nonce.to_hex())).is_none());
        assert!(parse_injected_identity(Some("bad"), Some(&nonce.to_hex())).is_none());
        assert!(parse_injected_identity(Some(sid.as_str()), Some("xyz")).is_none());
    }

    /// THE 4↔5 seam: the env a parent injects, reconstructed into the child's
    /// table, authorizes the parent's retained tokens for the EXACT op — read,
    /// write AND signal (or recursion is silently read-only). Wrong nonce / wrong
    /// op fail closed.
    #[test]
    fn provision_child_env_is_symmetric_across_all_three_ops() {
        let parent = SessionId::generate();
        let (env, prov) = provision_child_recursion_env(&parent);
        let get = |k: &str| env.iter().find(|(ek, _)| ek == k).map(|(_, v)| v.clone());

        // The child adopts the injected identity and installs the parent's edges.
        // The edge-token SECRETS are no longer in env (F1) — they travel via the
        // 0600 file / ProxyEntry; the symmetry property is over `prov`'s tokens.
        let (child_sid, child_nonce) = parse_injected_identity(
            get("ATERM_SESSION_ID").as_deref(),
            get("ATERM_LAUNCH_NONCE").as_deref(),
        )
        .expect("child adopts injected identity");
        assert!(
            get("ATERM_EDGE_READ").is_none(),
            "edge secrets must NOT be in the identity env (F1)"
        );
        let mut table = EdgeTable::new();
        let n = install_parent_edges(
            &mut table,
            &child_sid,
            &child_nonce,
            get("ATERM_PARENT_SESSION_ID").as_deref(),
            Some(&prov.read.to_hex()),
            Some(&prov.write.to_hex()),
            Some(&prov.signal.to_hex()),
        );
        assert_eq!(n, 3, "all three op edges recorded");

        // The parent's retained tokens authorize against the child's table, each
        // for its OWN op.
        assert_eq!(
            table.authorize(&prov.read, &child_sid, &child_nonce),
            Some(Op::ReadScreen)
        );
        assert_eq!(
            table.authorize(&prov.write, &child_sid, &child_nonce),
            Some(Op::WriteInput)
        );
        assert_eq!(
            table.authorize(&prov.signal, &child_sid, &child_nonce),
            Some(Op::Signal)
        );

        // A DIFFERENT (re-launch) nonce fails every edge closed.
        let stale = LaunchNonce::generate();
        assert_eq!(table.authorize(&prov.read, &child_sid, &stale), None);
        // A token never minted does not authorize.
        assert_eq!(
            table.authorize(
                &aterm_session::EdgeToken::generate(),
                &child_sid,
                &child_nonce
            ),
            None,
        );
    }

    /// A missing parent id records nothing (no half-provisioned authority).
    #[test]
    fn install_parent_edges_requires_parent_id() {
        let mut table = EdgeTable::new();
        let n = install_parent_edges(
            &mut table,
            &SessionId::generate(),
            &LaunchNonce::generate(),
            None,
            Some(&aterm_session::EdgeToken::generate().to_hex()),
            None,
            None,
        );
        assert_eq!(n, 0);
    }
}
