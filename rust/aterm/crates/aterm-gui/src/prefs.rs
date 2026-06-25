// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The native macOS PREFERENCES window (App ▸ Preferences…, ⌘,).
//!
//! aterm's settings live in `~/.config/aterm/aterm.toml` (hot-reloading — see
//! [`crate::app_config`]). The Preferences window is a thin, READ-ONLY surface over
//! that file: it shows the EFFECTIVE config — font size, theme, cursor style,
//! scrollback limit, font family, copy-on-select — as a list of label/value rows,
//! plus two actions:
//!   * **Open aterm.toml** — open the config file in the default editor (the same
//!     [`crate::menu::open_config_file`] the menu item used to call directly, which
//!     also seeds a documented starter file the first time); and
//!   * **Reload config** — post the SAME [`Wake::ConfigReload`](crate::Wake) the
//!     config-watcher fires, so the live hot-reload path ([`crate::App::reload_config`])
//!     re-reads + re-applies the file. No parallel reload logic lives here.
//!
//! Like `menu.rs` / `toolbar.rs`, the window adds NO new behavior: every button is a
//! thin DISPATCH stub onto an existing path. In-window EDITING is intentionally out of
//! scope — the file is the source of truth and edits flow through the hot-reload path.
//!
//! The displayed rows are factored into the PURE [`preferences_rows`] function so the
//! label/value mapping is unit-tested without any AppKit. The objc2 window code is
//! `#[cfg(target_os = "macos")]` and modelled on `toolbar.rs` (non-raising factory
//! initializers, every call on the main thread behind a `MainThreadMarker`); off macOS
//! [`open_preferences`] is a graceful no-op so the workspace builds everywhere.

use crate::app_config::Config;

/// Default scrollback line cap when `scrollback_lines` is unset (mirrors the engine
/// `TerminalConfig.scrollback_limit` default documented in [`Config`]). Shown so the
/// Preferences row reflects what the terminal actually uses, not a blank.
const DEFAULT_SCROLLBACK_LINES: usize = 100_000;

/// The default cursor style label when `cursor_style` is unset (the engine default is
/// a block cursor — see [`Config::cursor_style`]).
const DEFAULT_CURSOR_STYLE: &str = "block";

/// Build the label/value rows the Preferences window displays for `cfg`, surfacing the
/// EFFECTIVE setting for each field — the configured value, or its built-in default
/// rendered explicitly (with a "(default)" marker) so an unset key reads as a real
/// resolved value rather than a blank.
///
/// PURE + TESTABLE: this is the single source of truth for what the window shows, kept
/// free of AppKit so the mapping is unit-tested. The window code just renders these
/// pairs as rows.
///
/// Surfaced fields (in order): font size, font family, theme, cursor style, scrollback
/// limit, copy-on-select.
pub(crate) fn preferences_rows(cfg: &Config) -> Vec<(String, String)> {
    let mut rows = Vec::with_capacity(6);

    // Font size — the configured physical px (matching `Config::font_px`), or
    // "(default)" when auto-derived. The live post-Retina-scale size is a render
    // detail; the config row shows intent.
    rows.push((
        "Font size".to_string(),
        match cfg.font_px {
            Some(px) => format!("{px} px"),
            None => "(default)".to_string(),
        },
    ));

    // Font family — the configured family, or the built-in candidate chain.
    rows.push((
        "Font family".to_string(),
        match cfg.font_family.as_deref() {
            Some(f) if !f.trim().is_empty() => f.to_string(),
            _ => "(default)".to_string(),
        },
    ));

    // Theme — the configured scheme name (possibly a `dark:…,light:…` split, shown
    // verbatim), or the built-in Default scheme.
    rows.push((
        "Theme".to_string(),
        match cfg.theme.as_deref() {
            Some(t) if !t.trim().is_empty() => t.trim().to_string(),
            _ => "Default".to_string(),
        },
    ));

    // Cursor style — the configured shape, or the block default.
    rows.push((
        "Cursor style".to_string(),
        match cfg.cursor_style.as_deref() {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => format!("{DEFAULT_CURSOR_STYLE} (default)"),
        },
    ));

    // Scrollback limit — N lines, "unlimited" at 0, or the default cap when unset.
    rows.push((
        "Scrollback limit".to_string(),
        match cfg.scrollback_lines {
            Some(0) => "unlimited".to_string(),
            Some(n) => format!("{n} lines"),
            None => format!("{DEFAULT_SCROLLBACK_LINES} lines (default)"),
        },
    ));

    // Copy-on-select — the resolved boolean (default off).
    rows.push((
        "Copy on select".to_string(),
        if cfg.copy_on_select_or_default() {
            "on".to_string()
        } else {
            "off".to_string()
        },
    ));

    rows
}

