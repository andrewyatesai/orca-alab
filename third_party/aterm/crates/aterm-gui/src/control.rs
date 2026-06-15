// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Live introspection CONTROL SOCKET (aterm introspection control protocol v1).
//!
//! A background thread binds a Unix domain socket and serves newline-delimited
//! text requests so an out-of-process intelligence can read the live screen
//! (text/cursor/cell/search), drive the shell (send/key), snapshot the pixels
//! (image), drive text selection (select — plain ranges plus the gesture
//! forms `word`/`line`/`block`/`extend` — and selection/copy), and resize the
//! engine + PTY — all against the SAME running terminal the window presents.
//! This is aterm introspecting itself: no OS screen recording, just the
//! engine's own grid and renderer.
//!
//! Threading: text/cursor/cell/search read the shared [`Terminal`] directly;
//! send/key/resize poke the PTY master fd. The `image` verb needs the renderer,
//! which lives on the MAIN thread, so this thread cannot render. Instead it
//! pushes an [`ImageReq`] onto a shared queue, wakes the event loop with
//! [`Wake::Control`], and blocks on the reply channel; the main thread drains
//! the queue, renders, writes the PNG, and replies with the frame dimensions.
//!
//! `image` (and the SIGUSR1 snapshot) is WYSIWYG: it renders the exact pixels
//! the window shows, INCLUDING the current cursor blink phase and the hollow
//! unfocused-cursor override — so a focused blinking session may legitimately
//! capture a frame with no cursor pixels. Headless sessions pin the blink
//! phase on (always deterministic). For deterministic cursor state regardless
//! of phase, use the `cursor` verb (row, col, visible, style).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use aterm_containment::log_denial;
use aterm_core::grid::{CellFlags, Grid, MAX_GRID_COLS, MAX_GRID_ROWS};
use aterm_core::selection::{SelectionSide, SelectionType, SmartSelection};
use aterm_core::terminal::{CursorStyle, Terminal};
use winit::event_loop::EventLoopProxy;

use crate::control_auth::{self, AuthOutcome};
use crate::{Wake, term_lock};

/// The containment subsystem name used in audit denials from this socket.
const AUDIT_SUBSYSTEM: &str = "control_socket";

/// A request for the MAIN thread to render the live screen to a PNG.
///
/// The control thread fills the CONFINED target (a canonical dir + single
/// filename, already validated by `confine_image_path`), sends [`Wake::Control`],
/// then blocks on `reply`; the main thread renders and sends back
/// `(width, height)`. TOCTOU-1: passing the dir + filename (not a re-resolvable
/// path string) lets the writer `openat` the final component under a dir fd, so
/// no intermediate path component can be symlink-swapped after the check.
pub struct ImageReq {
    /// The confined image target (canonical `images/` dir + validated filename).
    pub target: control_auth::ConfinedImage,
    /// Channel the main thread replies on with the rendered `(width, height)`.
    pub reply: Sender<(u32, u32)>,
}

/// Shared queue of pending [`ImageReq`]s, drained by the main thread.
pub type ImageQueue = Arc<Mutex<VecDeque<ImageReq>>>;

/// Spawn the control-listener thread: provision the capability token, bind
/// the plan's socket (sweeping crashed instances' stale files first), lock it
/// to `0600`, publish the `latest` symlink, then accept connections and serve
/// the protocol on each AUTHENTICATED connection.
///
/// Access control is DEFAULT-ON (see [`control_auth`](crate::control_auth)):
/// the socket lives in a per-user `0700` directory, every accepted peer must be
/// the same uid, and every connection must present the per-launch token before
/// any verb runs. If the token cannot be provisioned we FAIL CLOSED — the
/// socket is never bound, so it cannot be driven without auth.
pub fn spawn(
    term: Arc<Mutex<Terminal>>,
    master: i32,
    proxy: EventLoopProxy<Wake>,
    queue: ImageQueue,
    plan: control_auth::SocketPlan,
    cell_size: (u32, u32),
) {
    std::thread::spawn(move || {
        let sock_path = plan.sock_path.clone();
        // The token + image-confinement subdir live alongside the socket file.
        let sock_dir = control_auth::dir_of_socket(&sock_path);
        if control_auth::ensure_private_dir(&sock_dir).is_err() {
            eprintln!(
                "aterm-gui: control socket dir {} not creatable; socket disabled",
                sock_dir.display()
            );
            return;
        }
        // Crashed instances cannot clean up after themselves: sweep dead pids'
        // per-instance socket/token leftovers. Only in the shared default dir —
        // an explicit override path owns its directory.
        if plan.latest_link.is_some() {
            control_auth::sweep_stale_instances(&sock_dir);
        }
        // Provision the per-launch capability token. FAIL CLOSED: no token =>
        // no socket (better to lose introspection than serve it unauthed).
        let token = match control_auth::provision_token(&plan.token_path) {
            Some(t) => Arc::new(t),
            None => {
                eprintln!(
                    "aterm-gui: could not provision control-socket token; socket disabled"
                );
                return;
            }
        };

        // A stale socket file from a prior run makes bind() fail with EADDRINUSE.
        let _ = std::fs::remove_file(&sock_path);
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("aterm-gui: control socket bind failed at {sock_path}: {e}");
                return;
            }
        };
        // Lock the socket file itself to 0600 (owner-only connect).
        control_auth::lock_socket_file(&sock_path);
        // Newest instance wins the `latest` symlink, atomically — flagless
        // single-instance `aterm-ctl` keeps working unchanged.
        if let Some(link) = &plan.latest_link {
            control_auth::publish_latest_link(link, &sock_path);
        }
        eprintln!("aterm-gui: control socket listening at {sock_path} (token-gated, same-uid only)");
        let our_uid = control_auth::our_uid();
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            // Peer credential gate: refuse any connection NOT from our own uid
            // before spending a thread on it. `None` (cannot verify) also
            // refuses — fail closed.
            match control_auth::peer_uid(&stream) {
                Some(uid) if uid == our_uid => {}
                other => {
                    log_denial(
                        AUDIT_SUBSYSTEM,
                        &format!("connect (peer uid {other:?} != {our_uid})"),
                        aterm_containment::mode_or_containment(),
                        "peer uid mismatch",
                    );
                    // Dropping `stream` closes the connection.
                    continue;
                }
            }
            // One thread per connection so a client that holds the socket open
            // (serving many commands) never blocks other clients.
            let term = term.clone();
            let proxy = proxy.clone();
            let queue = queue.clone();
            let token = token.clone();
            let sock_dir = sock_dir.clone();
            std::thread::spawn(move || {
                serve(stream, &term, master, &proxy, &queue, cell_size, &token, &sock_dir);
            });
        }
    });
}

/// Serve one connection: AUTHENTICATE the first line against the capability
/// token, then read newline-delimited requests and write one response each,
/// until the client disconnects or a write fails (dead client).
///
/// The peer's uid was already verified in [`spawn`]; here we require the token.
/// The first line MUST be `AUTH <hex>` or `TOKEN <hex> <verb...>`; anything else
/// gets `ERR auth\n` and the connection is closed BEFORE any verb executes.
#[allow(clippy::too_many_arguments)]
fn serve(
    stream: UnixStream,
    term: &Arc<Mutex<Terminal>>,
    master: i32,
    proxy: &EventLoopProxy<Wake>,
    queue: &ImageQueue,
    cell_size: (u32, u32),
    token: &str,
    sock_dir: &std::path::Path,
) {
    let reader = match stream.try_clone() {
        Ok(s) => BufReader::new(s),
        Err(_) => return,
    };
    let mut writer = stream;
    let mut lines = reader.lines();

    // First line is the auth handshake. A `TOKEN <hex> <verb...>` form folds the
    // first verb in, so we may have a verb to dispatch immediately.
    let first = match lines.next() {
        Some(Ok(l)) => l,
        _ => return, // client hung up before authenticating
    };
    let inline_verb = match control_auth::check_auth_line(&first, token) {
        AuthOutcome::Ok(verb) => verb,
        AuthOutcome::Denied => {
            log_denial(
                AUDIT_SUBSYSTEM,
                "auth",
                aterm_containment::mode_or_containment(),
                "missing or invalid capability token",
            );
            let _ = writer.write_all(b"ERR auth\n");
            let _ = writer.flush();
            return;
        }
    };

    // A folded-in verb runs first (empty tail = bare TOKEN line, just an ack).
    if let Some(verb) = inline_verb {
        if !verb.is_empty() {
            let resp = handle(&verb, term, master, proxy, queue, cell_size, sock_dir);
            if writer.write_all(resp.as_bytes()).is_err() {
                return;
            }
            let _ = writer.flush();
        }
    }

    for line in lines {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let resp = handle(&line, term, master, proxy, queue, cell_size, sock_dir);
        // A dead client (broken pipe) must not crash the app — just drop it.
        if writer.write_all(resp.as_bytes()).is_err() {
            break;
        }
        let _ = writer.flush();
    }
}

