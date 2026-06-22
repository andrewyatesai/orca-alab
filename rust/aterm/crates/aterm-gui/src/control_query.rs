// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Read-only screen introspection serializers — the SACRED AI-reads-the-screen
//! path. These verbs read the live [`Terminal`] grid/renderer and serialize it
//! to the control protocol's text/JSON replies; they NEVER mutate state. Moved
//! verbatim from `control.rs` (behavior-preserving); the shared JSON/encode
//! helpers (`json_*`, `pct_encode`, `visible_char`, `cursor_style_name`) stay in
//! `control.rs` and are imported via `super::`.

use std::sync::{Arc, Mutex};

use aterm_core::grid::extra::ImageData;
use aterm_core::grid::{CellFlags, Grid};
use aterm_core::terminal::{RenderCell, Terminal, UnderlineStyle};

use super::{
    cursor_style_name, image_payload, json_escape, json_ok, json_str_field, pct_encode,
    visible_char,
};
use crate::term_lock;

/// The visible, trailing-trimmed text of screen row `r`: the engine's
/// combining-aware `get_line_text` with interior control chars collapsed to
/// spaces and the tail trimmed. THE single source for a screen row's text —
/// `text`, `text --json`, and the pushed `subscribe screen` DELTA all route here
/// so the polled and pushed faces stay byte-identical. Caller holds the term lock.
pub(crate) fn visible_row(t: &Terminal, r: usize) -> String {
    let line = t.get_line_text(r as i32, None).unwrap_or_default();
    line.chars()
        .map(visible_char)
        .collect::<String>()
        .trim_end()
        .to_string()
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
pub(crate) fn cmd_text(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let mut out = format!("OK {rows}\n");
    for r in 0..rows {
        out.push_str(&visible_row(&t, r));
        out.push('\n');
    }
    out
}

/// `cursor` -> `OK <row> <col> <visible:0|1> <style>\n` (0-based). `<style>`
/// is the terminal's DECSCUSR cursor style as a lowercase name:
/// `blinking_block` (default), `steady_block`, `blinking_underline`,
/// `steady_underline`, `blinking_bar`, `steady_bar`, `hidden`, `hollow_block`.
pub(crate) fn cmd_cursor(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let c = t.cursor();
    let vis = u8::from(t.cursor_visible());
    let style = cursor_style_name(t.cursor_style());
    format!("OK {} {} {} {}\n", c.row, c.col, vis, style)
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
pub(crate) fn cmd_cell(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
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

/// `dims` -> `OK <rows> <cols> <pixel_w> <pixel_h>\n`. Pixels are the renderer's
/// fixed per-glyph cell size multiplied by the live grid (the framebuffer size
/// the `image` verb would produce).
pub(crate) fn cmd_dims(term: &Arc<Mutex<Terminal>>, cell_size: (u32, u32)) -> String {
    let t = term_lock(term);
    let rows = u32::from(t.rows());
    let cols = u32::from(t.cols());
    let (cw, ch) = cell_size;
    format!("OK {rows} {cols} {} {}\n", cols * cw, rows * ch)
}

/// `metrics [reset]` -> one `OK k=v ...\n` line of live render/latency counters so a
/// driving AI can MEASURE responsiveness AND DETECT lag in the same loop it drives
/// with — `send`/`key`, then `metrics`. `metrics reset` first zeroes the
/// measurement-window stats (frames / maxima / slow count) so a SPECIFIC workload can
/// be timed: `metrics reset`, drive it, then `metrics`.
///
/// Fields: `backend=<cpu|gpu>`, grid `rows`/`cols`, `frames` (real presents since
/// reset — a steady screen does NOT advance it), `last_/max_present_latency_ms` (the
/// `output→present` slice `$ATERM_TRACE_LATENCY` logs, most-recent + worst), and the
/// LAG SIGNATURE: `last_/max_frame_render_ms` + `slow_frames` (frames over the ~30 fps
/// budget, `slow_threshold_ms`). A non-zero `slow_frames`, a large
/// `max_frame_render_ms`, or `backend=cpu` under heavy output all mean the terminal is
/// lagging. Values are the process-global [`crate::metrics`] counters + the grid size.
pub(crate) fn cmd_metrics(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    if rest.trim() == "reset" {
        crate::metrics::reset();
    }
    let (rows, cols) = {
        let t = term_lock(term);
        (u32::from(t.rows()), u32::from(t.cols()))
    };
    let m = crate::metrics::snapshot();
    let backend = if m.backend_gpu { "gpu" } else { "cpu" };
    let ms = |ns: u64| ns as f64 / 1e6;
    format!(
        "OK backend={backend} rows={rows} cols={cols} frames={} \
         last_present_latency_ms={:.2} max_present_latency_ms={:.2} \
         last_frame_render_ms={:.2} max_frame_render_ms={:.2} \
         slow_frames={} slow_threshold_ms={:.1}\n",
        m.frames_presented,
        ms(m.last_present_latency_ns),
        ms(m.max_present_latency_ns),
        ms(m.last_frame_render_ns),
        ms(m.max_frame_render_ns),
        m.slow_frames,
        ms(crate::metrics::SLOW_FRAME_THRESHOLD_NS),
    )
}

/// `lines` -> `OK <total_scrollback_lines>\n` — how many lines of history
/// (tiered + ring-buffer scrollback) currently exist above the visible screen.
pub(crate) fn cmd_lines(term: &Arc<Mutex<Terminal>>) -> String {
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
pub(crate) fn cmd_line(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
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
pub(crate) enum AbsRow {
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
pub(crate) fn abs_row_text(t: &Terminal, abs_row: u64) -> AbsRow {
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
pub(crate) fn cmd_search(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
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

    // P1.0b: reuse the cached full-content search index when the active grid's
    // content is unchanged. `indexed_search` builds the SAME index this used to
    // build inline (every still-retained addressable line keyed by ABSOLUTE row:
    // scrollback history -> oldest + i, visible rows -> oldest + scrollback + r),
    // so each returned SearchMatch.line is already an absolute row and results
    // (matches, order, absolute rows, INCOMPLETE) are identical. It rebuilds only
    // on a content change (content_seq bump) or alt-screen swap; an unchanged
    // repeat query reuses the index for the O(1) win. `&mut` for the cache.
    let mut t = term_lock(term);
    let results = match t
        .indexed_search()
        .search_results_opts(pat, case_sensitive, is_regex)
    {
        Ok(r) => r,
        Err(e) => return format!("ERR search: {e}\n"),
    };
    drop(t);

    let incomplete = if results.incomplete {
        " incomplete"
    } else {
        ""
    };
    let mut out = format!("OK {}{incomplete}\n", results.matches.len());
    for m in &results.matches {
        // m.line is the ABSOLUTE row (we keyed the index by absolute row above).
        out.push_str(&format!("{} {} {}\n", m.line, m.start_col, m.len()));
    }
    out
}

/// `modes` -> `OK\n` then one `key=value` line per introspected mode:
/// `alt_screen`, `cursor_visible`, `app_cursor_keys` (DECCKM),
/// `app_keypad` (DECPAM), `bracketed_paste` (2004), `mouse_mode`
/// (`none|normal|button|any|x10`), and `mouse_encoding`
/// (`x10|utf8|sgr|urxvt|sgr_pixel`).
pub(crate) fn cmd_modes(term: &Arc<Mutex<Terminal>>) -> String {
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
pub(crate) fn cmd_title(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.title())
}

/// `cwd` -> `OK <working directory>\n` (the shell's directory as reported via
/// OSC 7; empty if never reported). Lets an introspecting client know where
/// commands will run without scraping the prompt.
pub(crate) fn cmd_cwd(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.current_working_directory().unwrap_or(""))
}

/// `text --json` -> `{"rows":["<row0>",...],"cursor":{...},"seq":N,"dims":{...}}`.
/// The rows are the SAME grapheme-faithful, control-collapsed, tail-trimmed lines
/// `cmd_text` emits, the cursor/dims mirror the `cursor`/`dims` verbs, and `seq`
/// is the engine `content_seq` (so an agent can diff frames without re-reading).
pub(crate) fn cmd_text_json(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let cols = t.cols();
    let mut row_items: Vec<String> = Vec::with_capacity(rows);
    for r in 0..rows {
        row_items.push(format!("\"{}\"", json_escape(&visible_row(&t, r))));
    }
    let c = t.cursor();
    let vis = t.cursor_visible();
    let style = cursor_style_name(t.cursor_style());
    json_ok(&format!(
        "{{\"rows\":[{}],\"cursor\":{{\"row\":{},\"col\":{},\"visible\":{vis},{}}},\
         \"dims\":{{\"rows\":{rows},\"cols\":{cols}}},\"seq\":{}}}",
        row_items.join(","),
        c.row,
        c.col,
        json_str_field("style", style),
        t.content_seq(),
    ))
}

/// `cursor --json` -> `{"row":R,"col":C,"visible":bool,"style":"<name>"}`.
pub(crate) fn cmd_cursor_json(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let c = t.cursor();
    json_ok(&format!(
        "{{\"row\":{},\"col\":{},\"visible\":{},{}}}",
        c.row,
        c.col,
        t.cursor_visible(),
        json_str_field("style", cursor_style_name(t.cursor_style())),
    ))
}

/// The wire name of an [`UnderlineStyle`]: lowercase, matching the SGR 4:x family.
fn underline_style_name(u: UnderlineStyle) -> &'static str {
    match u {
        UnderlineStyle::None => "none",
        UnderlineStyle::Single => "single",
        UnderlineStyle::Double => "double",
        UnderlineStyle::Curly => "curly",
        UnderlineStyle::Dotted => "dotted",
        UnderlineStyle::Dashed => "dashed",
    }
}

/// Serialize ONE cell as the canonical `StyledCell` JSON object — the LOSSLESS,
/// fully-resolved view a styled-screen consumer (an outer agent driving an inner
/// TUI) needs. Every rendition field is read from the RESOLVED [`RenderCell`]
/// (the renderer's own decisions: palette/RGB/bold-bright/dim/inverse/hidden/
/// DECSCNM already folded into `fg`/`bg`), NOT the raw flag bits — so this carries
/// the four decorations the legacy `cell` verb dropped (underline SUBSTYLE,
/// overline, underline colour, emoji presentation). `glyph` is the combining-aware
/// grapheme (same source as `cell`/`text`); `wide_lead` is the only geometry field
/// (the raw `WIDE` flag), kept distinct from the `wide` right-half continuation.
///
/// NOTE on semantic boundary: `dim`/`blink`/`inverse`/`hidden` are baked into the
/// resolved `fg`/`bg` by `render_row` and are deliberately NOT reported as attrs
/// (recovering them is the raw-flags path; byte-exact SGR replay is the `cast`
/// raw-bytes channel's job, not this resolved-screen view).
fn styled_cell_json(t: &Terminal, r: usize, c: usize, cell: &RenderCell) -> String {
    let mut attrs: Vec<&str> = Vec::new();
    if cell.bold {
        attrs.push("bold");
    }
    if cell.italic {
        attrs.push("italic");
    }
    if cell.underline != UnderlineStyle::None {
        attrs.push("underline");
    }
    if cell.strikethrough {
        attrs.push("strike");
    }
    let attrs_json = attrs
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(",");
    let glyph = t.cell_grapheme(r, c).unwrap_or_default();
    let underline_color = cell.underline_color.map_or_else(
        || "null".to_string(),
        |[r, g, b]| format!("\"{r:02x}{g:02x}{b:02x}\""),
    );
    let hyperlink = t
        .hyperlink_at(r as u16, c as u16)
        .map_or_else(|| "null".to_string(), |u| format!("\"{}\"", json_escape(u)));
    let wide_lead = cell_attrs(t.grid(), r, c).contains(CellFlags::WIDE);
    format!(
        "{{\"glyph\":\"{}\",\"fg\":\"{:02x}{:02x}{:02x}\",\"bg\":\"{:02x}{:02x}{:02x}\",\
         \"attrs\":[{attrs_json}],\"underline_style\":\"{}\",\"overline\":{},\
         \"underline_color\":{underline_color},\"emoji_presentation\":{},\
         \"wide\":{},\"wide_lead\":{wide_lead},\"hyperlink\":{hyperlink}}}",
        json_escape(&glyph),
        cell.fg[0],
        cell.fg[1],
        cell.fg[2],
        cell.bg[0],
        cell.bg[1],
        cell.bg[2],
        underline_style_name(cell.underline),
        cell.overline,
        cell.emoji_presentation,
        cell.wide,
    )
}

/// Build the whole styled-screen frame as a single-line JSON object:
/// `{"seq":N,"dims":{...},"cursor":{...},"rows":[[StyledCell,...],...]}`.
///
/// Called with the `Terminal` lock ALREADY HELD (the subscribe `cells` stream
/// reuses it under one lock so the frame is internally consistent). Every row is
/// padded out to the FULL grid width with default-coloured blanks (NO `trim_end`,
/// unlike `text`) so the consumer receives exactly `dims.rows × dims.cols` cells —
/// the lossless contract. The blank-tail fallback mirrors [`cmd_cell`] exactly.
/// The wire name of a DEC [`LineSize`](aterm_core::grid::LineSize): the renderer
/// scales these rows, so a lossless frame must carry them (audit finding F2).
fn line_size_name(s: aterm_core::grid::LineSize) -> &'static str {
    use aterm_core::grid::LineSize;
    match s {
        LineSize::SingleWidth => "single",
        LineSize::DoubleWidth => "double_width",
        LineSize::DoubleHeightTop => "double_height_top",
        LineSize::DoubleHeightBottom => "double_height_bottom",
        _ => "single",
    }
}

/// The DEC line size of visible row `r` (default single-width).
fn row_line_size(t: &Terminal, r: usize) -> aterm_core::grid::LineSize {
    u16::try_from(r)
        .ok()
        .and_then(|rr| t.grid().row(rr))
        .map_or(aterm_core::grid::LineSize::SingleWidth, |row| {
            row.line_size()
        })
}

/// Every DISTINCT inline image on the visible grid, each at its top-left grid
/// anchor `(row, col)` (deduplicated by payload identity). Shared shape with
/// `cmd_image_read`'s screen mode; consumed by the styled frame (audit finding F1)
/// so a `subscribe cells` / `screen` watcher sees images, not blank cells.
fn distinct_images(t: &Terminal) -> Vec<(usize, usize, std::sync::Arc<ImageData>)> {
    let mut seen: Vec<*const ImageData> = Vec::new();
    let mut out: Vec<(usize, usize, std::sync::Arc<ImageData>)> = Vec::new();
    for r in 0..t.rows() as usize {
        for (col, iref) in t.images_row(r) {
            let ptr = std::sync::Arc::as_ptr(&iref.image);
            if seen.contains(&ptr) {
                continue;
            }
            seen.push(ptr);
            let anchor_r = r.saturating_sub(iref.cell_row as usize);
            let anchor_c = col.saturating_sub(iref.cell_col as usize);
            out.push((anchor_r, anchor_c, iref.image.clone()));
        }
    }
    out
}

/// One inline image as a JSON object for the styled frame: anchor grid position,
/// cell footprint, format, raw byte length, and the base64 payload — so a watcher
/// reconstructs the picture the human sees, independent of the GUI framebuffer.
pub(crate) fn styled_image_json(anchor_r: usize, anchor_c: usize, img: &ImageData) -> String {
    let (fmt, b64) = image_payload(img); // F4: oversized -> ("truncated", "")
    format!(
        "{{\"row\":{anchor_r},\"col\":{anchor_c},\"cols\":{},\"rows\":{},\"format\":\"{fmt}\",\
         \"nbytes\":{},\"b64\":\"{b64}\"}}",
        img.cols,
        img.rows,
        img.bytes.len(),
    )
}

pub(crate) fn styled_frame_payload(t: &Terminal) -> String {
    let rows = t.rows() as usize;
    let cols = t.cols() as usize;
    let (dfg, dbg) = (t.default_foreground(), t.default_background());
    let blank = RenderCell {
        ch: ' ',
        fg: [dfg.r, dfg.g, dfg.b],
        bg: [dbg.r, dbg.g, dbg.b],
        wide: false,
        emoji_presentation: false,
        bold: false,
        italic: false,
        underline: UnderlineStyle::None,
        strikethrough: false,
        overline: false,
        underline_color: None,
    };
    let mut row_items: Vec<String> = Vec::with_capacity(rows);
    let mut line_sizes: Vec<&'static str> = Vec::with_capacity(rows);
    for r in 0..rows {
        let rendered = t.render_row(r);
        let mut cells: Vec<String> = Vec::with_capacity(cols);
        for c in 0..cols {
            let cell = rendered.get(c).unwrap_or(&blank);
            cells.push(styled_cell_json(t, r, c, cell));
        }
        row_items.push(format!("[{}]", cells.join(",")));
        line_sizes.push(line_size_name(row_line_size(t, r))); // F2: DEC double-width/height
    }
    let line_sizes_json = line_sizes
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(",");
    // F1: inline images (OSC 1337), base64 once per distinct image at its anchor —
    // without this a `cells`/`screen` watcher sees blank cells where the human sees
    // a picture. Empty array (cheap) on the common no-image screen.
    let images_json = distinct_images(t)
        .iter()
        .map(|(ar, ac, img)| styled_image_json(*ar, *ac, img))
        .collect::<Vec<_>>()
        .join(",");
    // F3: text selection highlight (a human/peer-initiated selection a watcher
    // would otherwise miss); `null` when nothing is selected.
    let sel = t.text_selection();
    let selection_json = if sel.is_empty() {
        "null".to_string()
    } else {
        let (sr, sc, er, ec) = sel.normalized_bounds();
        format!("{{\"start_row\":{sr},\"start_col\":{sc},\"end_row\":{er},\"end_col\":{ec}}}")
    };
    let cur = t.cursor();
    format!(
        "{{\"seq\":{},\"dims\":{{\"rows\":{rows},\"cols\":{cols}}},\
         \"cursor\":{{\"row\":{},\"col\":{},\"visible\":{},{}}},\
         \"rows\":[{}],\"line_sizes\":[{line_sizes_json}],\"selection\":{selection_json},\
         \"images\":[{images_json}]}}",
        t.content_seq(),
        cur.row,
        cur.col,
        t.cursor_visible(),
        json_str_field("style", cursor_style_name(t.cursor_style())),
        row_items.join(","),
    )
}

/// COMPILE-GATE (the GENERAL fix for the F1/F2/F3 dropped-field class): every field
/// of the renderer's input MUST be a CONSCIOUS decision — reflected in the lossless
/// styled frame, or explicitly omitted with a reason. This destructures
/// [`RenderInput`](aterm_core::render::RenderInput) WITHOUT `..`, so adding a new
/// renderer-consumed field fails to compile until someone decides whether
/// `styled_frame_payload` carries it. That turns "we silently dropped a field"
/// (F1 images, F2 line_sizes, F3 selection — all present in `RenderInput`, all once
/// missing from the frame) into a build error. Never called; it exists to type-check.
#[allow(dead_code)]
fn _styled_frame_covers_every_render_input_field(ri: &aterm_core::render::RenderInput) {
    let aterm_core::render::RenderInput {
        rows: _,           // frame "dims.rows"
        cols: _,           // frame "dims.cols"
        cells: _,          // frame "rows" (per-cell styled_cell_json)
        cursor_row: _,     // frame "cursor.row"
        cursor_col: _,     // frame "cursor.col"
        cursor_visible: _, // frame "cursor.visible"
        cursor_style: _,   // frame "cursor.style"
        display_offset: _, // OMITTED: viewport scroll position, not visible-cell content
        selection: _,      // frame "selection" (F3)
        clusters: _,       // folded into per-cell "glyph" (cell_grapheme)
        combining: _,      // folded into per-cell "glyph" (cell_grapheme)
        line_sizes: _,     // frame "line_sizes" (F2)
        images: _,         // frame "images" (F1)
        snapshot_seq: _,   // frame "seq" (the engine content version stamp)
    } = ri;
}

/// `screen` -> the full LOSSLESS styled grid as a single-line JSON frame, wrapped
/// in the standard `OK 1\n<json>\n` read framing (so the existing line-count
/// client streams it unchanged). This is the keystone "see everything" verb: it
/// carries per-cell colour + every resolved decoration + the cursor + dims + seq,
/// so an outer agent reconstructs exactly what a human sees in the inner TUI.
/// `--json` is implied (the verb is always styled JSON).
pub(crate) fn cmd_screen_styled_json(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    json_ok(&styled_frame_payload(&t))
}

/// `dims --json` -> `{"rows":R,"cols":C,"pixel_w":W,"pixel_h":H}`.
pub(crate) fn cmd_dims_json(term: &Arc<Mutex<Terminal>>, cell_size: (u32, u32)) -> String {
    let t = term_lock(term);
    let rows = u32::from(t.rows());
    let cols = u32::from(t.cols());
    let (cw, ch) = cell_size;
    json_ok(&format!(
        "{{\"rows\":{rows},\"cols\":{cols},\"pixel_w\":{},\"pixel_h\":{}}}",
        cols * cw,
        rows * ch,
    ))
}

/// `colors` -> the terminal's theme colors:
/// `OK fg=<rrggbb> bg=<rrggbb> cursor=<rrggbb|default>`.
/// Programs change these via OSC 10/11/12; the per-cell `cell` verb only reports
/// already-RESOLVED colors, so this surfaces the theme itself (the default
/// fg/bg and the cursor color) for a client deciding how to render or reason.
pub(crate) fn cmd_colors(term: &Arc<Mutex<Terminal>>) -> String {
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
