// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The native macOS PREFERENCES window (App ▸ Preferences…, ⌘,).
//!
//! aterm's settings live in `~/.config/aterm/aterm.toml` (hot-reloading — see
//! [`crate::app_config`]). The Preferences window is an EDITABLE surface over that
//! file: it shows the core config — font size, font family, theme, cursor style,
//! scrollback limit, copy-on-select — in editable controls (text fields + a
//! checkbox), plus actions:
//!   * **Save** — read the edited control values, write them back NON-DESTRUCTIVELY
//!     into `aterm.toml` (preserving the user's other keys, comments, and formatting
//!     via [`apply_prefs_edits`] / `toml_edit`), then post the SAME
//!     [`Wake::ConfigReload`](crate::Wake) the config-watcher fires so the live
//!     hot-reload ([`crate::App::reload_config`]) re-applies the file;
//!   * **Open aterm.toml** — open the config file in the default editor (the same
//!     [`crate::menu::open_config_file`] the menu item used to call directly, which
//!     also seeds a documented starter file the first time); and
//!   * **Reload config** — post the SAME [`Wake::ConfigReload`](crate::Wake) the
//!     config-watcher fires, so the live hot-reload re-reads + re-applies the file.
//!     No parallel reload logic lives here.
//!
//! Clearing a field to blank REMOVES that key (reverting it to its built-in default)
//! rather than writing an empty string. Save is best-effort: a missing config file is
//! created, an unwritable one is logged, never a panic.
//!
//! The control-seeding mapping is factored into the PURE [`editable_fields`] function,
//! and the file-editing logic into the PURE, UNIT-TESTED [`apply_prefs_edits`] — both
//! AppKit-free, so they are the test safety net for the objc2 window code (which cannot
//! be unit-tested). The objc2 window code is
//! `#[cfg(target_os = "macos")]` and modelled on `toolbar.rs` (non-raising factory
//! initializers, every call on the main thread behind a `MainThreadMarker`); off macOS
//! [`open_preferences`] is a graceful no-op so the workspace builds everywhere.

use crate::app_config::Config;

/// The TOML keys the Preferences window edits, paired with how each should be TYPED
/// when written back ([`apply_prefs_edits`]). The order matches the on-screen row order
/// (see [`editable_fields`]). These are the exact `Config` field names so a Save
/// followed by a reload round-trips through serde (see [`crate::app_config::Config`]).
pub(crate) const EDIT_FONT_PX: &str = "font_px";
pub(crate) const EDIT_FONT_FAMILY: &str = "font_family";
pub(crate) const EDIT_THEME: &str = "theme";
pub(crate) const EDIT_CURSOR_STYLE: &str = "cursor_style";
pub(crate) const EDIT_SCROLLBACK: &str = "scrollback_lines";
pub(crate) const EDIT_COPY_ON_SELECT: &str = "copy_on_select";

/// How a Preferences key should be TYPED in the written TOML, so a Save round-trips
/// through `Config`'s serde types (font_px float, scrollback_lines int, copy_on_select
/// bool, the rest strings). Drives [`apply_prefs_edits`]'s value construction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EditKind {
    /// A floating-point number (`font_px`). Written as a TOML float.
    Float,
    /// A non-negative integer (`scrollback_lines`). Written as a TOML integer.
    Integer,
    /// A boolean (`copy_on_select`). Written as a TOML `true`/`false`.
    Bool,
    /// A free-form string (`theme`, `font_family`, `cursor_style`). Written as a
    /// TOML basic string.
    Text,
}

/// The TOML type of each editable key, so [`apply_prefs_edits`] can parse the raw
/// control text into a correctly-typed `toml_edit` value. The single source of truth
/// shared by the window (which builds the controls) and the writer (which types them).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn edit_kind(key: &str) -> EditKind {
    match key {
        EDIT_FONT_PX => EditKind::Float,
        EDIT_SCROLLBACK => EditKind::Integer,
        EDIT_COPY_ON_SELECT => EditKind::Bool,
        _ => EditKind::Text,
    }
}

/// An error from [`apply_prefs_edits`]: either the existing file is not valid TOML
/// (so a non-destructive edit can't be performed safely), or a supplied value does not
/// parse as the key's declared type. Both are surfaced (logged) by the caller, which
/// then leaves the file untouched rather than risk clobbering it.
#[derive(Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum PrefsEditError {
    /// The current `aterm.toml` text failed to parse as TOML (`toml_edit` error). The
    /// edit is refused so a malformed file is never overwritten.
    Parse(String),
    /// A control value did not parse as its key's declared [`EditKind`] (e.g. a
    /// non-numeric font size). Carries the offending `(key, raw)` for the message.
    BadValue { key: String, raw: String },
}

impl std::fmt::Display for PrefsEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrefsEditError::Parse(e) => write!(f, "existing aterm.toml is not valid TOML: {e}"),
            PrefsEditError::BadValue { key, raw } => {
                write!(f, "invalid value for {key}: {raw:?}")
            }
        }
    }
}

