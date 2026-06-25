// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The SACRED introspection render path: the AI-reads-the-real-screen feature.
//! `snapshot` (SIGUSR1 PNG+txt) and `render_image` (the control `image` verb)
//! render the CURRENT terminal through the SAME renderer the window uses — what
//! the AI sees is byte-identical to what is presented (WYSIWYG, incl. the bell
//! invert + tab-strip splice). `read_native_chrome`/`capture_window` add the OS
//! chrome + the on-glass window capture. Verbatim relocation — never alter this
//! logic; this is a hard project invariant.

use std::time::Instant;

use crate::app_render::apply_bell_invert;
#[cfg(target_os = "macos")]
use crate::platform::AppRt;
use crate::{App, accessibility, control_auth, snapshot_path, term_lock};

impl App {
    /// Introspect the live screen: render the CURRENT terminal to a PNG (the
    /// exact pixels on screen, via the same renderer the window uses) and write a
    /// parallel .txt of the visible text. Triggered by SIGUSR1. The files are
    /// written 0600 into the per-user 0700 control dir by default;
    /// $ATERM_SNAPSHOT_PATH overrides only into a safe dir (see `snapshot_path`).
    pub(crate) fn snapshot(&mut self) {
        let Some(path) = snapshot_path::resolve() else {
            return; // refusal already logged by resolve()
        };
        let Some(front) = self.frontmost_window else {
            return;
        };
        let strip_rows = self.tab_strip_rows as usize;
        // Trailing HUD rows are chrome too — captured here so the .txt below can drop
        // them (the front borrow can't read `self.*`).
        let hud_rows = self.hud_rows as usize;
        let (rows, cols) = match self.windows.get(&front) {
            Some(ws) => (ws.rows as usize, ws.cols as usize),
            None => return,
        };
        // Lock only to snapshot the grid; render + serialize without the lock.
        {
            let Some(ws) = self.windows.get_mut(&front) else {
                return;
            };
            let mut term = term_lock(&ws.term);
            // REFILL the reused snapshot in place (no per-frame container-Vec alloc).
            // A-3: the ENGINE builds the snapshot (`Terminal::cell_frame_into`).
            term.cell_frame_into(&mut ws.input_scratch, rows, cols);
        }
        // WYSIWYG: the on-screen present splices the tab strip above the terminal
        // grid, so splice it here too — the snapshot pixels then match the glass. A
        // no-op when the strip is disabled. Done BEFORE the disjoint-field borrow.
        self.splice_tab_strip(front);
        // Sample the interval-driven panels here too: headless has no window tick,
        // so this is what makes CPU/net/app-fed live in `image`/`snapshot` output.
        let hud_now = Instant::now();
        for p in &mut self.panels {
            p.poll(hud_now);
        }
        self.splice_hud_bar(front);
        // Disjoint borrows: `self.backend` (renderer), the introspection GPU
        // scratch, and the front window's input_scratch are separate fields.
        let App {
            backend,
            introspect_gpu,
            windows,
            ..
        } = self;
        let Some(ws) = windows.get_mut(&front) else {
            return;
        };
        // pixels: the same offscreen frame the window blits on screen (GPU path
        // if active) — byte-identical, so the AI sees exactly what is presented.
        // `backend.render_input` returns an owned Frame on both backends (the
        // snapshot/image path keeps the pixels past the next render, unlike the
        // borrowing window hot path).
        let mut frame = backend.render_input(introspect_gpu, &ws.input_scratch);
        // I-2: WYSIWYG — the on-screen present inverts the whole frame during a
        // visual-bell flash (CPU `src ^ 0x00ff_ffff`; GPU blit shader). Apply the
        // SAME invert here so a snapshot taken DURING a flash matches the glass
        // instead of showing the un-inverted frame.
        apply_bell_invert(&mut frame, ws.bell_flash.is_active(Instant::now()));
        // text: the visible grid, row by row, from the same snapshot. Shares the
        // exact row serialization with the accessibility snapshot (push_visible_row)
        // so "what an AI sees" and "what a screen reader reads" never diverge. The
        // tab-strip chrome rows (top `tab_strip_rows`) are skipped — the .txt is the
        // terminal text only (a no-op skip when the strip is disabled).
        let mut text = String::with_capacity(rows * (cols + 1));
        // Skip the tab-strip CHROME rows so the .txt is terminal text only (a no-op
        // skip when the strip is disabled — byte-identical to the pre-strip snapshot).
        let txt_end = ws.input_scratch.cells.len().saturating_sub(hud_rows);
        for cells in ws.input_scratch.cells[strip_rows..txt_end].iter() {
            accessibility::push_visible_row(&mut text, cells, cols);
        }
        let _ = snapshot_path::write_private(std::path::Path::new(&path), &frame.to_png());
        let _ = snapshot_path::write_private(
            std::path::Path::new(&format!("{path}.txt")),
            text.as_bytes(),
        );
        // a marker the requester can stat() for; stderr is unreliable for GUIs
        let _ = snapshot_path::write_private(
            std::path::Path::new(&format!("{path}.done")),
            format!("{}x{}\n", frame.width, frame.height).as_bytes(),
        );
        eprintln!("aterm-gui: snapshot written to {path} (+ .txt, .done)");
    }