/// Dispatch a single request line to its handler, returning the full response
/// (including any trailing data rows) as a string.
#[allow(clippy::too_many_arguments)]
fn handle(
    line: &str,
    term: &Arc<Mutex<Terminal>>,
    master: i32,
    proxy: &EventLoopProxy<Wake>,
    queue: &ImageQueue,
    cell_size: (u32, u32),
    sock_dir: &std::path::Path,
) -> String {
    // Tolerate CRLF clients; the protocol itself is bare-LF terminated.
    let line = line.strip_suffix('\r').unwrap_or(line);
    let (verb, rest) = match line.split_once(' ') {
        Some((v, r)) => (v, r),
        None => (line, ""),
    };
    match verb {
        "text" => cmd_text(term),
        "cursor" => cmd_cursor(term),
        "cell" => cmd_cell(term, rest),
        "search" => cmd_search(term, rest),
        "send" => cmd_send(master, rest),
        "key" => cmd_key(term, master, rest),
        "ctrl" => cmd_ctrl(term, master, rest),
        "feed" => cmd_feed(master, rest),
        "signal" => cmd_signal(master, rest),
        "mouse" => cmd_mouse(term, master, rest),
        "paste" => cmd_paste(term, master, rest),
        "image" => cmd_image(proxy, queue, rest, sock_dir),
        "resize" => cmd_resize(proxy, rest),
        "scroll" => cmd_scroll(term, proxy, rest),
        "dims" => cmd_dims(term, cell_size),
        "lines" => cmd_lines(term),
        "line" => cmd_line(term, rest),
        "modes" => cmd_modes(term),
        "title" => cmd_title(term),
        "cwd" => cmd_cwd(term),
        "blocks" => cmd_blocks(term, rest),
        "blocktext" => cmd_blocktext(term, rest),
        "wait" => cmd_wait(term, rest),
        "colors" => cmd_colors(term),
        "select" => cmd_select(term, proxy, rest),
        "selection" => cmd_selection(term),
        "copy" => cmd_copy(term),
        _ => "ERR unknown verb\n".to_string(),
    }
}

/// Map a [`RenderCell`](aterm_core::terminal::RenderCell) char to its on-screen
/// glyph, collapsing NUL/control chars to a space.
fn visible_char(ch: char) -> char {
    if ch == '\0' || ch.is_control() {
        ' '
    } else {
        ch
    }
}

/// `text` -> `OK <nrows>\n` then each visible row (trailing spaces trimmed).
///
/// FIDELITY (I-1): each row is extracted through the engine's combining-aware
/// `get_line_text` — the SAME path `selection_to_string`/`copy` and the
/// renderer's `combining_row`/`cluster_row` use — so an NFD accent
/// (`e`+U+0301) or a ZWJ emoji cluster (👨‍👩‍👧) reads back intact instead of
/// being flattened to its base codepoint. (The old per-`RenderCell` scan only
/// saw the resolved base char and silently dropped combining marks / clusters,
/// corrupting the AI's primary screen-read.) Control chars still collapse to
/// spaces via the extraction's NUL→space rule plus an explicit visible map.
fn cmd_text(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let mut out = format!("OK {rows}\n");
    for r in 0..rows {
        // get_line_text(r, None) == visible_row_bounds_to_string over the whole
        // row == the selection path for a full-line selection of row r.
        let line = t.get_line_text(r as i32, None).unwrap_or_default();
        // Collapse interior control chars to spaces for a clean text read
        // (combining marks are non-control and survive), then re-trim the tail.
        let line: String = line.chars().map(visible_char).collect();
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// `cursor` -> `OK <row> <col> <visible:0|1> <style>\n` (0-based). `<style>`
/// is the terminal's DECSCUSR cursor style as a lowercase name:
/// `blinking_block` (default), `steady_block`, `blinking_underline`,
/// `steady_underline`, `blinking_bar`, `steady_bar`, `hidden`, `hollow_block`.
fn cmd_cursor(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let c = t.cursor();
    let vis = u8::from(t.cursor_visible());
    let style = cursor_style_name(t.cursor_style());
    format!("OK {} {} {} {}\n", c.row, c.col, vis, style)
}

/// The wire name of a [`CursorStyle`]: its variant in lowercase snake_case.
fn cursor_style_name(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::BlinkingBlock => "blinking_block",
        CursorStyle::SteadyBlock => "steady_block",
        CursorStyle::BlinkingUnderline => "blinking_underline",
        CursorStyle::SteadyUnderline => "steady_underline",
        CursorStyle::BlinkingBar => "blinking_bar",
        CursorStyle::SteadyBar => "steady_bar",
        CursorStyle::Hidden => "hidden",
        CursorStyle::HollowBlock => "hollow_block",
        // the enum is non-exhaustive; name future variants when they exist
        _ => "unknown",
    }
}

/// `cell <r> <c>` -> `OK <grapheme> <fg> <bg> <attrs>\n` or `ERR <msg>\n`.
///
/// `<grapheme>` is the cell's FULL on-screen grapheme — the resolved base char
/// plus any complex-cluster string and combining marks — percent-encoded into a
/// single space-free token (decode it the same way as `cwd`/`cmdline`). It is
/// the SAME text the `text`/`search`/selection paths and the renderer's
/// `combining_row`/`cluster_row` produce, so a single-cell read of `é`
/// (`e`+U+0301) or a ZWJ family (👨‍👩‍👧) is FAITHFUL — not the base codepoint
/// alone (FIDELITY I-1; this REPLACES the previous `char as u32` codepoint
/// field, which silently dropped combining marks / emoji clusters). A blank or
/// wide-continuation cell yields an empty token (`%20`-free → ``). `<fg>`/`<bg>`
/// are the fully-resolved `RRGGBB` colors the renderer would paint; `<attrs>` is
/// a comma-separated list (or `none`) of the cell's active text attributes —
/// `bold,dim,italic,underline,blink,inverse,strike,hidden`.
fn cmd_cell(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let mut it = rest.split_whitespace();
    let (Some(rs), Some(cs)) = (it.next(), it.next()) else {
        return "ERR usage: cell <r> <c>\n".to_string();
    };
    let (Ok(r), Ok(c)) = (rs.parse::<usize>(), cs.parse::<usize>()) else {
        return "ERR bad args\n".to_string();
    };
    let t = term_lock(term);
    // Bound by the GRID (per `dims`), not by row content: `render_row` trims
    // trailing blanks, but every 0<=r<rows, 0<=c<cols is a real, readable cell.
    if r >= t.rows() as usize || c >= t.cols() as usize {
        return "ERR out of range\n".to_string();
    }
    let row = t.render_row(r);
    let (fg, bg) = match row.get(c) {
        Some(cell) => (cell.fg, cell.bg),
        // a blank in-grid cell: the terminal's default colors
        None => {
            let (dfg, dbg) = (t.default_foreground(), t.default_background());
            ([dfg.r, dfg.g, dfg.b], [dbg.r, dbg.g, dbg.b])
        }
    };
    // Combining-aware grapheme for THIS cell, via the same core extraction the
    // selection/text paths use. A wide-continuation cell yields "" (its glyph
    // belongs to the lead cell); a blank cell yields "" (the consumer infers a
    // space from the in-grid position, matching `text`'s trailing trim).
    let grapheme = t.cell_grapheme(r, c).unwrap_or_default();
    let grapheme_tok = pct_encode(&grapheme);
    // Width markers, so a consumer can distinguish a full-width (CJK) glyph from
    // an ASCII space without inferring from columns:
    //   `wide`      — the LEAD cell, which holds the double-width glyph
    //   `wide_cont` — its blank right-half spacer
    // PROTECTED (DECSCA) shares a flag bit with WIDE_CONTINUATION;
    // `is_wide_continuation_at` disambiguates via the left neighbor, so a
    // protected character gets NEITHER token (it is ordinary text).
    let flags = cell_attrs(t.grid(), r, c);
    let mut attrs = attrs_string(flags);
    let wide_tok = if flags.contains(CellFlags::WIDE) {
        Some("wide")
    } else if t.grid().is_wide_continuation_at(r as u16, c as u16) {
        Some("wide_cont")
    } else {
        None
    };
    if let Some(tok) = wide_tok {
        if attrs == "none" {
            attrs = tok.to_string();
        } else {
            attrs.push(',');
            attrs.push_str(tok);
        }
    }
    // OSC 8 hyperlink target for this cell, surfaced so an introspecting
    // intelligence sees the link a human would click. Appended as a trailing
    // ` link=<url>` token only when present — positional fields 1-4 (grapheme,
    // fg, bg, attrs) are unchanged, so existing parsers keep working.
    let link = t
        .hyperlink_at(r as u16, c as u16)
        .map(|u| format!(" link={u}"))
        .unwrap_or_default();
    format!(
        "OK {grapheme_tok} {:02x}{:02x}{:02x} {:02x}{:02x}{:02x} {attrs}{link}\n",
        fg[0], fg[1], fg[2], bg[0], bg[1], bg[2],
    )
}

/// Resolve the effective [`CellFlags`] at grid `(r, c)`.
///
/// Inline-styled cells carry their attribute bits directly; cells that intern
/// their style in the grid's `StyleTable` keep only `USES_STYLE_ID` (plus any
/// extra flags) inline, so the real attributes are rehydrated from the table —
/// the same path [`Terminal::render_row`] uses for colors. Out-of-range
/// coordinates yield empty flags.
fn cell_attrs(grid: &Grid, r: usize, c: usize) -> CellFlags {
    let (Ok(row), Ok(col)) = (u16::try_from(r), u16::try_from(c)) else {
        return CellFlags::default();
    };
    let Some(cell) = grid.row(row).and_then(|gr| gr.get(col)) else {
        return CellFlags::default();
    };
    if cell.uses_style_id() {
        let extra = cell.flags().difference(CellFlags::USES_STYLE_ID);
        grid.resolve_style_to_colors(cell.style_id(), extra).2
    } else {
        cell.flags()
    }
}

/// Render active text attributes as a stable comma list, or `none` when bare.
///
/// `underline` is reported for any underline style (single/double/curly and the
/// dotted/dashed combinations, which all set one of those bits).
fn attrs_string(flags: CellFlags) -> String {
    let any_underline = CellFlags::UNDERLINE
        .union(CellFlags::DOUBLE_UNDERLINE)
        .union(CellFlags::CURLY_UNDERLINE);
    let mut parts: Vec<&str> = Vec::new();
    if flags.contains(CellFlags::BOLD) {
        parts.push("bold");
    }
    if flags.contains(CellFlags::DIM) {
        parts.push("dim");
    }
    if flags.contains(CellFlags::ITALIC) {
        parts.push("italic");
    }
    if flags.intersects(any_underline) {
        parts.push("underline");
    }
    if flags.contains(CellFlags::BLINK) {
        parts.push("blink");
    }
    if flags.contains(CellFlags::INVERSE) {
        parts.push("inverse");
    }
    if flags.contains(CellFlags::STRIKETHROUGH) {
        parts.push("strike");
    }
    if flags.contains(CellFlags::HIDDEN) {
        parts.push("hidden");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(",")
    }
}

/// `scroll <up|down|top|bottom|N>` -> move the scrollback viewport and report
/// the new position as `OK <display_offset> <scrollback_lines>\n`. `up`/`down`
/// move one screen into/out of history; `top`/`bottom` jump; a signed integer
/// `N` moves N lines into history (negative = toward the live bottom). With no
/// argument it just reports the current position. After moving it nudges a
/// windowed session to repaint (no-op when headless).
fn cmd_scroll(term: &Arc<Mutex<Terminal>>, proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    let mut t = term_lock(term);
    let page = i32::from(t.rows()).max(1);
    match rest.trim() {
        "" => {}
        "top" => t.scroll_to_top(),
        "bottom" => t.scroll_to_bottom(),
        "up" => t.scroll_display(page),
        "down" => t.scroll_display(-page),
        n => match n.parse::<i32>() {
            Ok(d) => t.scroll_display(d),
            Err(_) => return "ERR usage: scroll <up|down|top|bottom|N>\n".to_string(),
        },
    }
    let offset = t.grid().display_offset();
    let max = t.grid().scrollback_lines();
    drop(t);
    let _ = proxy.send_event(Wake::Output);
    format!("OK {offset} {max}\n")
}

/// `dims` -> `OK <rows> <cols> <pixel_w> <pixel_h>\n`. Pixels are the renderer's
/// fixed per-glyph cell size multiplied by the live grid (the framebuffer size
/// the `image` verb would produce).
fn cmd_dims(term: &Arc<Mutex<Terminal>>, cell_size: (u32, u32)) -> String {
    let t = term_lock(term);
    let rows = u32::from(t.rows());
    let cols = u32::from(t.cols());
    let (cw, ch) = cell_size;
    format!("OK {rows} {cols} {} {}\n", cols * cw, rows * ch)
}

/// `lines` -> `OK <total_scrollback_lines>\n` — how many lines of history
/// (tiered + ring-buffer scrollback) currently exist above the visible screen.
fn cmd_lines(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.grid().scrollback_lines())
}