impl std::error::Error for PrefsEditError {}

/// Apply a set of Preferences edits to the CURRENT `aterm.toml` text NON-DESTRUCTIVELY,
/// returning the new text. PURE + UNIT-TESTED — no AppKit, no filesystem — so the
/// edit semantics are the test safety net for the (untestable) objc2 window.
///
/// `existing_toml` is the file's current contents (`""` for a missing file). `edits` is
/// a list of `(key, value)`:
///   * `Some(raw)` — SET `key` to `raw`, typed per [`edit_kind`] (`font_px` → float,
///     `scrollback_lines` → integer, `copy_on_select` → bool, the rest → string). An
///     existing key is UPDATED in place (its surrounding formatting/comment survives);
///     a new key is appended.
///   * `None` — REMOVE `key` (revert to its built-in default). Absent already ⇒ no-op.
///
/// Every OTHER key, every comment, and the document's formatting are PRESERVED, because
/// the edit goes through `toml_edit`'s format-preserving DOM, not a re-serialize. Only
/// the listed keys change.
///
/// Errors ([`PrefsEditError`]): the existing text isn't valid TOML, or a value doesn't
/// parse as its key's type — in both cases the caller leaves the file untouched.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn apply_prefs_edits(
    existing_toml: &str,
    edits: &[(&str, Option<String>)],
) -> Result<String, PrefsEditError> {
    let mut doc = existing_toml
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| PrefsEditError::Parse(e.to_string()))?;

    for (key, value) in edits {
        match value {
            // Blank/cleared → remove the key (revert to default). Absent ⇒ no-op.
            None => {
                doc.remove(key);
            }
            Some(raw) => {
                let item = typed_item(key, raw)?;
                doc[*key] = item;
            }
        }
    }

    Ok(doc.to_string())
}

/// Build the correctly-TYPED `toml_edit` item for `key` from its raw control text,
/// per [`edit_kind`]. A malformed numeric/bool is a [`PrefsEditError::BadValue`] so a
/// Save never writes a value the reload parser would reject.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn typed_item(key: &str, raw: &str) -> Result<toml_edit::Item, PrefsEditError> {
    use toml_edit::{Item, Value};
    let bad = || PrefsEditError::BadValue {
        key: key.to_string(),
        raw: raw.to_string(),
    };
    let trimmed = raw.trim();
    let value = match edit_kind(key) {
        EditKind::Float => Value::from(trimmed.parse::<f64>().map_err(|_| bad())?),
        EditKind::Integer => Value::from(trimmed.parse::<i64>().map_err(|_| bad())?),
        EditKind::Bool => Value::from(trimmed.parse::<bool>().map_err(|_| bad())?),
        // Strings keep the user's text verbatim (trimmed of surrounding whitespace) —
        // a `dark:…,light:…` split theme or a multi-word family round-trips unchanged.
        EditKind::Text => Value::from(trimmed),
    };
    Ok(Item::Value(value))
}

/// One editable field for the Preferences window: a human `label`, the `Config`/TOML
/// `key` it edits, its [`EditKind`] (so the window builds the right control and the
/// writer types the value), and the field's CURRENT raw value from the config.
///
/// `seed` is the user's CONFIGURED value (NOT the effective default): `None` for an
/// unset key so the control starts BLANK — clearing it back to blank then removes the
/// key on Save. For the bool field `seed` is `Some("true")`/`Some("false")` reflecting
/// the resolved state so the checkbox starts in the right position.
///
/// Off macOS the native window is never built (`open_preferences` falls back to opening
/// the file), so the `label`/`kind` fields the window would consume are unused there —
/// allow the dead-code only on non-macOS, keeping macOS strict.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct EditField {
    /// The on-screen row label.
    pub(crate) label: &'static str,
    /// The `aterm.toml` / `Config` key this row edits.
    pub(crate) key: &'static str,
    /// How the value is typed when written ([`apply_prefs_edits`]).
    pub(crate) kind: EditKind,
    /// The configured raw value to seed the control with (`None` = unset = blank).
    pub(crate) seed: Option<String>,
}