    /// Render the CURRENT terminal to the confined `target` (the same renderer
    /// the window uses, GPU path if active) and return the frame's
    /// `(width, height)`. Serves the control socket's `image` verb; runs on the
    /// main thread per [`Wake::Control`].
    pub(crate) fn render_image(&mut self, target: &control_auth::ConfinedImage) -> (u32, u32) {
        let Some(front) = self.frontmost_window else {
            return (0, 0);
        };
        let (rows, cols) = match self.windows.get(&front) {
            Some(ws) => (ws.rows as usize, ws.cols as usize),
            None => return (0, 0),
        };
        // Lock only to snapshot the grid; render without the lock.
        {
            let Some(ws) = self.windows.get_mut(&front) else {
                return (0, 0);
            };
            let mut term = term_lock(&ws.term);
            // REFILL the reused snapshot in place (no per-frame container-Vec alloc).
            // A-3: the ENGINE builds the snapshot (`Terminal::cell_frame_into`).
            term.cell_frame_into(&mut ws.input_scratch, rows, cols);
        }
        // WYSIWYG: splice the tab strip above the terminal grid so the `image` verb
        // matches the glass (a no-op when the strip is disabled). Before the borrow.
        self.splice_tab_strip(front);
        // Sample the interval-driven panels here too: headless has no window tick,
        // so this is what makes CPU/net/app-fed live in `image`/`snapshot` output.
        let hud_now = Instant::now();
        for p in &mut self.panels {
            p.poll(hud_now);
        }
        self.splice_hud_bar(front);
        // Disjoint borrows: `self.backend` (renderer), the introspection GPU
        // scratch, and the front window's input_scratch are separate fields.
        let App {
            backend,
            introspect_gpu,
            windows,
            ..
        } = self;
        let Some(ws) = windows.get_mut(&front) else {
            return (0, 0);
        };
        // Time the rasterization so the `metrics` verb reports a real
        // `last_frame_render_ms` in HEADLESS mode too. On-screen frames are timed in
        // `redraw_window`; without this, headless (no OS surface → no
        // RedrawRequested → no `record_present`) leaves every counter frozen at 0,
        // so a perf audit driven over the control socket could measure nothing.
        // Present latency is recorded as 0 — honest: the `image` verb rasterizes to
        // a buffer, it does not present on glass.
        let render_t0 = Instant::now();
        let mut frame = backend.render_input(introspect_gpu, &ws.input_scratch);
        let render_ns = render_t0.elapsed().as_nanos() as u64;
        crate::metrics::record_present(0, render_ns);
        // I-2: match the on-screen visual-bell invert (see `snapshot`) so the
        // `image` verb is WYSIWYG even during a bell flash.
        apply_bell_invert(&mut frame, ws.bell_flash.is_active(Instant::now()));
        // `confine_image_path` (control thread) produced `target` as a canonical
        // `images/` dir + a SINGLE filename, forbidding nested target dirs. We
        // write by opening THAT directory `O_DIRECTORY|O_NOFOLLOW` and
        // `openat`-ing the final component `O_NOFOLLOW|O_CREAT|O_TRUNC` — so the
        // only guarantee we rely on is: the write lands in the canonical images
        // dir and never follows a symlink at the directory OR the final name.
        // (We do NOT claim atomicity vs. a same-uid client deleting+recreating
        // the directory between threads; we DO close the intermediate-dir
        // symlink-swap window by never re-resolving a multi-segment path string.)
        let _ = snapshot_path::write_private_at(&target.dir, &target.file_name, &frame.to_png());
        let (w, h) = (frame.width as u32, frame.height as u32);
        // Feed the frame-coupled panels (Perf) AFTER the disjoint-field borrows above
        // end (the destructure held `windows`/`backend`; `self.panels` is separate).
        let hud_now = Instant::now();
        for p in &mut self.panels {
            p.on_present(render_ns, 0, hud_now);
        }
        (w, h)
    }