/// `line <n>` -> `OK <text>\n` for the line at MONOTONIC ABSOLUTE row `n`, or
/// `ERR out of range\n` / `ERR evicted\n`.
///
/// COORDINATE SPACE (B-2): `n` is an ABSOLUTE row — the same space `blocks` and
/// `search` report — NOT a 0-based history index. This is the ONE documented
/// read coordinate: `blocks` gives output/command/prompt rows as absolute
/// numbers and `search` returns absolute match rows, and BOTH are fed straight
/// to `line`/`text` with the conversion done HERE at the read site. The mapping
/// (identical to the engine's `text_range`):
///   `hist = n - grid.oldest_absolute_row()`
///   `hist <  scrollback_lines`        → scrollback history line `hist`
///   `hist >= scrollback_lines`        → visible row `hist - scrollback_lines`
/// A row OLDER than `oldest_absolute_row()` has scrolled past the scrollback cap
/// and is reported as an EXPLICIT `ERR evicted\n` (never silently-shifted text —
/// the same eviction contract `blocktext` honors). Control chars collapse to
/// spaces; trailing spaces are trimmed.
fn cmd_line(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let Ok(n) = rest.trim().parse::<u64>() else {
        return "ERR usage: line <abs_row>\n".to_string();
    };
    let t = term_lock(term);
    let text = match abs_row_text(&t, n) {
        AbsRow::Text(s) => s,
        AbsRow::Evicted => return "ERR evicted\n".to_string(),
        AbsRow::OutOfRange => return "ERR out of range\n".to_string(),
    };
    let mut s: String = text.chars().map(visible_char).collect();
    while s.ends_with(' ') {
        s.pop();
    }
    format!("OK {s}\n")
}

/// Outcome of resolving an absolute row to its text (B-2 coordinate space).
enum AbsRow {
    /// The combining-aware, NOT-yet-control-collapsed line text.
    Text(String),
    /// Older than `oldest_absolute_row()` — scrolled past the scrollback cap.
    Evicted,
    /// Newer than the live bottom visible row (no such row).
    OutOfRange,
}

/// Resolve a MONOTONIC ABSOLUTE row to its grapheme-faithful text, in the ONE
/// documented read coordinate space shared by `blocks`/`search`/`line`/`text`.
///
/// Conversion is identical to the engine's `text_range`: an absolute row maps to
/// a history index relative to the oldest retained line; indices at/above the
/// scrollback count land on the visible screen. Scrollback lines come from
/// `get_history_line` (Line text); visible rows from the combining-aware
/// `get_line_text` so accents / ZWJ clusters survive (FIDELITY I-1).
fn abs_row_text(t: &Terminal, abs_row: u64) -> AbsRow {
    let grid = t.grid();
    let oldest = grid.oldest_absolute_row();
    if abs_row < oldest {
        return AbsRow::Evicted;
    }
    let scrollback = grid.scrollback_lines() as u64;
    let visible_rows = u64::from(t.rows());
    let rel = abs_row - oldest;
    if rel < scrollback {
        // Scrollback history line `rel` (0 = oldest retained).
        match grid.get_history_line(rel as usize) {
            Some(line) => AbsRow::Text(line.to_string()),
            None => AbsRow::OutOfRange,
        }
    } else {
        let visible = rel - scrollback;
        if visible >= visible_rows {
            return AbsRow::OutOfRange;
        }
        AbsRow::Text(t.get_line_text(visible as i32, None).unwrap_or_default())
    }
}

/// `search <pat> [case] [regex]` -> `OK <count>[ incomplete]\n` then
/// `<abs_row> <col> <len>` per match.
///
/// SEARCH-1: backed by the engine's real `TerminalSearch`, indexing BOTH the
/// SCROLLBACK (`get_history_line(0..scrollback_lines)`) AND the visible rows
/// with grapheme-aware text — so a term that has scrolled OFF the screen is
/// still found, not just the visible page. Each match's row is an ABSOLUTE row
/// (B-2's one coordinate space): feed it straight to `line`/`text`, which
/// convert at the read site. `col`/`len` are CHARACTER columns within that row.
///
/// FLAGS (order-independent, after the pattern): `case` = case-SENSITIVE match
/// (default is case-insensitive); `regex` = treat `<pat>` as a regular
/// expression (requires the `aterm-search` `regex` feature, enabled for the
/// engine). The pattern is the first token; flags are any trailing `case`/`regex`
/// tokens, so a literal pattern containing spaces is not supported here (use a
/// single-token needle, as the naive scan also required).
///
/// INCOMPLETE (DL-2): if the search index evicted lines (the searchable window
/// is capped), the header carries a trailing ` incomplete` token so the AI knows
/// results are NOT exhaustive rather than trusting a short list silently.
fn cmd_search(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    use aterm_core::search::TerminalSearch;

    let mut it = rest.split_whitespace();
    let Some(pat) = it.next() else {
        return "OK 0\n".to_string();
    };
    // Parse order-independent trailing flags.
    let (mut case_sensitive, mut is_regex) = (false, false);
    for tok in it {
        match tok {
            "case" => case_sensitive = true,
            "regex" => is_regex = true,
            other => return format!("ERR unknown flag: {other}\n"),
        }
    }

    let t = term_lock(term);
    let grid = t.grid();
    let oldest = grid.oldest_absolute_row();
    let scrollback = grid.scrollback_lines();
    let rows = t.rows() as usize;

    // Index every still-retained addressable line KEYED BY ABSOLUTE ROW, so each
    // returned SearchMatch.line is already an absolute row (no post-conversion):
    //   scrollback history 0..scrollback  -> absolute oldest + i
    //   visible rows       0..rows         -> absolute oldest + scrollback + r
    // index_visible_content assigns base_line + offset to each line in order.
    let mut search = TerminalSearch::new();
    let history: Vec<String> = (0..scrollback)
        .map(|i| {
            grid.get_history_line(i)
                .map(|l| l.to_string())
                .unwrap_or_default()
        })
        .collect();
    // oldest..(oldest+scrollback) are the absolute rows of these history lines.
    let hist_base = usize::try_from(oldest).unwrap_or(usize::MAX);
    search.index_visible_content(hist_base, &history);
    let visible: Vec<String> =
        (0..rows).map(|r| t.get_line_text(r as i32, None).unwrap_or_default()).collect();
    let vis_base = hist_base.saturating_add(scrollback);
    search.index_visible_content(vis_base, &visible);

    let results = match search.search_results_opts(pat, case_sensitive, is_regex) {
        Ok(r) => r,
        Err(e) => return format!("ERR search: {e}\n"),
    };
    drop(t);

    let incomplete = if results.incomplete { " incomplete" } else { "" };
    let mut out = format!("OK {}{incomplete}\n", results.matches.len());
    for m in &results.matches {
        // m.line is the ABSOLUTE row (we keyed the index by absolute row above).
        out.push_str(&format!("{} {} {}\n", m.line, m.start_col, m.len()));
    }
    out
}