/// Build the editable field specs (label/key/kind/seed) the Preferences window renders
/// as controls, in the documented row order (font size, font family, theme, cursor
/// style, scrollback limit, copy-on-select). PURE + TESTABLE: the seeding logic (which
/// keys start blank vs. populated, and the bool's resolved state) is unit-tested
/// without AppKit; the window just maps each spec to a control.
///
/// This seeds the editor with the CONFIGURED raw value only — an unset key seeds `None`
/// so the control is blank and a Save of an untouched blank field removes nothing
/// (rather than materialising the effective default).
pub(crate) fn editable_fields(cfg: &Config) -> Vec<EditField> {
    vec![
        EditField {
            label: "Font size",
            key: EDIT_FONT_PX,
            kind: EditKind::Float,
            seed: cfg.font_px.map(|px| format!("{px}")),
        },
        EditField {
            label: "Font family",
            key: EDIT_FONT_FAMILY,
            kind: EditKind::Text,
            seed: cfg
                .font_family
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        },
        EditField {
            label: "Theme",
            key: EDIT_THEME,
            kind: EditKind::Text,
            seed: cfg
                .theme
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        },
        EditField {
            label: "Cursor style",
            key: EDIT_CURSOR_STYLE,
            kind: EditKind::Text,
            seed: cfg
                .cursor_style
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        },
        EditField {
            label: "Scrollback limit",
            key: EDIT_SCROLLBACK,
            kind: EditKind::Integer,
            seed: cfg.scrollback_lines.map(|n| n.to_string()),
        },
        EditField {
            label: "Copy on select",
            key: EDIT_COPY_ON_SELECT,
            kind: EditKind::Bool,
            // The checkbox always reflects the RESOLVED state (default off), so it
            // starts in the right position; Save writes the explicit bool.
            seed: Some(cfg.copy_on_select_or_default().to_string()),
        },
    ]
}

/// Persist a batch of Preferences edits to `aterm.toml` NON-DESTRUCTIVELY, then return
/// whether the file changed so the caller can post a reload only when it did.
///
/// Best-effort + never panics: a missing file is treated as empty (the keys are
/// created); a read or write error, or an `apply_prefs_edits` failure (malformed
/// existing file / bad value) is LOGGED and the file is left untouched. Returns `true`
/// only when a new file content was actually written (so an all-no-op Save is a true
/// no-op and skips the reload).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn save_prefs_edits(edits: &[(&str, Option<String>)]) -> bool {
    let Some(path) = crate::app_config::config_path() else {
        eprintln!("aterm-gui: prefs save: no config path (HOME/XDG unset); skipping");
        return false;
    };
    // A missing file is fine — start from empty and create it on write.
    let existing = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!(
                "aterm-gui: prefs save: {} unreadable ({e}); leaving config unchanged",
                path.display()
            );
            return false;
        }
    };
    let updated = match apply_prefs_edits(&existing, edits) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("aterm-gui: prefs save: {e}; leaving config unchanged");
            return false;
        }
    };
    if updated == existing {
        return false; // nothing actually changed — skip the write + reload
    }
    // Best-effort create-parent + write; an unwritable file is logged, never a panic.
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "aterm-gui: prefs save: cannot create {} ({e}); leaving config unchanged",
            parent.display()
        );
        return false;
    }
    // ATOMIC replace: write a sibling temp then rename over the target, so a crash
    // mid-write can never leave a truncated/partial aterm.toml holding the user's
    // config (mirrors the fs::rename idiom in control_auth.rs). The temp sits in the
    // same dir, so the rename stays on one filesystem (atomic).
    let tmp = path.with_extension("toml.tmp");
    match std::fs::write(&tmp, &updated).and_then(|()| std::fs::rename(&tmp, &path)) {
        Ok(()) => true,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp); // best-effort: don't leave a stray temp
            eprintln!(
                "aterm-gui: prefs save: {} unwritable ({e}); config unchanged",
                path.display()
            );
            false
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{PrefsHandle, open_preferences};

/// Non-macOS no-op handle: there is no native Preferences window off macOS. Held by
/// `App` in the same field on every target so the struct shape is platform-independent
/// (mirrors `menu::MenuHandle` / `toolbar::ToolbarHandle`).
#[cfg(not(target_os = "macos"))]
pub type PrefsHandle = ();