    /// Read the frontmost window's NATIVE macOS chrome — the window's `NSToolbar`
    /// items and the application menu bar — into human-readable text lines for the
    /// `chrome` introspection verb. Runs on the MAIN thread (the SOLE place AppKit
    /// objects may be touched), driven by [`Wake::ReadChrome`]; the control thread
    /// posts that and blocks on the reply.
    ///
    /// This is the ONLY introspection path that sees OS chrome: `image`/`text`
    /// render just the terminal content view, never the toolbar or menu bar, so a
    /// driving AI uses `chrome` to confirm e.g. the "+" New Tab toolbar button and
    /// the menu structure. Pure read: it only CALLS getters (`toolbar()`/`items()`/
    /// `itemIdentifier()`/`label()`, `mainMenu()`/`itemArray()`/`title()`/
    /// `submenu()`), never mutating AppKit state.
    ///
    /// Off macOS there is no native chrome, so it returns a single explanatory line.
    #[cfg(target_os = "macos")]
    pub(crate) fn read_native_chrome(&self) -> Vec<String> {
        use objc2_app_kit::{NSApplication, NSToolbarDisplayMode, NSView, NSWindowToolbarStyle};
        use objc2_foundation::MainThreadMarker;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let mut out: Vec<String> = Vec::new();

        // We are on the winit main-loop thread (this runs via `user_event`), so the
        // marker is always present; bail gracefully if somehow not.
        let Some(mtm) = MainThreadMarker::new() else {
            out.push("ERR not on main thread".to_string());
            return out;
        };

        // --- The frontmost window's NSToolbar ---------------------------------
        // Reach the NSWindow the SAME way `match_window_colorspace_to_content` /
        // `toolbar::install_window_toolbar` do: winit Window -> AppKit
        // RawWindowHandle -> NSView -> NSWindow.
        let ns_window = self
            .front()
            .and_then(|ws| ws.os_window.as_ref())
            .and_then(|w| w.window_handle().ok())
            .and_then(|handle| match handle.as_raw() {
                // SAFETY: `ns_view` points at the front window's live NSView (owned
                // by winit for the window's lifetime); we only borrow it on the main
                // thread, as AppKit requires, to read its `window`.
                RawWindowHandle::AppKit(h) => {
                    let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
                    view.window()
                }
                _ => None,
            });

        // SAFETY: all the AppKit getters below (`toolbar`/`toolbarStyle`/
        // `displayMode`/`items`/`itemIdentifier`/`label`) are plain side-effect-free
        // accessors with no preconditions beyond a live receiver, called here on the
        // MAIN thread (this method runs only via `Wake::ReadChrome` in `user_event`).
        unsafe {
            match ns_window.as_deref().and_then(|w| w.toolbar()) {
                Some(toolbar) => {
                    let style = match ns_window.as_deref().map(|w| w.toolbarStyle()) {
                        Some(NSWindowToolbarStyle::Automatic) => "automatic",
                        Some(NSWindowToolbarStyle::Expanded) => "expanded",
                        Some(NSWindowToolbarStyle::Preference) => "preference",
                        Some(NSWindowToolbarStyle::Unified) => "unified",
                        Some(NSWindowToolbarStyle::UnifiedCompact) => "unified-compact",
                        _ => "?",
                    };
                    let display_mode = match toolbar.displayMode() {
                        NSToolbarDisplayMode::IconOnly => "icon-only",
                        NSToolbarDisplayMode::LabelOnly => "label-only",
                        NSToolbarDisplayMode::IconAndLabel => "icon-and-label",
                        _ => "default",
                    };
                    let items = toolbar.items();
                    out.push(format!(
                        "toolbar style={style} displayMode={display_mode} items={}",
                        items.len()
                    ));
                    for item in &items {
                        let id = item.itemIdentifier();
                        let label = item.label();
                        out.push(format!("toolbar-item id={id} label={label:?}"));
                    }
                }
                None => out.push("toolbar (none)".to_string()),
            }
        }

        // The native TAB SWITCHER: if the front window's `aterm.tabs` toolbar item is
        // present (2+ tabs — it is removed at ≤1), emit its NSSegmentedControl's
        // segmentCount / selectedSegment / per-segment labels so the tabs are
        // INTROSPECTABLE (a single-tab window emits NO `toolbar-tabs` line, mirroring
        // the hidden switcher). Read off the retained handle (we own the control), not
        // via a toolbar-item view downcast (objc2 0.5 has no `Retained::downcast`).
        if let Some(handle) = self.frontmost_window.and_then(|w| self._toolbars.get(&w))
            && let Some(line) = self.apprt.read_toolbar_chrome(handle)
        {
            out.push(line);
        }

        // --- The application menu bar (NSApplication.mainMenu) ----------------
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: `mainMenu`/`itemArray`/`title`/`submenu` are side-effect-free
        // getters with no preconditions beyond a live receiver, on the main thread.
        unsafe {
            match app.mainMenu() {
                Some(main) => {
                    for top in &main.itemArray() {
                        let title = top.title();
                        match top.submenu() {
                            Some(sub) => {
                                let names: Vec<String> = sub
                                    .itemArray()
                                    .iter()
                                    // Skip separators (empty title) so the listing
                                    // reads as the command set, not the dividers.
                                    .filter(|i| !i.title().is_empty())
                                    .map(|i| i.title().to_string())
                                    .collect();
                                out.push(format!("menu {title:?}: {}", names.join(", ")));
                            }
                            // A top-level item with no submenu (uncommon for a bar).
                            None => out.push(format!("menu {title:?}: (no submenu)")),
                        }
                    }
                }
                None => out.push("menu (none)".to_string()),
            }
        }

        out
    }