/// `send <text>` -> write `<text>` to the PTY. A trailing literal `\n` (a
/// backslash followed by `n`) becomes carriage-return 0x0d so commands run.
fn cmd_send(master: i32, rest: &str) -> String {
    let bytes: Vec<u8> = if let Some(head) = rest.strip_suffix("\\n") {
        let mut b = head.as_bytes().to_vec();
        b.push(0x0d);
        b
    } else {
        rest.as_bytes().to_vec()
    };
    write_pty(master, &bytes);
    "OK\n".to_string()
}

/// `key <name>` -> send a named key to the PTY, encoded for the terminal's
/// CURRENT keyboard mode (DECCKM application-cursor-keys, application keypad,
/// kitty/xterm protocols) via the engine's own `encode_key` — so e.g. arrows
/// become SS3 `ESC O B` when an app has enabled DECCKM, not always CSI.
fn cmd_key(term: &Arc<Mutex<Terminal>>, master: i32, name: &str) -> String {
    use aterm_types::keyboard::NamedKey as Nk;
    let named = match name.trim() {
        "enter" => Nk::Enter,
        "tab" => Nk::Tab,
        "esc" | "escape" => Nk::Escape,
        "backspace" => Nk::Backspace,
        "delete" | "del" => Nk::Delete,
        "insert" | "ins" => Nk::Insert,
        "up" => Nk::ArrowUp,
        "down" => Nk::ArrowDown,
        "right" => Nk::ArrowRight,
        "left" => Nk::ArrowLeft,
        "home" => Nk::Home,
        "end" => Nk::End,
        "pageup" | "pgup" => Nk::PageUp,
        "pagedown" | "pgdn" => Nk::PageDown,
        "f1" => Nk::F1,
        "f2" => Nk::F2,
        "f3" => Nk::F3,
        "f4" => Nk::F4,
        "f5" => Nk::F5,
        "f6" => Nk::F6,
        "f7" => Nk::F7,
        "f8" => Nk::F8,
        "f9" => Nk::F9,
        "f10" => Nk::F10,
        "f11" => Nk::F11,
        "f12" => Nk::F12,
        _ => return "ERR\n".to_string(),
    };
    let mode = term_lock(term).keyboard_mode();
    let bytes = aterm_types::keyboard::encode_key(
        &aterm_types::keyboard::Key::Named(named),
        aterm_types::keyboard::Modifiers::empty(),
        mode,
    );
    write_pty(master, &bytes);
    "OK\n".to_string()
}

/// `ctrl <letter>` -> send a Control-modified key (Ctrl-C, Ctrl-L, ...) encoded
/// for the terminal's CURRENT keyboard mode (so it's a proper CSI-u sequence
/// under the kitty/xterm protocol, or the legacy control byte otherwise). This
/// is the correct way to deliver control keys — unlike a raw byte via `feed`,
/// it respects the active keyboard protocol.
fn cmd_ctrl(term: &Arc<Mutex<Terminal>>, master: i32, rest: &str) -> String {
    let mut chars = rest.trim().chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return "ERR usage: ctrl <single-letter>\n".to_string();
    };
    let mode = term_lock(term).keyboard_mode();
    let bytes = aterm_types::keyboard::encode_key(
        &aterm_types::keyboard::Key::Character(c.to_ascii_lowercase()),
        aterm_types::keyboard::Modifiers::CTRL,
        mode,
    );
    write_pty(master, &bytes);
    "OK\n".to_string()
}