/// Non-macOS stub: there is no native window toolkit wired here off macOS, so opening
/// Preferences falls back to opening the config file for editing (itself a no-op off
/// macOS — see [`crate::menu::open_config_file`]); the editable window + its Save are
/// macOS-only. Returns `Option<PrefsHandle>` so the call site in `open_preferences` is
/// identical on every target. (`apply_prefs_edits` / `editable_fields` are still built
/// and unit-tested off macOS — only the AppKit window is gated out.)
#[cfg(not(target_os = "macos"))]
pub fn open_preferences(
    _proxy: &winit::event_loop::EventLoopProxy<crate::Wake>,
    _fields: &[EditField],
) -> Option<PrefsHandle> {
    crate::menu::open_config_file();
    None
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
    use objc2::{ClassType, DeclaredClass, declare_class, msg_send_id, mutability, sel};
    use objc2_app_kit::{
        NSBackingStoreType, NSButton, NSControlStateValueOn, NSFont, NSTextField, NSView, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{CGFloat, CGPoint, CGRect, CGSize, MainThreadMarker, NSString};
    use winit::event_loop::EventLoopProxy;

    use super::{EditField, EditKind};
    use crate::Wake;

    /// Window geometry (points). A compact fixed-size utility panel — wide enough for a
    /// "Scrollback limit" label + an editable field, tall enough for the rows + the
    /// button row with comfortable margins. The window is built with MANUAL frame layout
    /// (no Auto Layout / NSStackView), exactly like `toolbar.rs`, so it needs no extra
    /// objc2-app-kit feature and no raising initializer.
    const WIN_W: CGFloat = 440.0;
    const WIN_H: CGFloat = 340.0;
    /// Inset of the content from the window edges.
    const MARGIN: CGFloat = 20.0;
    /// Height (points) of one label/control row.
    const ROW_H: CGFloat = 24.0;
    /// Vertical gap between rows.
    const ROW_GAP: CGFloat = 8.0;
    /// Width (points) of a row's LABEL column, so the controls align in a column.
    const LABEL_W: CGFloat = 140.0;
    /// Button row geometry. Three buttons (Save | Open aterm.toml | Reload) fit the
    /// content width: `2·MARGIN + 3·BUTTON_W + 2·BUTTON_GAP = 40 + 372 + 20 = 432 ≤
    /// WIN_W (440)`.
    const BUTTON_H: CGFloat = 30.0;
    const BUTTON_W: CGFloat = 124.0;
    const BUTTON_GAP: CGFloat = 10.0;
    /// Height (points) of the header line above the rows.
    const HEADER_H: CGFloat = 22.0;

    /// One editable control kept live for the Save handler to read back. A text field
    /// (font size / family / theme / cursor style / scrollback) or the copy-on-select
    /// checkbox; both are read by their `key`'s [`EditKind`] in `saveConfig:`.
    enum Ctl {
        /// A text field — its `stringValue` is read; blank ⇒ remove the key.
        Text(Retained<NSTextField>),
        /// The copy-on-select checkbox — its on/off `state` is read as a bool.
        Check(Retained<NSButton>),
    }

    /// A live control paired with the TOML key it edits, so `saveConfig:` can read each
    /// value back. The value's TOML TYPE is decided at write time by `edit_kind(key)`
    /// inside [`super::apply_prefs_edits`], so no per-control kind is stored here.
    struct Saved {
        /// The `aterm.toml` key (a `'static str` from [`super`]'s `EDIT_*` consts).
        key: &'static str,
        /// The retained control to read at Save time.
        ctl: Ctl,
    }

    /// The ivars of [`PrefsTarget`]: the `Wake` channel for the relays (Save reload /
    /// Reload / nothing for Open) and the live editable controls Save reads back. The
    /// controls are in a `RefCell` because they are populated AFTER the target is
    /// allocated (the controls reference the target as their action), then only READ in
    /// `saveConfig:` — never mutated, just borrowed.
    pub(crate) struct PrefsIvars {
        /// The event-loop channel: Save (after writing) and Reload post `ConfigReload`.
        proxy: EventLoopProxy<Wake>,
        /// The editable controls, in row order, read by `saveConfig:`. Empty until the
        /// window is built (the controls target this object, so it exists first).
        controls: RefCell<Vec<Saved>>,
    }

    /// What [`open_preferences`] returns: the retained backing objects. AppKit holds a
    /// window's button/control targets only WEAKLY and a borderless retained `NSWindow`
    /// must be kept alive by its owner, so `App` stashes this in a field for the
    /// window's life.
    pub struct PrefsHandle {
        /// The Preferences `NSWindow`. Retained so it is not deallocated the moment this
        /// function returns (a freshly built window with no controller is owned by us).
        _window: Retained<NSWindow>,
        /// The button/control action target (owns the proxy AND the editable controls).
        /// The buttons reference their target only weakly, so retain it here.
        _target: Retained<PrefsTarget>,
    }

    // SAFETY: `PrefsHandle` is only ever created, read, and dropped on the main thread
    // (the event loop). It holds main-thread-only AppKit objects; `App` stores it in a
    // field and never sends it across threads. We add no unsafe Send/Sync — the
    // auto-derived non-Send is the safe default.

    declare_class!(
        /// The target object for the Preferences window's buttons. Owns the
        /// `EventLoopProxy<Wake>` + the editable controls and exposes three selectors:
        ///   * `saveConfig:` — read the edited control values, write them back to
        ///     `aterm.toml` non-destructively, then post `Wake::ConfigReload`;
        ///   * `openConfigFile:` — open `aterm.toml` in the default editor (directly,
        ///     via `crate::menu::open_config_file`), and
        ///   * `reloadConfig:` — post `Wake::ConfigReload` so the live hot-reload runs.
        pub(crate) struct PrefsTarget;

        // SAFETY:
        // - NSObject imposes no subclassing requirements.
        // - InteriorMutable is the safe default; the `RefCell` ivar is filled once
        //   (post-alloc) then only borrowed (read) in `saveConfig:`.
        // - PrefsTarget has no Drop impl beyond the auto-generated ivar drop.
        unsafe impl ClassType for PrefsTarget {
            type Super = objc2::runtime::NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "ATermPrefsTarget";
        }

        impl DeclaredClass for PrefsTarget {
            type Ivars = PrefsIvars;
        }

        unsafe impl NSObjectProtocol for PrefsTarget {}

        unsafe impl PrefsTarget {
            /// `saveConfig:` — read each editable control's value, build the
            /// `(key, value)` edit list (blank text ⇒ `None` = remove the key), write
            /// it back to `aterm.toml` NON-DESTRUCTIVELY via `super::save_prefs_edits`
            /// (preserving the user's other keys/comments), and — only when the file
            /// actually changed — post the SAME `Wake::ConfigReload` the Reload button
            /// fires so the live hot-reload re-applies it. Best-effort; never panics.
            #[method(saveConfig:)]
            fn save_config(&self, _sender: Option<&AnyObject>) {
                let ivars = self.ivars();
                // Read each control's CURRENT value. A text field's `stringValue`,
                // trimmed-blank ⇒ remove (revert to default). The checkbox's on-state
                // is a bool string typed by `apply_prefs_edits`.
                let controls = ivars.controls.borrow();
                let mut edits: Vec<(&str, Option<String>)> = Vec::with_capacity(controls.len());
                for saved in controls.iter() {
                    let value: Option<String> = match &saved.ctl {
                        Ctl::Text(field) => {
                            // SAFETY: `stringValue` is a plain main-thread getter on the
                            // live field; we copy it into a Rust String immediately.
                            let s = unsafe { field.stringValue() }.to_string();
                            let t = s.trim();
                            if t.is_empty() { None } else { Some(t.to_string()) }
                        }
                        Ctl::Check(btn) => {
                            // SAFETY: `state` is a plain main-thread getter; compare to
                            // the documented On constant.
                            let on = unsafe { btn.state() } == NSControlStateValueOn;
                            Some(on.to_string())
                        }
                    };
                    // The value's TOML TYPE is decided at write time by `edit_kind(key)`
                    // inside `apply_prefs_edits`, so only the key + raw value travel here.
                    edits.push((saved.key, value));
                }
                drop(controls);
                // Persist non-destructively; reload only if the file actually changed.
                if super::save_prefs_edits(&edits) {
                    let _ = ivars.proxy.send_event(Wake::ConfigReload);
                }
            }

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
                let _ = self.ivars().proxy.send_event(Wake::ConfigReload);
            }
        }
    );

    impl PrefsTarget {
        /// Allocate a button/control target owning `proxy` (controls filled in later).
        /// `mtm` proves we are on the main thread (AppKit requirement), which the winit
        /// loop guarantees.
        fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<Wake>) -> Retained<Self> {
            let this = mtm.alloc().set_ivars(PrefsIvars {
                proxy,
                controls: RefCell::new(Vec::new()),
            });
            // SAFETY: plain `[super init]` on a freshly allocated instance.
            unsafe { msg_send_id![super(this), init] }
        }
    }

    /// Open (or re-open) the native Preferences window with EDITABLE controls for the
    /// `fields` (from [`super::editable_fields`]) plus Save / Open-file / Reload buttons.
    /// Returns the retained backing objects for `App` to keep alive (AppKit holds button
    /// targets, and a controller-less window, only weakly/by-owner).
    ///
    /// Best-effort: off the main thread the window is simply not built (`None`) — never a
    /// panic. A fresh window is built each call (the previous handle is dropped by the
    /// caller, closing the old window); the controls are re-seeded from the live config.
    pub fn open_preferences(
        proxy: &EventLoopProxy<Wake>,
        fields: &[EditField],
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

        // Manual top-down layout in the content view's (origin bottom-left) coordinate
        // space: we compute each row's y from the TOP down by subtracting from the
        // content height, matching `toolbar.rs`'s explicit-frame approach (no Auto
        // Layout, so no extra objc2 feature). `y` tracks the next row's TOP edge.
        let content_w = WIN_W - 2.0 * MARGIN;
        let ctl_x = MARGIN + LABEL_W;
        let ctl_w = content_w - LABEL_W;
        let mut y = WIN_H - MARGIN - HEADER_H;

        // Header line.
        let header = make_label(mtm, "Edit configuration", true);
        place(&header, MARGIN, y, content_w, HEADER_H);
        y -= HEADER_H + ROW_GAP;

        let mut subviews: Vec<Retained<NSView>> = Vec::with_capacity(fields.len() * 2 + 4);
        // NSTextField -> NSControl -> NSView: two `into_super` lifts to the common view
        // type so a heterogeneous (label + field + button) set lives in one Vec.
        subviews.push(to_view_label(header));

        // The controls we will hand to the target for Save to read back.
        let mut saved: Vec<Saved> = Vec::with_capacity(fields.len());

        // One row per setting: a fixed-width label column + an editable control. The
        // copy-on-select bool is a checkbox (its title carries the row label, so it
        // needs no separate label); everything else is a label + editable text field.
        for field in fields {
            match field.kind {
                EditKind::Bool => {
                    // A checkbox whose title is the row label; seeded on/off from `seed`.
                    let on = field.seed.as_deref() == Some("true");
                    let check = make_checkbox(mtm, field.label, on);
                    place(&check, MARGIN, y, content_w, ROW_H);
                    subviews.push(to_view_button(check.clone()));
                    saved.push(Saved {
                        key: field.key,
                        ctl: Ctl::Check(check),
                    });
                }
                _ => {
                    let label_field = make_label(mtm, field.label, false);
                    place(&label_field, MARGIN, y, LABEL_W, ROW_H);
                    let edit = make_text_field(mtm, field.seed.as_deref().unwrap_or(""));
                    place(&edit, ctl_x, y, ctl_w, ROW_H);
                    subviews.push(to_view_label(label_field));
                    subviews.push(to_view_label(edit.clone()));
                    saved.push(Saved {
                        key: field.key,
                        ctl: Ctl::Text(edit),
                    });
                }
            }
            y -= ROW_H + ROW_GAP;
        }

        // Hand the live controls to the target so `saveConfig:` can read them back.
        *target.ivars().controls.borrow_mut() = saved;

        // The button row, pinned near the bottom: Save | Open aterm.toml | Reload.
        let save = make_button(mtm, "Save", &target, sel!(saveConfig:));
        place(&save, MARGIN, MARGIN, BUTTON_W, BUTTON_H);
        let open = make_button(mtm, "Open aterm.toml", &target, sel!(openConfigFile:));
        place(
            &open,
            MARGIN + BUTTON_W + BUTTON_GAP,
            MARGIN,
            BUTTON_W,
            BUTTON_H,
        );
        let reload = make_button(mtm, "Reload", &target, sel!(reloadConfig:));
        place(
            &reload,
            MARGIN + 2.0 * (BUTTON_W + BUTTON_GAP),
            MARGIN,
            BUTTON_W,
            BUTTON_H,
        );
        // NSButton -> NSControl -> NSView (two levels), like NSTextField above.
        subviews.push(to_view_button(save));
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
    /// -> NSView) so labels, fields, and buttons share one `Vec<Retained<NSView>>`.
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

    /// Build an EDITABLE, bezeled text field seeded with `value` (the configured raw
    /// value, or `""` for an unset key). Uses the documented non-raising
    /// `textFieldWithString:` factory, then flips it editable + bezeled. Its value is
    /// read back in `saveConfig:`.
    fn make_text_field(mtm: MainThreadMarker, value: &str) -> Retained<NSTextField> {
        // SAFETY: `textFieldWithString:` is the documented non-raising factory; plain
        // setters follow on the fresh field; all on the main thread.
        unsafe {
            let field = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
            field.setEditable(true);
            field.setSelectable(true);
            field.setBezeled(true);
            field.setDrawsBackground(true);
            field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
            field
        }
    }

    /// Build a CHECKBOX titled `title`, seeded `on`/off. No target/action is wired: the
    /// checkbox toggles its OWN state on click and that state is SAMPLED only when Save
    /// runs (`saveConfig:` reads it back from the retained control), so it needs no
    /// per-click relay. Uses the documented non-raising `checkboxWithTitle:target:action:`
    /// factory; `setState:` seeds the initial position.
    fn make_checkbox(mtm: MainThreadMarker, title: &str, on: bool) -> Retained<NSButton> {
        use objc2_app_kit::NSControlStateValueOff;
        // SAFETY: `checkboxWithTitle:target:action:` is the documented factory; a `nil`
        // target/action is valid (no relay); `setState:` is a plain main-thread setter;
        // on the main thread.
        unsafe {
            let check = NSButton::checkboxWithTitle_target_action(
                &NSString::from_str(title),
                None,
                None,
                mtm,
            );
            check.setState(if on {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            check
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

/// Tests for the PURE non-destructive editor — the safety net for the (untestable)
/// objc2 Save handler: each edit goes through exactly the [`apply_prefs_edits`] the
/// window calls, so the file-rewrite semantics are proven here, AppKit-free.
#[cfg(test)]
mod edit_tests {
    use super::{
        Config, EDIT_COPY_ON_SELECT, EDIT_CURSOR_STYLE, EDIT_FONT_FAMILY, EDIT_FONT_PX,
        EDIT_SCROLLBACK, EDIT_THEME, PrefsEditError, apply_prefs_edits, editable_fields,
    };

    /// `Some(v)` helper to keep the edit lists terse.
    fn set(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    /// Setting a NEW key on an empty file writes it typed correctly and re-parses
    /// through `Config` (a Save → reload round-trip).
    #[test]
    fn set_new_key_on_empty_file() {
        let out = apply_prefs_edits("", &[(EDIT_FONT_PX, set("15.5"))]).unwrap();
        assert!(out.contains("font_px"), "wrote the key: {out:?}");
        // Floats are typed as TOML floats, not strings, so serde reads f32.
        let c: Config = toml::from_str(&out).expect("round-trips");
        assert_eq!(c.font_px, Some(15.5));
    }

    /// UPDATING an existing key changes only its value, leaving its line in place.
    #[test]
    fn update_existing_key() {
        let existing = "font_px = 12.0\ntheme = \"Dracula\"\n";
        let out = apply_prefs_edits(existing, &[(EDIT_FONT_PX, set("18.0"))]).unwrap();
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.font_px, Some(18.0));
        // The unrelated key is untouched.
        assert_eq!(c.theme.as_deref(), Some("Dracula"));
    }

    /// A `None` (blank/cleared) edit REMOVES the key — reverting it to its default —
    /// rather than writing an empty string.
    #[test]
    fn blank_removes_key() {
        let existing = "font_px = 12.0\ntheme = \"Dracula\"\n";
        let out = apply_prefs_edits(existing, &[(EDIT_FONT_PX, None)]).unwrap();
        assert!(!out.contains("font_px"), "key removed: {out:?}");
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.font_px, None);
        // Removing the cleared key does NOT touch the sibling.
        assert_eq!(c.theme.as_deref(), Some("Dracula"));
    }

    /// Removing an ALREADY-absent key is a clean no-op, not an error.
    #[test]
    fn remove_absent_key_is_noop() {
        let out = apply_prefs_edits("theme = \"Dracula\"\n", &[(EDIT_FONT_PX, None)]).unwrap();
        assert_eq!(out, "theme = \"Dracula\"\n");
    }

    /// COMMENTS and unrelated keys survive an edit — the whole point of the
    /// non-destructive (toml_edit DOM) write vs. a re-serialize.
    #[test]
    fn preserves_comments_and_unrelated_keys() {
        let existing = "\
# my aterm config
font_px = 12.0  # cozy
gpu = true
[keybindings]
\"cmd+shift+t\" = \"new_tab\"
";
        let out = apply_prefs_edits(existing, &[(EDIT_THEME, set("Nord"))]).unwrap();
        // The header comment, the inline comment, the unrelated `gpu`, and the whole
        // `[keybindings]` table all survive verbatim.
        assert!(out.contains("# my aterm config"), "{out}");
        assert!(out.contains("# cozy"), "{out}");
        assert!(out.contains("gpu = true"), "{out}");
        assert!(out.contains("[keybindings]"), "{out}");
        assert!(out.contains("\"cmd+shift+t\" = \"new_tab\""), "{out}");
        // And the new key landed + re-parses.
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.theme.as_deref(), Some("Nord"));
        assert_eq!(c.font_px, Some(12.0));
    }

    /// Each key is typed PER ITS `Config` field: float / int / bool / string — so a
    /// Save round-trips through serde for every field at once.
    #[test]
    fn types_each_field_correctly() {
        let out = apply_prefs_edits(
            "",
            &[
                (EDIT_FONT_PX, set("14")),
                (EDIT_SCROLLBACK, set("50000")),
                (EDIT_COPY_ON_SELECT, set("true")),
                (EDIT_THEME, set("Dracula")),
                (EDIT_FONT_FAMILY, set("JetBrains Mono")),
                (EDIT_CURSOR_STYLE, set("bar")),
            ],
        )
        .unwrap();
        let c: Config = toml::from_str(&out).expect("typed values round-trip");
        assert_eq!(c.font_px, Some(14.0)); // float, even from "14"
        assert_eq!(c.scrollback_lines, Some(50000)); // integer
        assert_eq!(c.copy_on_select, Some(true)); // bool
        assert_eq!(c.theme.as_deref(), Some("Dracula")); // string
        assert_eq!(c.font_family.as_deref(), Some("JetBrains Mono")); // string w/ space
        assert_eq!(c.cursor_style.as_deref(), Some("bar")); // string
        // The numeric fields are NOT quoted strings in the output.
        assert!(out.contains("font_px = 14"), "float unquoted: {out}");
        assert!(
            out.contains("scrollback_lines = 50000"),
            "int unquoted: {out}"
        );
        assert!(
            out.contains("copy_on_select = true"),
            "bool unquoted: {out}"
        );
    }

    /// A non-numeric font size is rejected (`BadValue`) so Save never writes a value
    /// the reload parser would choke on; the caller leaves the file untouched.
    #[test]
    fn bad_numeric_value_is_rejected() {
        let err = apply_prefs_edits("", &[(EDIT_FONT_PX, set("not-a-number"))]).unwrap_err();
        match err {
            PrefsEditError::BadValue { key, .. } => assert_eq!(key, EDIT_FONT_PX),
            other => panic!("expected BadValue, got {other:?}"),
        }
    }

    /// A non-integer scrollback (a float string) is rejected — `scrollback_lines` is a
    /// usize, so "1.5" must not silently truncate or write a float.
    #[test]
    fn float_for_integer_field_is_rejected() {
        let err = apply_prefs_edits("", &[(EDIT_SCROLLBACK, set("1.5"))]).unwrap_err();
        assert!(matches!(err, PrefsEditError::BadValue { .. }));
    }

    /// A malformed EXISTING file is refused (`Parse`) rather than overwritten, so a
    /// hand-corrupted `aterm.toml` is never clobbered by a Save.
    #[test]
    fn malformed_existing_file_is_refused() {
        let err = apply_prefs_edits("this = = broken", &[(EDIT_THEME, set("Nord"))]).unwrap_err();
        assert!(matches!(err, PrefsEditError::Parse(_)));
    }

    /// Multiple edits in one batch (set + update + remove) all land together and
    /// nothing else moves.
    #[test]
    fn batched_set_update_remove() {
        let existing = "font_px = 12.0\ntheme = \"Dracula\"\ngpu = true\n";
        let out = apply_prefs_edits(
            existing,
            &[
                (EDIT_FONT_PX, set("20.0")), // update
                (EDIT_THEME, None),          // remove
                (EDIT_SCROLLBACK, set("0")), // set new (0 = unlimited)
            ],
        )
        .unwrap();
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.font_px, Some(20.0));
        assert_eq!(c.theme, None);
        assert_eq!(c.scrollback_lines, Some(0));
        assert!(out.contains("gpu = true"), "unrelated key survives: {out}");
    }

    /// A `dark:…,light:…` split theme (a string with a comma + colons) round-trips as a
    /// single string value, not mangled into a TOML structure.
    #[test]
    fn split_theme_string_round_trips() {
        let out =
            apply_prefs_edits("", &[(EDIT_THEME, set("dark:Dracula,light:GitHub Light"))]).unwrap();
        let c: Config = toml::from_str(&out).unwrap();
        assert_eq!(c.theme.as_deref(), Some("dark:Dracula,light:GitHub Light"));
    }

    /// `editable_fields` seeds CONFIGURED values only (unset = blank), in the documented
    /// row order, with the right keys + kinds — what the window maps to controls.
    #[test]
    fn editable_fields_seed_from_config() {
        let c: Config = toml::from_str(
            "font_px = 13.0\ntheme = \"Nord\"\nscrollback_lines = 4000\ncopy_on_select = true\n",
        )
        .unwrap();
        let fields = editable_fields(&c);
        let keys: Vec<&str> = fields.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            vec![
                EDIT_FONT_PX,
                EDIT_FONT_FAMILY,
                EDIT_THEME,
                EDIT_CURSOR_STYLE,
                EDIT_SCROLLBACK,
                EDIT_COPY_ON_SELECT,
            ]
        );
        let seed = |k: &str| {
            fields
                .iter()
                .find(|f| f.key == k)
                .and_then(|f| f.seed.clone())
        };
        assert_eq!(seed(EDIT_FONT_PX).as_deref(), Some("13"));
        assert_eq!(seed(EDIT_THEME).as_deref(), Some("Nord"));
        assert_eq!(seed(EDIT_SCROLLBACK).as_deref(), Some("4000"));
        // The bool seeds its RESOLVED state so the checkbox starts in the right spot.
        assert_eq!(seed(EDIT_COPY_ON_SELECT).as_deref(), Some("true"));
        // Unset keys seed None (blank control) — NOT the effective default.
        assert_eq!(seed(EDIT_FONT_FAMILY), None);
        assert_eq!(seed(EDIT_CURSOR_STYLE), None);
    }

    /// On a fully-unset config every text field seeds blank and the bool seeds the OFF
    /// default — so an unchanged Save (all-blank, checkbox off) writes/removes nothing
    /// the round-trip can't represent.
    #[test]
    fn editable_fields_default_config_is_blank() {
        let fields = editable_fields(&Config::default());
        let seed = |k: &str| {
            fields
                .iter()
                .find(|f| f.key == k)
                .and_then(|f| f.seed.clone())
        };
        assert_eq!(seed(EDIT_FONT_PX), None);
        assert_eq!(seed(EDIT_FONT_FAMILY), None);
        assert_eq!(seed(EDIT_THEME), None);
        assert_eq!(seed(EDIT_CURSOR_STYLE), None);
        assert_eq!(seed(EDIT_SCROLLBACK), None);
        assert_eq!(seed(EDIT_COPY_ON_SELECT).as_deref(), Some("false"));
    }

    /// A whitespace-only configured string seeds BLANK (None), matching the display
    /// fallback — so re-saving an untouched window doesn't materialise a "   " value.
    #[test]
    fn editable_fields_blank_string_seeds_none() {
        let c: Config = toml::from_str("theme = \"   \"\nfont_family = \"\"\n").unwrap();
        let fields = editable_fields(&c);
        let seed = |k: &str| {
            fields
                .iter()
                .find(|f| f.key == k)
                .and_then(|f| f.seed.clone())
        };
        assert_eq!(seed(EDIT_THEME), None);
        assert_eq!(seed(EDIT_FONT_FAMILY), None);
    }
}
