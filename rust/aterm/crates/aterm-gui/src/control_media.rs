// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Media-capture verbs: `image` (terminal-content framebuffer PNG), `image read`
//! (structured inline-image payloads, headless/cross-session-safe), `window`
//! (whole-window composited PNG), and `chrome` (native macOS UI readout). Moved
//! verbatim from `control.rs` (behavior-preserving). The `ImageReq`/`ImageQueue`
//! types and the `AUDIT_SUBSYSTEM` name stay in `control.rs`, reached via `super::`.

use std::sync::{Arc, Mutex};

use aterm_containment::log_denial;
use aterm_core::grid::extra::{ImageData, ImageFormat};
use aterm_core::terminal::Terminal;
use winit::event_loop::EventLoopProxy;

use super::{AUDIT_SUBSYSTEM, ImageQueue, ImageReq};
use crate::control_auth;
use crate::{Wake, term_lock};

/// One `image read` result line:
/// `<row> <col> <img_cols> <img_rows> <cell_row> <cell_col> <format> <nbytes> <base64>`.
/// `row`/`col` are the image's TOP-LEFT anchor on the grid; `cell_row`/`cell_col`
/// are the tile of interest (0/0 for a whole-image report, the queried tile in
/// cell mode); `nbytes` is the raw (pre-base64) length; the trailing base64 is the
/// image's full raw payload (PNG bytes etc.), independent of the GUI framebuffer.
/// Per-image payload cap for the line + JSON image channels (audit finding F4). An
/// inline image is USER-supplied (the inner TUI emits OSC 1337), so a hostile or
/// careless inner could embed a multi-megabyte image and force a large base64
/// allocation on every `image read` AND every styled `cells`/`screen` frame. Above
/// this raw-byte cap the payload is OMITTED and the image marked `truncated` — the
/// metadata + real `nbytes` still report it, so a consumer learns an image is there
/// and how big it is, then fetches it deliberately, without the per-frame blowup.
pub(crate) const MAX_IMAGE_PAYLOAD_BYTES: usize = 4 * 1024 * 1024; // 4 MiB raw (~5.3 MiB base64)

/// `(format, base64)` for an image, applying the F4 cap: oversized images report
/// `("truncated", "")` instead of encoding their bytes.
pub(crate) fn image_payload(img: &ImageData) -> (&'static str, String) {
    let fmt = match img.format {
        ImageFormat::Png => "png",
        ImageFormat::RawRgba8 { .. } => "rgba",
        _ => "unknown",
    };
    if img.bytes.len() > MAX_IMAGE_PAYLOAD_BYTES {
        ("truncated", String::new())
    } else {
        (fmt, aterm_codec::base64::encode(&img.bytes))
    }
}

pub(crate) fn image_read_line(
    anchor_r: usize,
    anchor_c: usize,
    tile_row: u16,
    tile_col: u16,
    img: &ImageData,
) -> String {
    let (fmt, b64) = image_payload(img);
    format!(
        "{anchor_r} {anchor_c} {} {} {tile_row} {tile_col} {fmt} {} {b64}",
        img.cols,
        img.rows,
        img.bytes.len(),
    )
}