    /// Off macOS there is no native window chrome (no `NSToolbar` / `NSMenu`), so
    /// the `chrome` verb reports that plainly. Kept as a method on every target so
    /// the [`Wake::ReadChrome`] handler is platform-independent.
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn read_native_chrome(&self) -> Vec<String> {
        vec!["OK (no native chrome on this platform)".to_string()]
    }

    /// Capture the frontmost window's ENTIRE on-screen pixels — the native OS
    /// chrome (titlebar, traffic lights, the unified toolbar, the full-width tab
    /// strip) AND the terminal content — to a PNG at the CONFINED `target`, and
    /// return the captured `(width, height)`. Serves the control socket's `window`
    /// verb; runs on the MAIN thread per [`Wake::CaptureWindow`].
    ///
    /// This is fundamentally different from [`Self::render_image`] (the `image`
    /// verb): `image` rasterizes only the terminal content framebuffer via the
    /// renderer, with NO OS chrome. `window` reaches the front window's real
    /// `NSWindow`, resolves its `windowNumber()` (a CGWindowID), and asks
    /// CoreGraphics' window server for the actual composited on-screen pixels —
    /// the only way an AI driving aterm can SEE the whole window. So the captured
    /// height is GREATER than `image`'s (it includes the titlebar + tab strip).
    ///
    /// Returns `Err(msg)` (never panics) when there is no front OS window (headless),
    /// or the CoreGraphics capture fails — most commonly because macOS Screen
    /// Recording permission has not been granted (the verb surfaces that as a clear,
    /// actionable error so the user can grant it and retry).
    #[cfg(target_os = "macos")]
    pub(crate) fn capture_window(
        &self,
        target: &control_auth::ConfinedImage,
    ) -> Result<(u32, u32), String> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        // Reach the front window's NSView the SAME way `read_native_chrome` /
        // `match_window_colorspace_to_content` / `toolbar::install_window_toolbar`
        // do: winit Window -> AppKit RawWindowHandle -> NSView. `None` here means
        // there is no attached OS surface — i.e. headless — so the capture has no
        // window to photograph.
        let Some(os_window) = self.front().and_then(|ws| ws.os_window.as_ref()) else {
            return Err("no window to capture (headless)".to_string());
        };
        let Ok(handle) = os_window.window_handle() else {
            return Err("no window to capture (headless)".to_string());
        };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return Err("no window to capture (headless)".to_string());
        };
        // SAFETY: `ns_view` points at the front window's live NSView (owned by winit
        // for the window's lifetime); we only borrow it on the main thread, as AppKit
        // requires, to read its `window` and the window's `windowNumber`.
        let view: &objc2_app_kit::NSView =
            unsafe { &*(h.ns_view.as_ptr() as *const objc2_app_kit::NSView) };
        let Some(ns_window) = view.window() else {
            return Err("no window to capture (headless)".to_string());
        };
        // `windowNumber()` is the CGWindowID the window server knows this NSWindow
        // by — the handle `CGWindowListCreateImage` keys off. A negative / zero number
        // means the window is off-screen / not yet committed; treat as uncapturable.
        // SAFETY: a side-effect-free accessor on the live front-window `NSWindow`,
        // called on the main thread (this runs only via `Wake::CaptureWindow`).
        let window_number = unsafe { ns_window.windowNumber() };
        if window_number <= 0 {
            return Err(
                "window capture failed (front window has no on-screen window number)".to_string(),
            );
        }

        // Off to CoreGraphics: photograph the composited on-screen pixels, encode to
        // RGBA8, and write the PNG to the confined target. Any failure (most commonly
        // a missing Screen Recording grant) returns a clear `Err`.
        let (rgba, w, h) = capture_window_pixels(window_number as u32)?;

        // Confined write, identical to `render_image`'s: `openat` the final component
        // under the canonical `images/` dir fd (`O_NOFOLLOW`), so no intermediate
        // path component can be symlink-swapped after `confine_image_path`'s check.
        let png = encode_rgba8_png(&rgba, w, h)
            .map_err(|e| format!("window capture failed (PNG encode error: {e})"))?;
        snapshot_path::write_private_at(&target.dir, &target.file_name, &png)
            .map_err(|e| format!("window capture failed (write error: {e})"))?;
        Ok((w, h))
    }

    /// Off macOS there is no `CGWindowListCreateImage` / on-screen window server to
    /// photograph, so the `window` verb reports that plainly. Kept as a method on
    /// every target so the [`Wake::CaptureWindow`] handler is platform-independent.
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn capture_window(
        &self,
        _target: &control_auth::ConfinedImage,
    ) -> Result<(u32, u32), String> {
        Err("window capture is only available on macOS".to_string())
    }
}