/// `feed <hex>` -> write raw bytes (decoded from a hex string, whitespace
/// allowed) straight to the PTY. The escape hatch for control/binary bytes the
/// line-delimited `send` verb can't carry: `feed 03` = Ctrl-C, `feed 1b5b41` =
/// ESC[A, `feed 0a` = a real newline. Replies `OK <n> bytes\n` or `ERR bad hex`.
fn cmd_feed(master: i32, rest: &str) -> String {
    let hex: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    if hex.len() % 2 != 0 {
        return "ERR bad hex (odd length)\n".to_string();
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let h = hex.as_bytes();
    let mut i = 0;
    while i < h.len() {
        let hi = (h[i] as char).to_digit(16);
        let lo = (h[i + 1] as char).to_digit(16);
        let (Some(hi), Some(lo)) = (hi, lo) else {
            return "ERR bad hex\n".to_string();
        };
        bytes.push((hi * 16 + lo) as u8);
        i += 2;
    }
    let n = bytes.len();
    write_pty(master, &bytes);
    format!("OK {n} bytes\n")
}

/// `signal <name>` -> deliver a job-control signal to the PTY's CURRENT
/// foreground process group (via `tcgetpgrp` on the master + `killpg`).
/// `name` is one of `int`/`c`, `quit`, `tstp`/`z`, `hup`, `term`, `kill`.
/// This makes Ctrl-C/Ctrl-\\/Ctrl-Z effects deliverable and testable regardless
/// of the line discipline / launch context (which may not generate them).
fn cmd_signal(master: i32, rest: &str) -> String {
    let sig = match rest.trim() {
        "int" | "c" | "sigint" => libc::SIGINT,
        "quit" | "sigquit" => libc::SIGQUIT,
        "tstp" | "z" | "sigtstp" => libc::SIGTSTP,
        "hup" | "sighup" => libc::SIGHUP,
        "term" | "sigterm" => libc::SIGTERM,
        "kill" | "sigkill" => libc::SIGKILL,
        other => return format!("ERR unknown signal: {other}\n"),
    };
    let pgrp = unsafe { libc::tcgetpgrp(master) };
    if pgrp <= 0 {
        return "ERR no foreground process group\n".to_string();
    }
    let rc = unsafe { libc::killpg(pgrp, sig) };
    if rc == 0 {
        format!("OK signalled pgrp {pgrp}\n")
    } else {
        "ERR killpg failed\n".to_string()
    }
}

/// `mouse <action> <button> <row> <col>` -> encode a mouse event for the
/// terminal's CURRENT mouse mode + encoding (SGR/X10/...) via the engine's own
/// `encode_mouse_*` and write it to the PTY. `action` is one of
/// `press|release|move|wheelup|wheeldown`; `button` is `left|middle|right`
/// (ignored for the wheel actions). `row`/`col` are 0-based.
///
/// When mouse tracking is OFF the verb sends nothing and replies
/// `OK (mouse off)\n` so a test can detect the mode without poking the PTY. With
/// tracking ON it always replies `OK\n`; note the encoder may still legitimately
/// produce no bytes for the active mode (e.g. `move` under Normal tracking, or
/// `release` under X10), in which case nothing is written.
fn cmd_mouse(term: &Arc<Mutex<Terminal>>, master: i32, rest: &str) -> String {
    use aterm_types::mouse::MouseButton;
    let mut it = rest.split_whitespace();
    let (Some(action), Some(button_s), Some(rs), Some(cs)) =
        (it.next(), it.next(), it.next(), it.next())
    else {
        return "ERR usage: mouse <press|release|move|wheelup|wheeldown> <left|middle|right> <row> <col>\n".to_string();
    };
    let (Ok(row), Ok(col)) = (rs.parse::<u16>(), cs.parse::<u16>()) else {
        return "ERR bad args\n".to_string();
    };
    // `button` is ignored for the wheel actions; default to Left so a wheel
    // request needn't carry a meaningful button.
    let button = match button_s {
        "left" => MouseButton::Left,
        "middle" => MouseButton::Middle,
        "right" => MouseButton::Right,
        _ => return "ERR bad button\n".to_string(),
    };
    let t = term_lock(term);
    // Off-mode short-circuit: report the mode without touching the PTY.
    if !t.mouse_tracking_enabled() {
        return "OK (mouse off)\n".to_string();
    }
    // Note the engine arg order: (button_code, col, row, modifiers). The verb
    // takes row-then-col (matching `cell`/`resize`), so swap here.
    let bytes = match action {
        "press" => t.encode_mouse_press(button.code(), col, row, 0),
        "release" => t.encode_mouse_release(button.code(), col, row, 0),
        "move" => t.encode_mouse_motion(button.code(), col, row, 0),
        "wheelup" => t.encode_mouse_wheel(true, col, row, 0),
        "wheeldown" => t.encode_mouse_wheel(false, col, row, 0),
        _ => return "ERR bad action\n".to_string(),
    };
    drop(t);
    if let Some(b) = bytes {
        write_pty(master, &b);
    }
    "OK\n".to_string()
}

/// `paste <text>` -> write `<text>` to the PTY exactly as if the user pasted
/// it: [`Terminal::format_paste`] strips control bytes that could escape the
/// guards (ESC, C1 controls), converts line breaks to CR, and wraps the body
/// in the bracketed-paste guards `ESC[200~` ... `ESC[201~` when the app has
/// enabled bracketed paste (DECSET 2004). The text is the rest of the line
/// taken literally; a literal trailing `\n` (backslash + n) becomes a line
/// break (sent as CR, like a real paste) so a paste can end in one. For raw
/// unsanitized bytes use `feed`/`send` instead.
fn cmd_paste(term: &Arc<Mutex<Terminal>>, master: i32, rest: &str) -> String {
    let text = match rest.strip_suffix("\\n") {
        Some(head) => format!("{head}\n"),
        None => rest.to_string(),
    };
    let out = term_lock(term).format_paste(&text);
    write_pty(master, &out);
    "OK\n".to_string()
}

/// `modes` -> `OK\n` then one `key=value` line per introspected mode:
/// `alt_screen`, `cursor_visible`, `app_cursor_keys` (DECCKM),
/// `app_keypad` (DECPAM), `bracketed_paste` (2004), `mouse_mode`
/// (`none|normal|button|any|x10`), and `mouse_encoding`
/// (`x10|utf8|sgr|urxvt|sgr_pixel`).
fn cmd_modes(term: &Arc<Mutex<Terminal>>) -> String {
    use aterm_types::mouse::{MouseEncoding, MouseMode};
    let t = term_lock(term);
    let m = t.modes();
    let mouse_mode = match m.mouse_mode {
        MouseMode::None => "none",
        MouseMode::Normal => "normal",
        MouseMode::ButtonEvent => "button",
        MouseMode::AnyEvent => "any",
        MouseMode::X10 => "x10",
        _ => "unknown",
    };
    let mouse_encoding = match m.mouse_encoding {
        MouseEncoding::X10 => "x10",
        MouseEncoding::Utf8 => "utf8",
        MouseEncoding::Sgr => "sgr",
        MouseEncoding::Urxvt => "urxvt",
        MouseEncoding::SgrPixel => "sgr_pixel",
        _ => "unknown",
    };
    // Framed as `OK <n>` + n lines so the client streams the body (same shape
    // as `text`/`search`), rather than truncating to the status line.
    let lines = [
        format!("alt_screen={}", t.is_alternate_screen()),
        format!("cursor_visible={}", t.cursor_visible()),
        format!("app_cursor_keys={}", m.application_cursor_keys),
        format!("app_keypad={}", m.application_keypad),
        format!("bracketed_paste={}", m.bracketed_paste),
        format!("mouse_mode={mouse_mode}"),
        format!("mouse_encoding={mouse_encoding}"),
        // Affect how typed input / printed output lands, so a client driving the
        // terminal can predict behavior: IRM (insert vs overwrite), DECAWM
        // (auto-wrap at the right margin), DECOM (cursor origin = scroll region).
        format!("insert_mode={}", m.insert_mode),
        format!("auto_wrap={}", m.auto_wrap),
        format!("origin_mode={}", m.origin_mode),
    ];
    let mut out = format!("OK {}\n", lines.len());
    for l in &lines {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// `title` -> `OK <window title>\n` (the OSC 0/2 window title; empty if unset).
fn cmd_title(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.title())
}

/// `cwd` -> `OK <working directory>\n` (the shell's directory as reported via
/// OSC 7; empty if never reported). Lets an introspecting client know where
/// commands will run without scraping the prompt.
fn cmd_cwd(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.current_working_directory().unwrap_or(""))
}

/// Percent-encode a string so it occupies ONE space-free token in a response
/// line: every byte that is not ASCII-graphic (and `%` itself) becomes `%XX`.
/// Spaces, newlines and non-ASCII are escaped; the client decodes. Empty -> "".
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_graphic() && b != b'%' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `blocks [N]` -> the shell-integration command blocks (OSC 133/633), oldest
/// first (or the last `N`). This is the project's point made concrete: an AI
/// driving the terminal navigates by COMMAND — exit codes, the output's absolute
/// row range, the command text and cwd — instead of scraping the screen.
///
/// COORDINATE SPACE (B-2): every `prompt`/`cmd`/`out`/`end` row is a MONOTONIC
/// ABSOLUTE row, the SINGLE read coordinate this socket uses. Feed any of them
/// DIRECTLY to `line <abs_row>` (one row) or `text` (the visible screen) — those
/// verbs accept absolute rows and convert at the read site. (Previously `line`
/// took a 0-based history index, so feeding it a block's absolute row read the
/// WRONG line; `line` now shares the absolute-row space.) For a block's full
/// output prefer `blocktext <id>`, which reads the absolute range itself and
/// reports an EXPLICIT `ERR` when those rows have been EVICTED from scrollback
/// (never silently-shifted text).
///
/// Header `OK <shown>\n`, then one line per block: `block <id> <state>
/// exit=<code|-> prompt=<row> cmd=<row|-> out=<row|-> end=<row|-> cwd=<pct>
/// cmdline=<pct>`. `state` is prompt|entering|executing|complete; cwd/cmdline
/// are percent-encoded (single tokens even with spaces). Needs a shell emitting
/// OSC 133 (see the `shell_integration` injection); empty otherwise.
fn cmd_blocks(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    use aterm_core::terminal::BlockState;
    let t = term_lock(term);
    let all: Vec<_> = t.all_blocks().collect();
    let slice: &[_] = match rest.trim().parse::<usize>() {
        Ok(n) if n < all.len() => &all[all.len() - n..],
        _ => &all,
    };
    let mut out = format!("OK {}\n", slice.len());
    let opt_row = |r: Option<u64>| r.map_or_else(|| "-".to_string(), |v| v.to_string());
    for b in slice {
        let state = match b.state {
            BlockState::PromptOnly => "prompt",
            BlockState::EnteringCommand => "entering",
            BlockState::Executing => "executing",
            BlockState::Complete => "complete",
            _ => "unknown",
        };
        let exit = b.exit_code.map_or_else(|| "-".to_string(), |c| c.to_string());
        out.push_str(&format!(
            "block {} {} exit={} prompt={} cmd={} out={} end={} cwd={} cmdline={}\n",
            b.id,
            state,
            exit,
            b.prompt_start_row,
            opt_row(b.command_start_row),
            opt_row(b.output_start_row),
            opt_row(b.end_row),
            pct_encode(b.working_directory.as_deref().unwrap_or("")),
            pct_encode(b.commandline.as_deref().unwrap_or("")),
        ));
    }
    out
}

/// `blocktext <id>` -> the OUTPUT text of command block `<id>` (from `blocks`),
/// one row per line after `OK <n>`. The engine reads the block's absolute row
/// range itself (across scrollback AND the visible screen), so the caller does
/// NOT juggle coordinate spaces — an AI reads a specific command's output (e.g.
/// the failed one's error) directly. `ERR` if the id is unknown or the block has
/// not produced output yet.
fn cmd_blocktext(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let Ok(id) = rest.trim().parse::<u64>() else {
        return "ERR usage: blocktext <id>\n".to_string();
    };
    let t = term_lock(term);
    let Some(block) = t.block_by_id(id).cloned() else {
        return "ERR no such block\n".to_string();
    };
    // Use the enum form so an EVICTED block returns an explicit signal instead
    // of silently-shifted or empty text (B-1 / DL-1).
    let text = match t.block_output_text(&block) {
        aterm_core::terminal::BlockText::Text(s) => s,
        aterm_core::terminal::BlockText::Evicted => {
            return "ERR block output evicted from scrollback\n".to_string();
        }
        aterm_core::terminal::BlockText::NotAvailable => {
            return "ERR block has no output yet\n".to_string();
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = format!("OK {}\n", lines.len());
    for line in lines {
        let s: String = line.chars().map(visible_char).collect();
        out.push_str(s.trim_end());
        out.push('\n');
    }
    out
}

/// `wait [timeout_ms]` -> block until a command block COMPLETES (a NEW one since
/// this call), then `OK complete <id> exit=<code|->`; `OK timeout` if none
/// completes in time (default 30 000 ms, capped at 600 000). The AI runs a
/// command then `wait`s for it to finish before reading with `blocktext`, with
/// no busy-polling. Needs shell integration (OSC 133); with none it times out.
/// Polls server-side, releasing the Terminal lock between checks so the PTY
/// reader keeps advancing the command.
fn cmd_wait(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    use aterm_core::terminal::BlockState;
    let timeout_ms = rest.trim().parse::<u64>().unwrap_or(30_000).min(600_000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let complete_count =
        |t: &Terminal| t.all_blocks().filter(|b| matches!(b.state, BlockState::Complete)).count();
    let baseline = complete_count(&term_lock(term));
    loop {
        {
            let t = term_lock(term);
            let completed: Vec<_> =
                t.all_blocks().filter(|b| matches!(b.state, BlockState::Complete)).collect();
            if completed.len() > baseline {
                let b = completed.last().expect("len > baseline >= 0");
                let exit = b.exit_code.map_or_else(|| "-".to_string(), |c| c.to_string());
                return format!("OK complete {} exit={}\n", b.id, exit);
            }
        }
        if std::time::Instant::now() >= deadline {
            return "OK timeout\n".to_string();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// `colors` -> the terminal's theme colors:
/// `OK fg=<rrggbb> bg=<rrggbb> cursor=<rrggbb|default>`.
/// Programs change these via OSC 10/11/12; the per-cell `cell` verb only reports
/// already-RESOLVED colors, so this surfaces the theme itself (the default
/// fg/bg and the cursor color) for a client deciding how to render or reason.
fn cmd_colors(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let h = |r: u8, g: u8, b: u8| format!("{r:02x}{g:02x}{b:02x}");
    let fg = t.default_foreground();
    let bg = t.default_background();
    let cursor = t
        .cursor_color()
        .map_or_else(|| "default".to_string(), |c| h(c.r, c.g, c.b));
    format!(
        "OK fg={} bg={} cursor={}\n",
        h(fg.r, fg.g, fg.b),
        h(bg.r, bg.g, bg.b),
        cursor,
    )
}

/// Process-wide smart-selection rules, built lazily ONCE (the builtin rules
/// compile a set of regexes). Shared by the GUI's double-click gesture and the
/// `select word` verb so both use identical word/URL/path boundaries.
static SMART_RULES: OnceLock<SmartSelection> = OnceLock::new();

/// The engine's builtin smart-selection rules (lazy singleton).
pub(crate) fn smart_rules() -> &'static SmartSelection {
    SMART_RULES.get_or_init(SmartSelection::with_builtin_rules)
}

/// Inclusive word-column bounds at live-screen `(row, col)`, from the engine's
/// builtin smart-selection rules (URL/path/email/... patterns, falling back to
/// plain alphanumeric+underscore words). `None` when the cell is whitespace or
/// to the right of the row's text — the caller selects just the clicked cell.
pub(crate) fn word_cols(t: &Terminal, row: i32, col: u16) -> Option<(u16, u16)> {
    let text = t.get_line_text(row, None)?;
    // `word_boundaries_at_column` clamps a past-the-text column INTO the text
    // (it would snap to the LAST word); a click right of the text is whitespace.
    if usize::from(col) >= aterm_core::grapheme::byte_to_column(&text, text.len()) {
        return None;
    }
    let (start, end) = smart_rules().word_boundaries_at_column(&text, usize::from(col))?;
    // The returned end column is EXCLUSIVE; selection anchors are inclusive cells.
    let last = end.saturating_sub(1).max(start);
    let clamp = |v: usize| u16::try_from(v).unwrap_or(u16::MAX);
    Some((clamp(start), clamp(last)))
}

/// Word-select at live-screen `(row, col)` — the double-click / `select word`
/// gesture: a `Semantic` selection spanning the word's cells (both boundary
/// cells inclusive, Left/Right anchor sides), or just the clicked cell when on
/// whitespace. Completes the selection and returns the inclusive
/// `(start_col, end_col)` actually selected.
pub(crate) fn select_word(t: &mut Terminal, row: i32, col: u16) -> (u16, u16) {
    let (start, end) = word_cols(t, row, col).unwrap_or((col, col));
    let sel = t.text_selection_mut();
    sel.start_selection(row, col, SelectionSide::Left, SelectionType::Semantic);
    sel.expand_semantic(start, end);
    sel.complete_selection();
    (start, end)
}

/// Line-select live-screen row `row` — the triple-click / `select line`
/// gesture: a `Lines` selection expanded to the full row width (the extracted
/// text is the whole row, trailing blanks trimmed). Completes the selection.
pub(crate) fn select_line(t: &mut Terminal, row: i32) {
    let max_col = t.cols().saturating_sub(1);
    let sel = t.text_selection_mut();
    sel.start_selection(row, 0, SelectionSide::Left, SelectionType::Lines);
    sel.expand_lines(max_col);
    sel.complete_selection();
}

/// `select ...` -> drive the engine's text selection. Forms:
///
/// * `select <r1> <c1> <r2> <c2>` — simple range from cell `(r1,c1)` to
///   `(r2,c2)`, BOTH endpoint cells INCLUSIVE (the two points are normalized
///   to reading order first, so either order works).
/// * `select word <r> <c>` — semantic (word/URL/path) selection at the cell
///   via the engine's builtin smart-selection rules; a whitespace cell selects
///   just itself. Same code path as the GUI's double-click.
/// * `select line <r>` — full-line selection of row `r` (triple-click).
/// * `select block <r1> <c1> <r2> <c2>` — rectangular (block) selection with
///   the two cells as INCLUSIVE corners (any corner order).
/// * `select extend <r> <c>` — extend the EXISTING selection so cell `(r,c)`
///   becomes its new (inclusive) endpoint (shift-click); `ERR no selection`
///   when nothing is selected.
/// * `select clear` — clear the selection.
///
/// Rows are LIVE-screen coords as signed integers: `0..rows` is the visible
/// live screen and NEGATIVE rows address scrollback (`-1` = the most recently
/// scrolled-off line). All forms nudge a windowed session to repaint the
/// highlight and reply `OK\n`.
fn cmd_select(term: &Arc<Mutex<Terminal>>, proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    const USAGE: &str = "ERR usage: select <r1> <c1> <r2> <c2> | select word <r> <c> | \
                         select line <r> | select block <r1> <c1> <r2> <c2> | \
                         select extend <r> <c> | select clear\n";
    let rest = rest.trim();
    if rest == "clear" {
        term_lock(term).text_selection_mut().clear();
        let _ = proxy.send_event(Wake::Output);
        return "OK\n".to_string();
    }
    let mut it = rest.split_whitespace();
    let Some(head) = it.next() else {
        return USAGE.to_string();
    };
    match head {
        "word" => {
            let (Some(Ok(r)), Some(Ok(c))) = (
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
            ) else {
                return "ERR usage: select word <r> <c>\n".to_string();
            };
            select_word(&mut term_lock(term), r, c);
        }
        "line" => {
            let Some(Ok(r)) = it.next().map(str::parse::<i32>) else {
                return "ERR usage: select line <r>\n".to_string();
            };
            select_line(&mut term_lock(term), r);
        }
        "block" => {
            let (Some(Ok(r1)), Some(Ok(c1)), Some(Ok(r2)), Some(Ok(c2))) = (
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
            ) else {
                return "ERR usage: select block <r1> <c1> <r2> <c2>\n".to_string();
            };
            // Block normalization is corner-order agnostic (min/max per axis)
            // and forces Left/Right sides on the normalized corners, so both
            // given cells are inclusive whichever corners they are.
            let mut t = term_lock(term);
            let sel = t.text_selection_mut();
            sel.start_selection(r1, c1, SelectionSide::Left, SelectionType::Block);
            sel.update_selection(r2, c2, SelectionSide::Right);
            sel.complete_selection();
        }
        "extend" => {
            let (Some(Ok(r)), Some(Ok(c))) = (
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
            ) else {
                return "ERR usage: select extend <r> <c>\n".to_string();
            };
            let mut t = term_lock(term);
            let sel = t.text_selection_mut();
            if !sel.has_selection() || sel.is_empty() {
                return "ERR no selection\n".to_string();
            }
            // Side by direction so the clicked cell is INCLUDED whichever way
            // the selection grows: extending backward the moving anchor is the
            // normalized START (Left side includes its cell), extending
            // forward it is the normalized END (Right side includes its cell).
            let st = sel.start();
            let side = if (r, c) < (st.row, st.col) {
                SelectionSide::Left
            } else {
                SelectionSide::Right
            };
            sel.extend_selection(r, c, side);
            sel.complete_selection();
        }
        r1s => {
            let (Some(c1s), Some(r2s), Some(c2s)) = (it.next(), it.next(), it.next()) else {
                return USAGE.to_string();
            };
            let (Ok(r1), Ok(c1), Ok(r2), Ok(c2)) = (
                r1s.parse::<i32>(),
                c1s.parse::<u16>(),
                r2s.parse::<i32>(),
                c2s.parse::<u16>(),
            ) else {
                return "ERR bad args\n".to_string();
            };
            // Normalize to reading order so the Left/Right anchor sides below
            // always make BOTH endpoint cells inclusive (a Right-sided end
            // includes its cell; after normalization the end is never
            // side-flipped into an exclusion).
            let ((sr, sc), (er, ec)) = if (r2, c2) < (r1, c1) {
                ((r2, c2), (r1, c1))
            } else {
                ((r1, c1), (r2, c2))
            };
            let mut t = term_lock(term);
            let sel = t.text_selection_mut();
            sel.start_selection(sr, sc, SelectionSide::Left, SelectionType::Simple);
            sel.update_selection(er, ec, SelectionSide::Right);
            sel.complete_selection();
        }
    }
    let _ = proxy.send_event(Wake::Output);
    "OK\n".to_string()
}

/// `selection` -> the currently selected text as `OK <n>\n` + `n` data lines
/// (the text split on newlines, same framing as `text`). No or empty
/// selection -> `OK 0\n`.
fn cmd_selection(term: &Arc<Mutex<Terminal>>) -> String {
    match term_lock(term).selection_to_string() {
        Some(text) if !text.is_empty() => {
            let lines: Vec<&str> = text.split('\n').collect();
            let mut out = format!("OK {}\n", lines.len());
            for l in lines {
                out.push_str(l);
                out.push('\n');
            }
            out
        }
        _ => "OK 0\n".to_string(),
    }
}

/// `copy` -> copy the currently selected text to the macOS system clipboard
/// (`pbcopy`) and reply `OK <byte-count>\n`; no or empty selection -> `OK 0\n`
/// (the clipboard is left untouched). The selection is NOT cleared.
fn cmd_copy(term: &Arc<Mutex<Terminal>>) -> String {
    let text = term_lock(term).selection_to_string();
    match text {
        Some(t) if !t.is_empty() => {
            if pbcopy(&t) {
                format!("OK {}\n", t.len())
            } else {
                "ERR pbcopy failed\n".to_string()
            }
        }
        _ => "OK 0\n".to_string(),
    }
}

/// Pipe `text` to `/usr/bin/pbcopy`, placing it on the macOS system clipboard.
/// Shared by the `copy` verb and the GUI's Cmd-C. Returns success.
pub(crate) fn pbcopy(text: &str) -> bool {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new("/usr/bin/pbcopy").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    let wrote = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // Reap the child regardless of write success (no zombies on failure).
    let status = child.wait();
    wrote && status.is_ok_and(|s| s.success())
}

/// `image [path]` -> hand the render to the MAIN thread (it owns the renderer),
/// block on the reply, and report `OK <w> <h> <path>\n`.
///
/// PATH SAFETY: the PNG is confined to the `images/` subdir of the per-user
/// socket directory. A bare name (`image shot.png`) lands there; an empty
/// request defaults to `images/aterm-control.png`. A path that would escape the
/// subdir (`../`, an absolute path elsewhere, a symlink out) is refused with
/// `ERR path\n` and audited — the socket can no longer be used to overwrite an
/// arbitrary file via a caller-supplied path.
fn cmd_image(
    proxy: &EventLoopProxy<Wake>,
    queue: &ImageQueue,
    rest: &str,
    sock_dir: &std::path::Path,
) -> String {
    let requested = {
        let p = rest.trim();
        if p.is_empty() { "aterm-control.png" } else { p }
    };
    let Some(target) = control_auth::confine_image_path(sock_dir, requested) else {
        log_denial(
            AUDIT_SUBSYSTEM,
            &format!("image write '{requested}'"),
            aterm_containment::mode_or_containment(),
            "path escapes images/ subdir or names a nested target",
        );
        return "ERR path\n".to_string();
    };
    // For the reply only — the writer re-opens via the dir fd, not this string.
    let path = target.display_path().to_string_lossy().into_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    queue
        .lock()
        .unwrap()
        .push_back(ImageReq { target, reply: tx });
    if proxy.send_event(Wake::Control).is_err() {
        return "ERR event loop gone\n".to_string();
    }
    match rx.recv() {
        Ok((w, h)) => format!("OK {w} {h} {path}\n"),
        Err(_) => "ERR render failed\n".to_string(),
    }
}

/// Parse + range-check a `resize <r> <c>` request (the PURE part, so it is unit
/// testable without an event loop). Returns the validated `(rows, cols)` or the
/// exact error string the verb replies with.
///
/// Requests outside `1..=MAX_GRID_ROWS`/`MAX_GRID_COLS` are rejected with
/// `ERR out of range` rather than silently clamped, so a caller learns its
/// requested size was not applied.
fn parse_resize(rest: &str) -> Result<(u16, u16), String> {
    let mut it = rest.split_whitespace();
    let (Some(rs), Some(cs)) = (it.next(), it.next()) else {
        return Err("ERR usage: resize <r> <c>\n".to_string());
    };
    let (Ok(r), Ok(c)) = (rs.parse::<u16>(), cs.parse::<u16>()) else {
        return Err("ERR bad args\n".to_string());
    };
    if !(1..=MAX_GRID_ROWS).contains(&r) || !(1..=MAX_GRID_COLS).contains(&c) {
        return Err("ERR out of range\n".to_string());
    }
    Ok((r, c))
}

/// `resize <r> <c>` -> resize the engine grid, the PTY, AND the GUI (RES-1).
///
/// The main thread is the SOLE geometry owner (`App.rows/cols`, the framebuffer,
/// the window). Resizing the term + PTY here directly — as the verb used to —
/// left `App` stale and sent no repaint, so a follow-up `image`/`dims` (which
/// read `App`/the framebuffer) disagreed with the engine. So the verb now ONLY
/// validates and forwards a [`Wake::Resize`] to the main thread, which applies
/// the term + PTY + window resize and requests a redraw in one owner. A dropped
/// proxy (event loop gone) means the GUI is shutting down: report it.
fn cmd_resize(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    let (r, c) = match parse_resize(rest) {
        Ok(rc) => rc,
        Err(e) => return e,
    };
    if proxy.send_event(Wake::Resize { rows: r, cols: c }).is_err() {
        return "ERR event loop closed\n".to_string();
    }
    "OK\n".to_string()
}

/// Write all of `data` to the PTY master fd, ignoring a closed peer. Forwards to
/// the single PTY seam (`aterm-pty`) so the raw `write` syscall lives in one place.
fn write_pty(fd: i32, data: &[u8]) {
    aterm_pty::write_all(fd, data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::io::FromRawFd;

    /// Run `cmd_paste` against the write end of a pipe (standing in for the
    /// PTY master) and return the bytes that reached it.
    fn paste_to_pipe(term: &Arc<Mutex<Terminal>>, rest: &str) -> Vec<u8> {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        assert_eq!(cmd_paste(term, fds[1], rest), "OK\n");
        unsafe { libc::close(fds[1]) };
        let mut buf = Vec::new();
        let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        reader.read_to_end(&mut buf).expect("read pipe");
        buf
    }

    /// A paste planted with ESC[201~ must not terminate the bracket guard:
    /// the engine sanitizer strips ESC, so the only ESC[201~ on the wire is
    /// the final guard and the planted "[201~" is inert text.
    #[test]
    fn paste_verb_cannot_escape_bracket_guard() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        term.lock().unwrap().process(b"\x1b[?2004h");
        let got = paste_to_pipe(&term, "safe\x1b[201~rm -rf ~");
        assert_eq!(got, b"\x1b[200~safe[201~rm -rf ~\x1b[201~");
    }

    /// The literal trailing `\n` still ends the paste with a line break,
    /// which the engine sends as CR exactly like a real paste.
    #[test]
    fn paste_verb_trailing_newline_becomes_cr() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        assert_eq!(paste_to_pipe(&term, "echo hi\\n"), b"echo hi\r");
    }

    /// `resize 65535 65535` asks for a ~4.3-billion-cell allocation; the parse
    /// must reject anything outside 1..=MAX_GRID_ROWS/COLS. (RES-1: the verb now
    /// forwards a `Wake::Resize` to the geometry-owning main thread; the pure
    /// `parse_resize` is the validator the verb gates on.)
    #[test]
    fn resize_rejects_out_of_range() {
        for req in ["65535 65535", "4097 80", "24 4097", "0 80", "24 0"] {
            assert_eq!(parse_resize(req), Err("ERR out of range\n".to_string()));
        }
    }

    /// `cwd` surfaces the OSC 7-reported working directory (empty until set).
    #[test]
    fn cwd_verb_reports_working_directory() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        assert_eq!(cmd_cwd(&term), "OK \n");
        // OSC 7: a program reports its cwd as a file:// URI.
        term.lock()
            .unwrap()
            .process(b"\x1b]7;file://localhost/Users/ayates/x\x07");
        let out = cmd_cwd(&term);
        assert!(out.contains("/Users/ayates/x"), "cwd not surfaced: {out}");
    }

    /// `blocks` surfaces the OSC 133/633 shell-integration command blocks so an
    /// AI can navigate by command: exit codes, output row range, command text.
    #[test]
    fn blocks_verb_surfaces_command_blocks() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // No shell integration yet -> no blocks.
        assert_eq!(cmd_blocks(&term, ""), "OK 0\n");
        // Two command blocks via OSC 133 (+ OSC 633;E commandline): exit 0, exit 1.
        // Each OSC mark is BEL-terminated so the surrounding text isn't swallowed.
        term.lock().unwrap().process(
            b"\x1b]133;A\x07$ \x1b]633;E;echo hi\x07\x1b]133;B\x07echo hi\n\x1b]133;C\x07hi\n\x1b]133;D;0\x07\
\x1b]133;A\x07$ \x1b]633;E;false\x07\x1b]133;B\x07false\n\x1b]133;C\x07\x1b]133;D;1\x07",
        );
        let out = cmd_blocks(&term, "");
        assert!(out.starts_with("OK 2\n"), "expected 2 blocks: {out}");
        assert!(out.contains("exit=0") && out.contains("cmdline=echo%20hi"), "block 1 wrong: {out}");
        assert!(out.contains("exit=1") && out.contains("cmdline=false"), "block 2 wrong: {out}");
        // `blocks 1` returns only the most recent (the failed one).
        let last = cmd_blocks(&term, "1");
        assert!(last.starts_with("OK 1\n") && last.contains("exit=1"), "last block wrong: {last}");
        // `blocktext 0` reads block 0's OUTPUT directly (no coordinate math).
        let txt = cmd_blocktext(&term, "0");
        assert!(txt.starts_with("OK ") && txt.contains("hi"), "block 0 output wrong: {txt}");
        assert_eq!(cmd_blocktext(&term, "99"), "ERR no such block\n");
    }

    /// `wait` blocks until the in-flight command completes, then reports it; with
    /// no new completion it times out.
    #[test]
    fn wait_verb_blocks_until_command_completes() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // No command in flight -> a short wait times out.
        assert_eq!(cmd_wait(&term, "0"), "OK timeout\n");
        // Start a command (executing), then complete it from another thread.
        term.lock()
            .unwrap()
            .process(b"\x1b]133;A\x07$ \x1b]133;B\x07sleep\n\x1b]133;C\x07");
        let bg = term.clone();
        let h = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(40));
            bg.lock().unwrap().process(b"\x1b]133;D;0\x07");
        });
        let resp = cmd_wait(&term, "5000");
        h.join().unwrap();
        assert!(
            resp.starts_with("OK complete ") && resp.contains("exit=0"),
            "wait should report the completed command: {resp}"
        );
    }

    /// `cell` appends `link=<url>` for an OSC 8 hyperlinked cell, and nothing
    /// for a plain cell (positional fields unchanged for non-link cells).
    #[test]
    fn cell_verb_surfaces_osc8_hyperlink() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // OSC 8 open (target https://example.com), one glyph 'X', OSC 8 close.
        term.lock()
            .unwrap()
            .process(b"\x1b]8;;https://example.com\x1b\\X\x1b]8;;\x1b\\");
        let linked = cmd_cell(&term, "0 0");
        assert!(
            linked.contains("link=https://example.com"),
            "linked cell missing hyperlink: {linked}"
        );
        let plain = cmd_cell(&term, "0 5");
        assert!(!plain.contains("link="), "plain cell has a stray link: {plain}");
    }

    /// `colors` reports the theme and reflects OSC 10/11/12 dynamic changes.
    #[test]
    fn colors_verb_reports_theme() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let out = cmd_colors(&term);
        assert!(
            out.starts_with("OK fg=") && out.contains(" bg=") && out.contains(" cursor="),
            "unexpected colors format: {out}"
        );
        // OSC 11 sets the background; the verb must reflect it.
        term.lock().unwrap().process(b"\x1b]11;#102030\x07");
        assert!(
            cmd_colors(&term).contains("bg=102030"),
            "bg not updated: {}",
            cmd_colors(&term)
        );
    }

    /// `modes` exposes IRM / DECAWM / DECOM, which a driving client needs to
    /// predict how typed input and printed output land.
    #[test]
    fn modes_verb_exposes_insert_wrap_origin() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let out = cmd_modes(&term);
        assert!(out.contains("insert_mode=false"), "{out}");
        assert!(out.contains("auto_wrap=true"), "{out}");
        assert!(out.contains("origin_mode=false"), "{out}");
        // IRM on (ESC[4h), auto-wrap off (ESC[?7l), origin on (ESC[?6h).
        term.lock().unwrap().process(b"\x1b[4h\x1b[?7l\x1b[?6h");
        let out2 = cmd_modes(&term);
        assert!(
            out2.contains("insert_mode=true")
                && out2.contains("auto_wrap=false")
                && out2.contains("origin_mode=true"),
            "{out2}"
        );
    }

    /// An in-range resize parses to the requested `(rows, cols)` (RES-1: the
    /// engine/PTY/window resize then happens on the main thread via
    /// `Wake::Resize`, which a headless unit test cannot drive — so we verify the
    /// validated geometry the verb forwards).
    #[test]
    fn resize_parses_in_range() {
        assert_eq!(parse_resize("30 100"), Ok((30, 100)));
        assert_eq!(parse_resize(""), Err("ERR usage: resize <r> <c>\n".to_string()));
        assert_eq!(parse_resize("x y"), Err("ERR bad args\n".to_string()));
    }

    /// The combining-aware grapheme content of a single cell, taken via the
    /// SELECTION path (`select` that one cell + `selection_to_string`) — the
    /// fidelity ground truth the pixels also render. Used by the I-1 test to
    /// prove `text`/`cell`/`search` agree with selection.
    fn selection_of_cell(term: &Arc<Mutex<Terminal>>, row: i32, col: u16) -> String {
        let mut t = term_lock(term);
        let sel = t.text_selection_mut();
        sel.start_selection(row, col, SelectionSide::Left, SelectionType::Simple);
        sel.update_selection(row, col, SelectionSide::Right);
        sel.complete_selection();
        t.selection_to_string().unwrap_or_default()
    }

    /// I-1 FIDELITY: `text`/`cell`/`search` must return the SAME grapheme content
    /// (base char + combining marks + complex cluster) the SELECTION path returns
    /// — the renderer consumes that same content via combining_row/cluster_row, so
    /// this also proves text/cell/search agree with the rendered pixels. The old
    /// code read only the resolved base `RenderCell.ch`, silently dropping an NFD
    /// accent and a ZWJ emoji family; this test would fail against that code.
    #[test]
    fn read_paths_preserve_combining_and_zwj_clusters() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // Row 0: an NFD accent "é" = 'e' + U+0301 in one cell.
        // Row 1: a ZWJ family "👨‍👩‍👧" = man + ZWJ + woman + ZWJ + girl in one
        // (wide) cell. Both are folded into a single grid cell with the trailing
        // codepoints stored as combining marks (the same path the renderer reads).
        term.lock()
            .unwrap()
            .process("e\u{0301}\r\n\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}".as_bytes());

        // Ground truth from the selection path.
        let accent_sel = selection_of_cell(&term, 0, 0);
        let family_sel = selection_of_cell(&term, 1, 0);
        assert_eq!(accent_sel, "e\u{0301}", "selection ground truth (accent)");
        assert_eq!(
            family_sel, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "selection ground truth (ZWJ family)"
        );

        // ---- cell verb: grapheme token (pct-encoded) decodes to the selection.
        let accent_cell = cmd_cell(&term, "0 0");
        let accent_tok = accent_cell
            .strip_prefix("OK ")
            .and_then(|s| s.split(' ').next())
            .expect("cell OK token");
        assert_eq!(
            pct_decode(accent_tok), accent_sel,
            "cell grapheme must equal selection (accent): {accent_cell}"
        );
        let family_cell = cmd_cell(&term, "1 0");
        let family_tok = family_cell
            .strip_prefix("OK ")
            .and_then(|s| s.split(' ').next())
            .expect("cell OK token");
        assert_eq!(
            pct_decode(family_tok), family_sel,
            "cell grapheme must equal selection (ZWJ family): {family_cell}"
        );
        // The old `ch as u32` field would have been a bare codepoint, NOT the
        // multi-codepoint cluster — assert the cluster is really present.
        assert!(
            pct_decode(family_tok).chars().count() >= 5,
            "ZWJ family must keep all 5 codepoints, got {family_tok}"
        );

        // ---- text verb: the row line equals the full-row selection.
        let text = cmd_text(&term);
        let lines: Vec<&str> = text.lines().collect();
        // lines[0] is the "OK <n>" header; row r is lines[r+1].
        assert_eq!(lines[1], accent_sel, "text row 0 must equal selection: {text}");
        assert_eq!(lines[2], family_sel, "text row 1 must equal selection: {text}");

        // ---- search verb: searching the cluster finds it, and the located cell
        // reads back (via cell) the same grapheme the selection shows.
        let s = cmd_search(&term, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}");
        assert!(s.starts_with("OK 1"), "search must find the ZWJ family once: {s}");
        let hit = s.lines().nth(1).expect("a match row");
        let mut parts = hit.split(' ');
        let abs_row: u64 = parts.next().unwrap().parse().expect("abs row");
        // The absolute row resolves (via abs_row_text, the `line`/`text` space)
        // to a row whose text contains the faithful cluster.
        let row_text = match abs_row_text(&term_lock(&term), abs_row) {
            AbsRow::Text(t) => t,
            AbsRow::Evicted => panic!("search row {abs_row} unexpectedly evicted"),
            AbsRow::OutOfRange => panic!("search row {abs_row} out of range"),
        };
        assert!(
            row_text.contains(&family_sel),
            "search's absolute row must resolve to the cluster: {row_text:?}"
        );
    }

    /// Decode the percent-encoding `pct_encode` produces (test helper).
    fn pct_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).expect("valid utf8")
    }

    /// SEARCH-1: a term scrolled OFF the visible screen into scrollback is still
    /// found, the match's ABSOLUTE row resolves (via `line`) to the same content,
    /// and a regex query returns the expected matches. Proves the real
    /// `TerminalSearch` (scrollback + visible, regex) replaced the naive
    /// visible-only substring scan.
    #[test]
    fn search_finds_scrollback_and_regex() {
        // Small grid so content scrolls into history quickly.
        let term = Arc::new(Mutex::new(Terminal::new(4, 40)));
        // Print a unique needle, then enough lines to push it off-screen.
        term.lock().unwrap().process(b"NEEDLE_alpha\r\n");
        for i in 0..20 {
            term.lock().unwrap().process(format!("filler line {i}\r\n").as_bytes());
        }
        // The needle is no longer on the visible 4-row screen.
        let visible = cmd_text(&term);
        assert!(
            !visible.contains("NEEDLE_alpha"),
            "needle should have scrolled off-screen: {visible}"
        );
        // But search (which indexes scrollback) finds it.
        let s = cmd_search(&term, "NEEDLE_alpha");
        assert!(s.starts_with("OK 1"), "scrolled-off needle must be found: {s}");
        let hit = s.lines().nth(1).expect("match row");
        let abs_row: u64 = hit.split(' ').next().unwrap().parse().expect("abs row");
        // The `line` verb resolves that absolute row to the needle's content.
        let line_out = cmd_line(&term, &abs_row.to_string());
        assert!(
            line_out.contains("NEEDLE_alpha"),
            "line {abs_row} must resolve to the needle (got {line_out})"
        );

        // Regex: a single-token pattern + the `regex` flag. `fill[a-z]+` matches
        // every "filler" row (the pattern carries no spaces, so it stays one
        // token; the trailing `regex` is parsed as a flag).
        let rx = cmd_search(&term, "fill[a-z]+ regex");
        assert!(
            rx.starts_with("OK "),
            "regex search should succeed (regex feature enabled): {rx}"
        );
        let count: usize = rx
            .lines()
            .next()
            .and_then(|h| h.strip_prefix("OK "))
            .and_then(|n| n.split(' ').next())
            .and_then(|n| n.parse().ok())
            .expect("count");
        assert!(count >= 2, "regex `fill[a-z]+` should match many filler rows: {rx}");

        // Case sensitivity: default is insensitive, `case` flips it.
        let ci = cmd_search(&term, "needle_alpha");
        assert!(ci.starts_with("OK 1"), "case-insensitive default must match: {ci}");
        let cs = cmd_search(&term, "needle_alpha case");
        assert!(cs.starts_with("OK 0"), "case-sensitive must NOT match lowercased: {cs}");
    }
}