/// `image read [<r> [<c>]]` -> the structured inline-image payloads (iTerm2 OSC
/// 1337) as base64, readable HEADLESS and CROSS-SESSION (unlike the framebuffer
/// `image` rasterize verb). With no args it reports every DISTINCT image on the
/// grid (deduplicated by payload identity), one line per image at its top-left
/// anchor; `image read <r>` restricts to images intersecting row `r`; `image read
/// <r> <c>` returns the single image tile covering that exact cell (`ERR none` if
/// the cell has no image). Framed `OK <nlines>\n` + one line per image.
pub(crate) fn cmd_image_read(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let cols = t.cols() as usize;
    let mut it = rest.split_whitespace();
    let r_tok = it.next();
    let c_tok = it.next();

    // Cell mode: the image covering exactly (r, c).
    if let (Some(rs), Some(cs)) = (r_tok, c_tok) {
        let (Ok(r), Ok(c)) = (rs.parse::<usize>(), cs.parse::<usize>()) else {
            return "ERR bad args\n".to_string();
        };
        if r >= rows || c >= cols {
            return "ERR out of range\n".to_string();
        }
        for (col, iref) in t.images_row(r) {
            if col == c {
                let anchor_r = r.saturating_sub(iref.cell_row as usize);
                let anchor_c = col.saturating_sub(iref.cell_col as usize);
                return format!(
                    "OK 1\n{}\n",
                    image_read_line(
                        anchor_r,
                        anchor_c,
                        iref.cell_row,
                        iref.cell_col,
                        &iref.image
                    )
                );
            }
        }
        return "ERR none\n".to_string();
    }

    // Row mode (one row) or screen mode (all rows): distinct images, anchored.
    let row_range: Vec<usize> = match r_tok {
        Some(rs) => match rs.parse::<usize>() {
            Ok(r) if r < rows => vec![r],
            Ok(_) => return "ERR out of range\n".to_string(),
            Err(_) => return "ERR bad args\n".to_string(),
        },
        None => (0..rows).collect(),
    };
    let mut seen: Vec<*const ImageData> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for r in row_range {
        for (col, iref) in t.images_row(r) {
            let ptr = std::sync::Arc::as_ptr(&iref.image);
            if seen.contains(&ptr) {
                continue;
            }
            seen.push(ptr);
            let anchor_r = r.saturating_sub(iref.cell_row as usize);
            let anchor_c = col.saturating_sub(iref.cell_col as usize);
            // Whole-image report: anchor + tile 0/0 (the full payload is carried).
            lines.push(image_read_line(anchor_r, anchor_c, 0, 0, &iref.image));
        }
    }
    let mut out = format!("OK {}\n", lines.len());
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
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
pub(crate) fn cmd_image(
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

/// `window [path]` -> capture the FRONT window's ENTIRE on-screen pixels — the
/// native macOS chrome (titlebar, traffic lights, the unified toolbar, the
/// full-width tab strip) AND the terminal content — to a PNG, replying
/// `OK <w> <h> <path>` (the SAME wire shape as `image`). This closes the
/// introspection gap `image` leaves: `image` rasterizes only the terminal content
/// framebuffer with NO OS chrome, so an AI driving aterm could never SEE the whole
/// window; `window` photographs the real composited pixels, so its height is
/// GREATER than `image`'s (it includes the titlebar + tab strip).
///
/// PATH CONFINEMENT (mirrors [`cmd_image`]): the caller-supplied `path` is validated
/// by `confine_image_path` to a single filename inside the socket dir's `images/`
/// subdir, so the socket can never overwrite an arbitrary file. The default name
/// (`aterm-window.png`) parallels `image`'s `aterm-control.png`.
///
/// MAIN-THREAD HOP (mirrors [`cmd_chrome`]): reaching the front window's `NSWindow` +
/// reading its window number + calling `CGWindowListCreateImage` may ONLY happen
/// on the main thread (AppKit / window-server state), but this runs on a background
/// control thread. So we post [`Wake::CaptureWindow`] with the confined target + a
/// one-shot reply channel and BLOCK; the main thread captures (`App::capture_window`)
/// and replies `Ok((w, h))` or an `Err(msg)` we surface verbatim as `ERR <msg>`.
///
/// PERMISSION: `CGWindowListCreateImage` needs macOS Screen Recording permission; if
/// it is not granted the main thread replies with the clear, actionable grant
/// instructions (it does NOT crash). Off macOS the main thread replies that capture
/// is macOS-only; headless (no OS window) replies that there is no window to capture.
pub(crate) fn cmd_window(
    proxy: &EventLoopProxy<Wake>,
    rest: &str,
    sock_dir: &std::path::Path,
) -> String {
    let requested = {
        let p = rest.trim();
        if p.is_empty() { "aterm-window.png" } else { p }
    };
    let Some(target) = control_auth::confine_image_path(sock_dir, requested) else {
        log_denial(
            AUDIT_SUBSYSTEM,
            &format!("window write '{requested}'"),
            aterm_containment::mode_or_containment(),
            "path escapes images/ subdir or names a nested target",
        );
        return "ERR path\n".to_string();
    };
    // For the reply only — the writer re-opens via the dir fd, not this string.
    let path = target.display_path().to_string_lossy().into_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy
        .send_event(Wake::CaptureWindow {
            path: target,
            reply: tx,
        })
        .is_err()
    {
        return "ERR event loop gone\n".to_string();
    }
    match rx.recv() {
        Ok(Ok((w, h))) => format!("OK {w} {h} {path}\n"),
        // The main thread's clear, actionable message (missing permission / headless /
        // off-macOS / capture failure) is surfaced verbatim as a single `ERR` line.
        Ok(Err(msg)) => format!("ERR {msg}\n"),
        Err(_) => "ERR window capture failed\n".to_string(),
    }
}

/// `chrome` -> dump the frontmost window's NATIVE macOS UI: its `NSToolbar` items
/// (each `id=<identifier> label="<label>"`, e.g. the "+" New Tab button) and the
/// app menu bar (`menu "File": New Window, New Tab, ...`). A read-only
/// introspection verb so an AI driving aterm can SEE and verify the native chrome
/// — which `image`/`text` CANNOT capture, as they render only the terminal content
/// view, never the OS toolbar/menu bar.
///
/// MAIN-THREAD HOP (mirrors [`cmd_image`]): AppKit objects (`NSToolbar`/`NSMenu`/
/// `NSWindow`) may ONLY be touched on the main thread, but this runs on a
/// background control thread. So we build a one-shot reply channel, post
/// [`Wake::ReadChrome`] to wake the event loop, and BLOCK on the reply; the main
/// thread reads the chrome (`App::read_native_chrome`) and sends back the text
/// lines. The lines are returned in the SAME multi-line shape as `text`:
/// `OK <n>\n` followed by `<n>` data rows.
///
/// Off macOS the main thread replies with one explanatory line (no native chrome),
/// so the wire shape (`OK 1` + one row) is identical on every platform.
pub(crate) fn cmd_chrome(proxy: &EventLoopProxy<Wake>) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy.send_event(Wake::ReadChrome { reply: tx }).is_err() {
        return "ERR event loop gone\n".to_string();
    }
    let lines = match rx.recv() {
        Ok(lines) => lines,
        Err(_) => return "ERR chrome read failed\n".to_string(),
    };
    // Same `OK <n>\n` + n-rows framing the `text` verb uses, so the aterm-ctl client
    // prints the rows verbatim (it lists `chrome` among the multi-line verbs).
    let mut out = format!("OK {}\n", lines.len());
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}