#[cfg(target_os = "macos")]
pub use macos::{PrefsHandle, open_preferences};

/// Non-macOS no-op handle: there is no native Preferences window off macOS. Held by
/// `App` in the same field on every target so the struct shape is platform-independent
/// (mirrors `menu::MenuHandle` / `toolbar::ToolbarHandle`).
#[cfg(not(target_os = "macos"))]
pub type PrefsHandle = ();

/// Non-macOS stub: there is no native window toolkit wired here off macOS, so opening
/// Preferences falls back to opening the config file (a no-op itself off macOS — see
/// [`crate::menu::open_config_file`]). Returns `Option<PrefsHandle>` so the call site in
/// `dispatch_menu_action` is identical on every target.
#[cfg(not(target_os = "macos"))]
pub fn open_preferences(
    _proxy: &winit::event_loop::EventLoopProxy<crate::Wake>,
    _rows: &[(String, String)],
) -> Option<PrefsHandle> {
    crate::menu::open_config_file();
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
    use objc2::{ClassType, DeclaredClass, declare_class, msg_send_id, mutability, sel};
    use objc2_app_kit::{
        NSBackingStoreType, NSButton, NSColor, NSFont, NSTextField, NSView, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{CGFloat, CGPoint, CGRect, CGSize, MainThreadMarker, NSString};
    use winit::event_loop::EventLoopProxy;

    use crate::Wake;

    /// Window geometry (points). A compact fixed-size utility panel — wide enough for a
    /// "Scrollback limit" label + a long value, tall enough for the rows + the button
    /// row with comfortable margins. The window is built with MANUAL frame layout (no
    /// Auto Layout / NSStackView), exactly like `toolbar.rs`, so it needs no extra
    /// objc2-app-kit feature and no raising initializer.
    const WIN_W: CGFloat = 440.0;
    const WIN_H: CGFloat = 340.0;
    /// Inset of the content from the window edges.
    const MARGIN: CGFloat = 20.0;
    /// Height (points) of one label/value row.
    const ROW_H: CGFloat = 20.0;
    /// Vertical gap between rows.
    const ROW_GAP: CGFloat = 6.0;
    /// Width (points) of a row's LABEL column, so the values align in a column.
    const LABEL_W: CGFloat = 140.0;
    /// Button row geometry.
    const BUTTON_H: CGFloat = 30.0;
    const BUTTON_W: CGFloat = 150.0;
    const BUTTON_GAP: CGFloat = 12.0;
    /// Height (points) of the header line above the rows.
    const HEADER_H: CGFloat = 22.0;

    /// What [`open_preferences`] returns: the retained backing objects. AppKit holds a
    /// window's button targets only WEAKLY and a borderless retained `NSWindow` must be
    /// kept alive by its owner, so `App` stashes this in a field for the window's life.
    pub struct PrefsHandle {
        /// The Preferences `NSWindow`. Retained so it is not deallocated the moment this
        /// function returns (a freshly built window with no controller is owned by us).
        _window: Retained<NSWindow>,
        /// The button action target (owns the `EventLoopProxy<Wake>`). The buttons
        /// reference their target only weakly, so retain it here.
        _target: Retained<PrefsTarget>,
    }

    // SAFETY: `PrefsHandle` is only ever created, read, and dropped on the main thread
    // (the event loop). It holds main-thread-only AppKit objects; `App` stores it in a
    // field and never sends it across threads. We add no unsafe Send/Sync — the
    // auto-derived non-Send is the safe default.

    declare_class!(
        /// The target object for the Preferences window's two buttons. Owns the
        /// `EventLoopProxy<Wake>` and exposes two selectors:
        ///   * `openConfigFile:` — open `aterm.toml` in the default editor (directly,
        ///     via `crate::menu::open_config_file`), and
        ///   * `reloadConfig:` — post `Wake::ConfigReload` so the live hot-reload runs.
        pub(crate) struct PrefsTarget;

        // SAFETY:
        // - NSObject imposes no subclassing requirements.
        // - InteriorMutable is the safe default; we never mutate the ivars.
        // - PrefsTarget has no Drop impl beyond the auto-generated ivar drop.
        unsafe impl ClassType for PrefsTarget {
            type Super = objc2::runtime::NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "ATermPrefsTarget";
        }

        impl DeclaredClass for PrefsTarget {
            type Ivars = EventLoopProxy<Wake>;
        }

        unsafe impl NSObjectProtocol for PrefsTarget {}

        unsafe impl PrefsTarget {
            /// `openConfigFile:` — open `~/.config/aterm/aterm.toml` in the default
            /// editor (seeding a documented starter file the first time), via the SAME
            /// `crate::menu::open_config_file` the Preferences menu item used to call.
            /// A synchronous AppKit `NSWorkspace` call, fine on the main thread.
            #[method(openConfigFile:)]
            fn open_config_file(&self, _sender: Option<&AnyObject>) {
                crate::menu::open_config_file();
            }

            /// `reloadConfig:` — post the SAME `Wake::ConfigReload` the config-watcher
            /// fires, so the main loop runs the live hot-reload (`App::reload_config`).
            /// Fire-and-forget: a closed loop just drops it (mirrors every relay here).
            #[method(reloadConfig:)]
            fn reload_config(&self, _sender: Option<&AnyObject>) {
                let _ = self.ivars().send_event(Wake::ConfigReload);
            }
        }
    );

    impl PrefsTarget {
        /// Allocate a button target owning `proxy`. `mtm` proves we are on the main
        /// thread (AppKit requirement), which the winit loop guarantees.
        fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<Wake>) -> Retained<Self> {
            let this = mtm.alloc().set_ivars(proxy);
            // SAFETY: plain `[super init]` on a freshly allocated instance.
            unsafe { msg_send_id![super(this), init] }
        }
    }

    /// Open (or re-open) the native Preferences window showing `rows` (label/value pairs
    /// from [`super::preferences_rows`]) plus the Open-file / Reload buttons. Returns the
    /// retained backing objects for `App` to keep alive (AppKit holds button targets, and
    /// a controller-less window, only weakly/by-owner).
    ///
    /// Best-effort: off the main thread the window is simply not built (`None`) — never a
    /// panic. A fresh window is built each call (the previous handle is dropped by the
    /// caller, closing the old window) — simple, and the read-only content is cheap.
    pub fn open_preferences(
        proxy: &EventLoopProxy<Wake>,
        rows: &[(String, String)],
    ) -> Option<PrefsHandle> {
        let mtm = MainThreadMarker::new()?;
        let target = PrefsTarget::new(mtm, proxy.clone());

        // The window: a titled, closable, fixed-size utility panel.
        // SAFETY: `initWithContentRect:styleMask:backing:defer:` is the documented
        // designated initializer (non-raising); plain setters follow on the fresh
        // instance, on the main thread (`mtm`). The content rect is a valid CGRect.
        let window = unsafe {
            let content_rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(WIN_W, WIN_H));
            let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                content_rect,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            );
            window.setTitle(&NSString::from_str("aterm Preferences"));
            // We own the window via the returned handle; do NOT let AppKit free it on
            // close, or dropping the handle would double-free.
            window.setReleasedWhenClosed(false);
            window
        };

        // Manual top-down layout in the content view's FLIPPED-free (origin bottom-left)
        // coordinate space: we compute each row's y from the TOP down by subtracting from
        // the content height, matching `toolbar.rs`'s explicit-frame approach (no Auto
        // Layout, so no extra objc2 feature). `y` tracks the next row's TOP edge.
        let content_w = WIN_W - 2.0 * MARGIN;
        let mut y = WIN_H - MARGIN - HEADER_H;

        // Header line.
        let header = make_label(mtm, "Effective configuration", true);
        place(&header, MARGIN, y, content_w, HEADER_H);
        y -= HEADER_H + ROW_GAP;

        let mut subviews: Vec<Retained<NSView>> = Vec::with_capacity(rows.len() * 2 + 3);
        // NSTextField -> NSControl -> NSView: two `into_super` lifts to the common view
        // type so a heterogeneous (label + button) set lives in one Vec.
        subviews.push(to_view_label(header));

        // One row per setting: a fixed-width label column + a dim value column.
        for (label, value) in rows {
            let label_field = make_label(mtm, label, false);
            place(&label_field, MARGIN, y, LABEL_W, ROW_H);
            let value_field = make_label(mtm, value, false);
            // SAFETY: `secondaryLabelColor` + `setTextColor:` are plain main-thread calls.
            unsafe {
                value_field.setTextColor(Some(&NSColor::secondaryLabelColor()));
            }
            let value_x = MARGIN + LABEL_W;
            place(&value_field, value_x, y, content_w - LABEL_W, ROW_H);
            y -= ROW_H + ROW_GAP;
            subviews.push(to_view_label(label_field));
            subviews.push(to_view_label(value_field));
        }

        // The button row, pinned near the bottom: Open aterm.toml | Reload config.
        let open = make_button(mtm, "Open aterm.toml", &target, sel!(openConfigFile:));
        place(&open, MARGIN, MARGIN, BUTTON_W, BUTTON_H);
        let reload = make_button(mtm, "Reload config", &target, sel!(reloadConfig:));
        place(
            &reload,
            MARGIN + BUTTON_W + BUTTON_GAP,
            MARGIN,
            BUTTON_W,
            BUTTON_H,
        );
        // NSButton -> NSControl -> NSView (two levels), like NSTextField above.
        subviews.push(to_view_button(open));
        subviews.push(to_view_button(reload));

        // Attach every subview to the window's content view, then show + center.
        // SAFETY: `contentView` is non-null for a titled window; `addSubview:` /
        // `center` / `makeKeyAndOrderFront:` are plain main-thread calls; the views were
        // all just built on this thread.
        unsafe {
            if let Some(content) = window.contentView() {
                for v in &subviews {
                    content.addSubview(v);
                }
            }
            window.center();
            window.makeKeyAndOrderFront(None);
        }

        Some(PrefsHandle {
            _window: window,
            _target: target,
        })
    }

    /// Set a view's frame to `(x, y, w, h)` (points, origin bottom-left). A thin wrapper
    /// over `setFrame:` so the layout reads as a sequence of placements.
    fn place(view: &NSView, x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat) {
        let frame = CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        // SAFETY: `setFrame:` is a plain main-thread setter on the live view.
        unsafe { view.setFrame(frame) };
    }

    /// Lift an `NSTextField` to its common `NSView` superclass (NSTextField -> NSControl
    /// -> NSView) so labels + buttons share one `Vec<Retained<NSView>>`.
    fn to_view_label(field: Retained<NSTextField>) -> Retained<NSView> {
        Retained::into_super(Retained::into_super(field))
    }

    /// Lift an `NSButton` to its common `NSView` superclass (NSButton -> NSControl ->
    /// NSView).
    fn to_view_button(button: Retained<NSButton>) -> Retained<NSView> {
        Retained::into_super(Retained::into_super(button))
    }

    /// Build a non-editable label (`labelWithString:` — the documented non-raising
    /// factory). `bold` selects a bold system font (for the header).
    fn make_label(mtm: MainThreadMarker, text: &str, bold: bool) -> Retained<NSTextField> {
        // SAFETY: `labelWithString:` is the documented non-raising factory; plain setters
        // follow on the fresh label; all on the main thread.
        unsafe {
            let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
            field.setDrawsBackground(false);
            field.setBezeled(false);
            field.setEditable(false);
            field.setSelectable(false);
            let size: CGFloat = 13.0;
            let font = if bold {
                NSFont::boldSystemFontOfSize(size)
            } else {
                NSFont::systemFontOfSize(size)
            };
            field.setFont(Some(&font));
            field
        }
    }

    /// Build a bordered push button titled `title` targeting `action` on `target`. Uses
    /// the documented non-raising `buttonWithTitle:target:action:` factory.
    fn make_button(
        mtm: MainThreadMarker,
        title: &str,
        target: &PrefsTarget,
        action: Sel,
    ) -> Retained<NSButton> {
        // SAFETY: `buttonWithTitle:target:action:` is the documented factory initializer;
        // the `target` outlives the call (retained in the handle); on the main thread.
        unsafe {
            let target_obj: &AnyObject = target;
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(target_obj),
                Some(action),
                mtm,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::preferences_rows;
    use crate::app_config::Config;

    fn cfg(toml: &str) -> Config {
        toml::from_str(toml).expect("valid toml")
    }

    /// An empty config surfaces every field at its built-in DEFAULT, rendered
    /// explicitly (never a blank), in the documented order.
    #[test]
    fn defaults_are_surfaced_explicitly() {
        let rows = preferences_rows(&Config::default());
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Font size",
                "Font family",
                "Theme",
                "Cursor style",
                "Scrollback limit",
                "Copy on select",
            ]
        );
        // Each default value is a concrete string, not empty.
        for (label, value) in &rows {
            assert!(!value.is_empty(), "{label} value must not be blank");
        }
        let find = |k: &str| rows.iter().find(|(l, _)| l == k).map(|(_, v)| v.as_str());
        assert_eq!(find("Font size"), Some("(default)"));
        assert_eq!(find("Font family"), Some("(default)"));
        assert_eq!(find("Theme"), Some("Default"));
        assert_eq!(find("Cursor style"), Some("block (default)"));
        assert_eq!(find("Scrollback limit"), Some("100000 lines (default)"));
        assert_eq!(find("Copy on select"), Some("off"));
    }

    /// A fully-populated config surfaces each configured value verbatim.
    #[test]
    fn configured_values_are_surfaced() {
        let c = cfg("font_px = 15.0\n\
             font_family = \"JetBrains Mono\"\n\
             theme = \"Dracula\"\n\
             cursor_style = \"bar\"\n\
             scrollback_lines = 50000\n\
             copy_on_select = true\n");
        let rows = preferences_rows(&c);
        let find = |k: &str| rows.iter().find(|(l, _)| l == k).map(|(_, v)| v.as_str());
        assert_eq!(find("Font size"), Some("15 px"));
        assert_eq!(find("Font family"), Some("JetBrains Mono"));
        assert_eq!(find("Theme"), Some("Dracula"));
        assert_eq!(find("Cursor style"), Some("bar"));
        assert_eq!(find("Scrollback limit"), Some("50000 lines"));
        assert_eq!(find("Copy on select"), Some("on"));
    }

    /// `scrollback_lines = 0` reads as "unlimited" (the engine's unlimited sentinel),
    /// distinct from both an unset default and a finite cap.
    #[test]
    fn zero_scrollback_is_unlimited() {
        let c = cfg("scrollback_lines = 0");
        let rows = preferences_rows(&c);
        let v = rows
            .iter()
            .find(|(l, _)| l == "Scrollback limit")
            .map(|(_, v)| v.as_str());
        assert_eq!(v, Some("unlimited"));
    }

    /// A whitespace-only theme / family falls back to the default label rather than
    /// surfacing the blank string (so the row is never visually empty).
    #[test]
    fn blank_strings_fall_back_to_defaults() {
        let c = cfg("theme = \"   \"\nfont_family = \"\"");
        let rows = preferences_rows(&c);
        let find = |k: &str| rows.iter().find(|(l, _)| l == k).map(|(_, v)| v.as_str());
        assert_eq!(find("Theme"), Some("Default"));
        assert_eq!(find("Font family"), Some("(default)"));
    }

    /// A split `theme = "dark:…,light:…"` is shown verbatim (trimmed) — the window is a
    /// faithful mirror of the config string, not a resolved single scheme.
    #[test]
    fn split_theme_shown_verbatim() {
        let c = cfg("theme = \" dark:Dracula,light:GitHub Light \"");
        let rows = preferences_rows(&c);
        let v = rows
            .iter()
            .find(|(l, _)| l == "Theme")
            .map(|(_, v)| v.as_str());
        assert_eq!(v, Some("dark:Dracula,light:GitHub Light"));
    }
}