/// Photograph the on-screen window with CoreGraphics window id `window_id` and
/// return its `(tightly-packed RGBA8 bytes, width, height)`. Runs on the MAIN
/// thread (called from [`App::capture_window`]).
///
/// Robust-format strategy (per the implementation note): rather than read the
/// source `CGImage`'s native, possibly-padded pixel layout, we draw it into a
/// freshly-created RGBA8 `CGBitmapContext` we own, then read THAT context's
/// tightly-packed buffer (`width * 4` stride, premultiplied-alpha-last). So the
/// bytes are always plain RGBA8 no matter what the window server hands us.
///
/// Returns `Err` (never panics / leaks) when CoreGraphics cannot capture — almost
/// always a missing Screen Recording grant, which the caller turns into the clear,
/// actionable permission error.
#[cfg(target_os = "macos")]
pub(crate) fn capture_window_pixels(window_id: u32) -> Result<(Vec<u8>, u32, u32), String> {
    use crate::cg_capture::*;

    // SAFETY: `CGWindowListCreateImage` is the documented capture entry point; we
    // pass `CGRectNull` (use the window's own bounds), the single-window option keyed
    // by `window_id`, and the ignore-framing | best-resolution image options. It
    // returns either a NEW CGImage we own (and release below) or NULL on failure.
    let image: CGImageRef = unsafe {
        CGWindowListCreateImage(
            CG_RECT_NULL,
            K_CG_WINDOW_LIST_OPTION_INCLUDING_WINDOW,
            window_id,
            K_CG_WINDOW_IMAGE_BOUNDS_IGNORE_FRAMING | K_CG_WINDOW_IMAGE_BEST_RESOLUTION,
        )
    };
    if image.is_null() {
        // The single most common cause is a missing Screen Recording grant; give the
        // exact, actionable remediation rather than a bare failure.
        return Err(
            "window capture failed (grant Screen Recording permission to aterm-gui in \
             System Settings > Privacy & Security > Screen Recording, then retry)"
                .to_string(),
        );
    }

    // From here on, `image` MUST be released on every path — use a tiny guard so an
    // early `?`/return cannot leak it. SAFETY: `image` is the live CGImage we just
    // created; `CGImageGetWidth/Height` are side-effect-free accessors on it.
    struct ImageGuard(CGImageRef);
    impl Drop for ImageGuard {
        fn drop(&mut self) {
            // SAFETY: `self.0` is the CGImage created above, released exactly once.
            unsafe { CGImageRelease(self.0) };
        }
    }
    let _image_guard = ImageGuard(image);

    let width = unsafe { CGImageGetWidth(image) };
    let height = unsafe { CGImageGetHeight(image) };
    if width == 0 || height == 0 {
        return Err("window capture failed (captured image has zero size)".to_string());
    }

    let bytes_per_row = width
        .checked_mul(BYTES_PER_PIXEL)
        .ok_or_else(|| "window capture failed (image too large)".to_string())?;

    // SAFETY: standard CG calls. `CGColorSpaceCreateDeviceRGB` returns a new colour
    // space we release below. `CGBitmapContextCreate` with NULL data + RGBA8 /
    // premultiplied-last creates a context whose backing buffer CG allocates and
    // owns until we release the context; we read it (via `CGBitmapContextGetData`)
    // strictly before that release.
    let color_space: CGColorSpaceRef = unsafe { CGColorSpaceCreateDeviceRGB() };
    if color_space.is_null() {
        return Err("window capture failed (could not create RGB color space)".to_string());
    }
    struct CsGuard(CGColorSpaceRef);
    impl Drop for CsGuard {
        fn drop(&mut self) {
            // SAFETY: the colour space created above, released exactly once.
            unsafe { CGColorSpaceRelease(self.0) };
        }
    }
    let _cs_guard = CsGuard(color_space);

    let context: CGContextRef = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            BITS_PER_COMPONENT,
            bytes_per_row,
            color_space,
            K_CG_IMAGE_ALPHA_PREMULTIPLIED_LAST,
        )
    };
    if context.is_null() {
        return Err("window capture failed (could not create bitmap context)".to_string());
    }
    struct CtxGuard(CGContextRef);
    impl Drop for CtxGuard {
        fn drop(&mut self) {
            // SAFETY: the context created above, released exactly once. Its backing
            // buffer is freed here — AFTER we have already copied the bytes out.
            unsafe { CGContextRelease(self.0) };
        }
    }
    let _ctx_guard = CtxGuard(context);

    // Draw the captured image to fill the whole context, normalizing it to our
    // known RGBA8 layout. SAFETY: `context` and `image` are both live objects we
    // created; the rect spans the full context.
    let full = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: width as f64,
            height: height as f64,
        },
    };
    unsafe { CGContextDrawImage(context, full, image) };

    // Read the tightly-packed RGBA8 bytes back out. SAFETY: `CGBitmapContextGetData`
    // returns a pointer to the context's backing buffer (valid until the context is
    // released, which the guard does only AFTER this copy). We copy exactly
    // `bytes_per_row * height` bytes — the buffer's full size for our chosen stride.
    let data_ptr = unsafe { CGBitmapContextGetData(context) } as *const u8;
    if data_ptr.is_null() {
        return Err("window capture failed (bitmap context has no data)".to_string());
    }
    let total = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| "window capture failed (image too large)".to_string())?;
    // SAFETY: `data_ptr` is the context's backing buffer of exactly `total` bytes
    // (width*4 stride, no extra padding — CG honours the stride we requested).
    let rgba = unsafe { std::slice::from_raw_parts(data_ptr, total) }.to_vec();

    Ok((rgba, width as u32, height as u32))
}

/// Encode a tightly-packed RGBA8 buffer (`width * height * 4` bytes, no row
/// padding) to PNG bytes, reusing the same `png` crate the `image` verb's
/// framebuffer path uses. Used by the `window` capture verb.
#[cfg(target_os = "macos")]
pub(crate) fn encode_rgba8_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}
