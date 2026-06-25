// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The native macOS window chrome attached to each aterm window: a SINGLE compact
//! Ghostty-style row hosting the traffic lights, a full-width VIEW-BASED TAB STRIP,
//! and a trailing "+" New Tab affordance — all in ONE titlebar row.
//!
//! ONE-ROW LAYOUT: a `UnifiedCompact`-style `NSToolbar` (`NSWindowToolbarStyle::
//! UnifiedCompact`) collapses the titlebar + toolbar into a SINGLE short row — the
//! traffic lights, the tab strip and the "+" all in line — with the terminal content
//! view sitting BELOW it (no occlusion). The title is hidden
//! (`setTitleVisibility:` Hidden) and the titlebar transparent for the seamless,
//! title-less Ghostty look.
//!
//! The toolbar holds exactly ONE custom-view `NSToolbarItem` whose view is the
//! full-width TAB STRIP container `NSView`: it holds one [`TabView`] sub-view PER tab,
//! laid out left→right edge-to-edge between the traffic-light inset and the "+", plus
//! a trailing "+" New Tab `NSButton` pinned to the right. This REPLACES the earlier
//! `NSSegmentedControl` — a custom view lets each tab carry a clean active highlight (a
//! subtle full-tab lighter fill + a brighter label), a per-tab CLOSE × (revealed on hover
//! via an `NSTrackingArea`, always shown on the active tab), and a drag-to-reorder
//! gesture — none of which a segmented control affords.
//!
//! Like the menu bar (`menu.rs`), this chrome adds NO new behavior: each affordance
//! is a thin DISPATCH stub that posts a `Wake` the main loop turns into an existing
//! `App` command — never a parallel path:
//!   * a `mouseDown:` on a [`TabView`] posts
//!     [`Wake::SelectTab`](crate::Wake) `{ window, index }` → `App::switch_tab_in`;
//!   * a click on a tab's CLOSE × posts
//!     [`Wake::CloseTab`](crate::Wake) `{ window, index }` → `App::close_tab_at`;
//!   * a `mouseDragged:` reorder posts a `tab move`-equivalent
//!     [`Wake::TabCmd`](crate::Wake) `{ Move }` → `App::move_tab`;
//!   * a click on the "+" button posts the SAME
//!     [`Wake::MenuAction`](crate::Wake) File ▸ New Tab
//!     ([`MenuAction::NewTab`](crate::menu::MenuAction::NewTab)) → `App::open_tab`.
//!
//! Three small Objective-C objects back the chrome, mirroring `menu.rs`:
//!   * a [`TabsTarget`] — an `NSObject` owning the [`EventLoopProxy<Wake>`] AND the
//!     window's [`WindowId`](crate::WindowId), whose `newTab:` selector (the "+"
//!     button) relays a [`Wake::MenuAction`] `{ NewTab }`;
//!   * a [`TabView`] — a custom `NSView` subclass, ONE per tab, owning the proxy +
//!     window + its tab index + active flag, plus its title `NSTextField` label and a
//!     close `NSButton`. It draws the (in)active background/accent in `drawRect:`,
//!     tracks hover (`mouseEntered:`/`mouseExited:`) to reveal the ×, and turns a
//!     `mouseDown:`/`mouseDragged:` gesture into select / reorder `Wake`s; and
//!   * a [`ToolbarDelegate`] — an `NSObject` conforming to `NSToolbarDelegate` that
//!     vends the single strip-item identifier and builds its custom-view item.
//!
//! The TAB STRIP container is HIDDEN at ≤1 tab (a single-tab window shows just the
//! bare compact titlebar + traffic lights, like Ghostty / native macOS apps); the
//! strip — and its "+" — appear only at 2+ tabs (a fresh tab is one Cmd-T / File ▸
//! New Tab away). It is kept in sync with app state by [`set_window_tabs`], called
//! from `App::sync_window` (via `App::refresh_window_tabs`) after every tab
//! open/close/switch/detach/migrate — it rebuilds the per-tab views (count / labels /
//! active accent) and toggles the container's hidden flag.
//!
//! AppKit holds a toolbar's delegate, an item's view, and a control's target only
//! WEAKLY, so [`install_window_toolbar`] returns a [`ToolbarHandle`] retaining the
//! target, the delegate, the toolbar, the container view, the live `TabView`s AND the
//! "+" button; `App` keeps it in a field for the window's life so the
//! callbacks/actions stay live (and so `set_window_tabs` can reach the container to
//! rebuild it) — mirrors `MenuHandle`.
//!
//! NEVER CRASH: objc2-app-kit 0.2.2 makes several initializers raise → a
//! non-unwinding abort. We construct buttons ONLY via the documented factory
//! initializers (`NSButton::buttonWithTitle:target:action:`,
//! `buttonWithImage:target:action:`), text fields via `NSTextField::labelWithString:`,
//! and views via `NSView::initWithFrame:` (all non-raising), and every AppKit call
//! is on the main thread behind a `MainThreadMarker`.
//!
//! Everything imperative is `#[cfg(target_os = "macos")]`; off macOS no-op
//! [`install_window_toolbar`] / [`set_window_tabs`] and a unit [`ToolbarHandle`]
//! keep the workspace building everywhere, exactly like `menu.rs`.

#[cfg(target_os = "macos")]
pub use macos::{ToolbarHandle, install_window_toolbar, read_tab_chrome, set_window_tabs};

#[cfg(not(target_os = "macos"))]
pub use non_macos::{ToolbarHandle, install_window_toolbar, read_tab_chrome, set_window_tabs};

/// Format the toolbar tab-switcher introspection line for `titles` (one label per
/// tab) with the 0-based `active` index selected, or `None` when there is no strip
/// to report (≤1 tab — a single-tab window shows no switcher, mirroring the macOS
/// strip's hide-when-≤1 rule). The line matches the macOS [`read_tab_chrome`]
/// format EXACTLY — `toolbar-tabs count=<n> selected=<i> labels=[...]` — so a
/// driving AI reads one stable shape on every platform. `active` is clamped into
/// range so an out-of-bounds index never produces a bogus `selected`.
///
/// PURE: no I/O, no AppKit/winit — just string formatting from the tab model, so it
/// is unit-tested directly (see the `non_macos_tests` module). The macOS strip
/// computes the per-tab "title  ⌘N" label inside its `NSView` build; here the labels
/// are the raw session titles the caller passes (a full GTK4 header bar would render
/// the same ⌘-hint decoration, deferred — see [`install_window_toolbar`]).
///
/// `allow(dead_code)`: this feeds [`read_tab_chrome`], whose only consumer is the
/// `chrome` verb's introspection path (`App::read_native_chrome`). That verb's
/// non-macOS arm — which lives in `app_introspect.rs`, OUTSIDE this toolbar/platform
/// seam — does not yet surface this line, so on Linux the formatter is reachable but
/// currently uncalled. Wiring it in is the documented next step (it is the macOS
/// `read_native_chrome` arm's `read_toolbar_chrome` call, mirrored for Linux). The
/// unit tests below DO exercise it, so the logic is verified regardless.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
#[must_use]
pub fn format_tab_chrome(titles: &[String], active: usize) -> Option<String> {
    let count = titles.len();
    if count <= 1 {
        // ≤1 tab: no switcher chrome, exactly like the hidden macOS strip.
        return None;
    }
    let selected = active.min(count - 1) as isize;
    Some(format!(
        "toolbar-tabs count={count} selected={selected} labels={titles:?}"
    ))
}

/// Compute the window TITLE a tab-aware header bar would show for `titles` with the
/// 0-based `active` index selected: the active tab's own title, with a ` — [i/n]`
/// position suffix when there is more than one tab (so the tab state is legible in
/// the window chrome even before a real tab strip exists). `None` when there is
/// nothing to title (no tabs) or only the bare single tab carries no extra suffix —
/// returns `Some(title)` with no counter so a one-tab window reads cleanly.
///
/// PURE: pure string assembly from the tab model, unit-tested below. The Linux
/// toolbar uses this in [`install_window_toolbar`] to seed the winit window title
/// from the initial tab set; live per-tab title updates continue to flow through the
/// cross-platform `App::apply_title` path (which owns `window.set_title` every
/// frame), so this helper never fights that owner — it only provides the seam's view
/// of what the active tab's title is.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn format_window_title(titles: &[String], active: usize) -> Option<String> {
    let n = titles.len();
    if n == 0 {
        return None;
    }
    let i = active.min(n - 1);
    let base = titles[i].trim();
    let base = if base.is_empty() { "aterm" } else { base };
    if n == 1 {
        Some(base.to_string())
    } else {
        Some(format!("{base} — [{}/{n}]", i + 1))
    }
}

/// The non-macOS toolbar seam: a REAL in-memory tab-chrome model (no GTK system
/// libraries required — see the deferred-work note on [`install_window_toolbar`]).
///
/// Off macOS aterm has no native `NSToolbar`; a full Linux equivalent is a GTK4
/// `GtkHeaderBar` (or a Wayland client-side-decoration tab strip), which needs the
/// gtk4/glib system development libraries that are NOT present on every host. Rather
/// than a dead `()` no-op, this module keeps the seam HONEST: it maintains the same
/// tab-chrome state the macOS strip does (titles + active index), reflects the
/// initial active tab into the winit window title, and serves the `chrome` verb's
/// introspection line from that live model. So [`set_window_tabs`] and
/// [`read_tab_chrome`] are real call sites with observable effect, not silent
/// dead-ends — the seam is ready for a real header bar to slot in behind it.
#[cfg(not(target_os = "macos"))]
mod non_macos {
    use std::cell::RefCell;

    use winit::event_loop::EventLoopProxy;
    use winit::window::Window;

    use super::{format_tab_chrome, format_window_title};
    use crate::{Wake, WindowId};

    /// The live tab-chrome model for one window: the per-tab titles (in tab order)
    /// and the 0-based active index. The single source of truth the (future) header
    /// bar would render and that [`read_tab_chrome`] introspects — the Linux analogue
    /// of the macOS handle's retained `Vec<TabView>` + active flag.
    #[derive(Default)]
    pub(super) struct TabChrome {
        /// One label per tab, in tab order — the raw session titles the caller syncs.
        titles: Vec<String>,
        /// The 0-based index of the active tab (clamped into range on read/format).
        active: usize,
    }

    /// What [`install_window_toolbar`] returns off macOS: a REAL handle wrapping the
    /// interior-mutable [`TabChrome`] model (so [`set_window_tabs`] can update it
    /// through the shared `&self` the seam hands out, exactly like the macOS handle's
    /// `RefCell<Vec<TabView>>`) plus the window's [`WindowId`] and the `Wake` proxy
    /// the future header bar's affordances would relay through. `App` keeps it in its
    /// `_toolbars` map for the window's life, identical to the macOS path.
    pub struct ToolbarHandle {
        /// The live tab-chrome model — updated by [`set_window_tabs`], read by
        /// [`read_tab_chrome`]. `RefCell` because the seam exposes only `&self`.
        chrome: RefCell<TabChrome>,
        /// The window this chrome belongs to, kept so a future header bar addresses
        /// the RIGHT window's tab affordances (the macOS handle holds it for the same
        /// reason). Not yet read on Linux — there is no native control to drive — so
        /// allow it to be dead until the GTK4 header bar lands.
        #[allow(dead_code)]
        window: WindowId,
        /// The `Wake` channel a future header bar's tab clicks / "+" button would
        /// relay through (select / close / new-tab), mirroring the macOS handle's
        /// retained targets. Held now so the seam already owns everything a real
        /// control needs; unused until that control exists.
        #[allow(dead_code)]
        proxy: EventLoopProxy<Wake>,
    }

    /// Install the non-macOS window "toolbar": there is no native control to attach,
    /// so this builds the in-memory [`ToolbarHandle`] model and seeds the winit
    /// window title from the (initially single-tab) state via the pure
    /// [`format_window_title`]. The strip starts empty; the caller's first
    /// `App::sync_window` calls [`set_window_tabs`] to populate it.
    ///
    /// DEFERRED — a full native Linux toolbar: this is where a real
    /// `gtk4::HeaderBar` (or a Wayland client-side-decoration tab strip) would be
    /// constructed and attached — packing one tab widget per title, a trailing "+"
    /// New Tab button relaying `Wake::MenuAction { NewTab }`, and per-tab close/select
    /// gestures relaying `Wake::CloseTab` / `Wake::SelectTab` (exactly the macOS
    /// `toolbar.rs` dispatch). That requires the **gtk4 + glib system development
    /// libraries** (`libgtk-4-dev` / `gtk4` pkg-config) and a `gtk4`/`glib` crate
    /// dependency, NONE of which are available on the macOS build host — so it is
    /// intentionally NOT built here. The seam (this handle + model) is the buildable
    /// scaffolding that header bar slots behind without touching `App`.
    pub fn install_window_toolbar(
        window: &Window,
        proxy: &EventLoopProxy<Wake>,
        wid: WindowId,
    ) -> Option<ToolbarHandle> {
        // Seed the title from the initial (empty) model. A fresh window has no synced
        // tabs yet, so `format_window_title` yields `None` and we fall back to the
        // bare app name — a sensible title before the first `set_window_tabs`. Live
        // per-tab updates are then owned by `App::apply_title`.
        let title = format_window_title(&[], 0).unwrap_or_else(|| "aterm".to_string());
        window.set_title(&title);
        Some(ToolbarHandle {
            chrome: RefCell::new(TabChrome::default()),
            window: wid,
            proxy: proxy.clone(),
        })
    }

    /// Re-sync the non-macOS tab-chrome model to the current app tab state: store
    /// `titles` + the 0-based `active` index in the handle's [`TabChrome`]. This is
    /// the real Linux analogue of the macOS strip rebuild — it keeps the seam's model
    /// in lock-step with `App`'s tabs, so [`read_tab_chrome`] always reports the live
    /// set. A future header bar would, in addition, re-pack its tab widgets here.
    ///
    /// NB: this does NOT call `window.set_title` — the handle holds no `&Window` (the
    /// seam passes only `&self`), and the cross-platform `App::apply_title` path
    /// already owns the live title every frame, so re-titling here would double-write
    /// it. The model update IS the observable effect.
    pub fn set_window_tabs(handle: &ToolbarHandle, titles: &[String], active: usize) {
        let mut chrome = handle.chrome.borrow_mut();
        chrome.titles.clear();
        chrome.titles.extend_from_slice(titles);
        chrome.active = active;
    }

    /// Read the non-macOS tab-switcher introspection line from the live model via the
    /// pure [`format_tab_chrome`]: `toolbar-tabs count=<n> selected=<i> labels=[...]`
    /// at 2+ tabs, `None` at ≤1 (mirroring the macOS hide-when-≤1 strip). Off the
    /// macOS path the `chrome` verb's non-macOS arm does not yet surface this line
    /// (that wiring lives in `app_introspect.rs`, OUTSIDE this seam — the documented
    /// next step is to mirror the macOS `read_native_chrome` arm's
    /// `apprt.read_toolbar_chrome` call for Linux), but the model is real and the
    /// reader is exercised by the unit tests below.
    ///
    /// `allow(dead_code)`: reachable through [`super::ToolbarHandle`] /
    /// `AppRt::read_toolbar_chrome` but, as noted, not yet called on Linux — same
    /// uncalled-on-Linux status the original `()`-handle stub had, now backed by a
    /// real model instead of an unconditional `None`.
    #[allow(dead_code)]
    #[must_use]
    pub fn read_tab_chrome(handle: &ToolbarHandle) -> Option<String> {
        let chrome = handle.chrome.borrow();
        format_tab_chrome(&chrome.titles, chrome.active)
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod non_macos_tests {
    use super::{format_tab_chrome, format_window_title};

    /// ≤1 tab reports NO switcher chrome (the hide-when-≤1 rule), matching the macOS
    /// strip that hides at a single tab.
    #[test]
    fn chrome_hidden_at_one_or_zero_tabs() {
        assert_eq!(format_tab_chrome(&[], 0), None);
        assert_eq!(format_tab_chrome(&["zsh".to_string()], 0), None);
    }

    /// 2+ tabs report the count / selected / labels line in the EXACT macOS shape, so
    /// the introspection output is platform-stable.
    #[test]
    fn chrome_line_matches_macos_shape() {
        let titles = vec!["zsh".to_string(), "vim".to_string(), "htop".to_string()];
        assert_eq!(
            format_tab_chrome(&titles, 1).as_deref(),
            Some(r#"toolbar-tabs count=3 selected=1 labels=["zsh", "vim", "htop"]"#)
        );
    }

    /// An out-of-range active index is clamped to the last tab rather than producing a
    /// bogus `selected` (defensive — a stale index never escapes the formatter).
    #[test]
    fn chrome_clamps_out_of_range_active() {
        let titles = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            format_tab_chrome(&titles, 9).as_deref(),
            Some(r#"toolbar-tabs count=2 selected=1 labels=["a", "b"]"#)
        );
    }

    /// A single tab titles the window with JUST the active title (no `[i/n]`
    /// counter), so a one-tab window reads cleanly.
    #[test]
    fn title_single_tab_has_no_counter() {
        assert_eq!(
            format_window_title(&["vim".to_string()], 0).as_deref(),
            Some("vim")
        );
    }

    /// 2+ tabs append the ` — [i/n]` position suffix from the ACTIVE index (1-based
    /// in the display), so the tab state is legible in the window chrome.
    #[test]
    fn title_multi_tab_has_position_counter() {
        let titles = vec!["zsh".to_string(), "vim".to_string(), "htop".to_string()];
        assert_eq!(
            format_window_title(&titles, 2).as_deref(),
            Some("htop — [3/3]")
        );
    }

    /// An empty active title falls back to "aterm" (never a blank titlebar), and the
    /// out-of-range index is clamped like the chrome line.
    #[test]
    fn title_blank_falls_back_and_clamps() {
        assert_eq!(
            format_window_title(&["   ".to_string()], 0).as_deref(),
            Some("aterm")
        );
        let titles = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            format_window_title(&titles, 99).as_deref(),
            Some("b — [2/2]")
        );
    }

    /// No tabs at all yields no title (the install seed then defaults to "aterm").
    #[test]
    fn title_no_tabs_is_none() {
        assert_eq!(format_window_title(&[], 0), None);
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::{Cell, RefCell};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
    use objc2::{ClassType, DeclaredClass, declare_class, msg_send_id, mutability, sel};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions, NSBezierPath, NSButton,
        NSCellImagePosition, NSColor, NSEvent, NSFont, NSImage, NSImageNameAddTemplate,
        NSTextAlignment, NSTextField, NSToolbar, NSToolbarDelegate, NSToolbarDisplayMode,
        NSToolbarItem, NSToolbarItemIdentifier, NSTrackingArea, NSTrackingAreaOptions, NSView,
        NSWindowTitleVisibility, NSWindowToolbarStyle,
    };
    use objc2_foundation::{CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSRect, NSString};
    use winit::event_loop::EventLoopProxy;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use crate::menu::MenuAction;
    use crate::{TabAction, Wake, WindowId};

    /// Height (points) of the title-bar tab strip — the SINGLE chrome row. Roughly one
    /// row of a regular control plus a little breathing room, matching the compact
    /// Ghostty tab bar. The traffic lights are ~14pt tall and center within this row.
    const STRIP_HEIGHT: f64 = 28.0;

    /// Leading inset (points) reserved for the macOS traffic-light buttons, so the tab
    /// strip starts to their RIGHT in the SAME row (the Ghostty layout). The three
    /// stoplights span ~70pt; 76pt leaves a little gutter before the first tab.
    const TRAFFIC_LIGHT_INSET: f64 = 76.0;

    /// Width (points) of the trailing "+" New Tab button at the right end of the strip.
    const PLUS_WIDTH: f64 = 30.0;

    /// Min/max width (points) of the tab-strip toolbar ITEM. A plain custom `NSView`
    /// has no intrinsic size; without these the `UnifiedCompact` toolbar collapses the
    /// strip to zero width and hides it behind an overflow `»` chevron. The small
    /// minimum keeps it present; the very large maximum lets it stretch full-width.
    const STRIP_MIN_WIDTH: f64 = 120.0;
    const STRIP_MAX_WIDTH: f64 = 100_000.0;

    /// Width (points) of a tab's close × button (a small square at the tab's left).
    const CLOSE_W: f64 = 16.0;

    /// The widest a single tab grows to (so two tabs don't each eat half a wide
    /// window); extra width is left as bare strip background past the last tab.
    const MAX_TAB_W: f64 = 220.0;
    /// The narrowest a tab shrinks to before we stop shrinking (it then just clips its
    /// label); keeps the close × + a sliver of title clickable.
    const MIN_TAB_W: f64 = 48.0;

    /// Curved-tab "pill" geometry. The active/hovered fill is a rounded rect inset from
    /// the full tab cell by these margins (so it floats with breathing room, the iTerm
    /// look) with this corner radius. The horizontal inset also yields the visual gap
    /// between adjacent pills.
    const TAB_PILL_INSET_X: f64 = 3.0;
    const TAB_PILL_INSET_Y: f64 = 4.0;
    const TAB_PILL_RADIUS: f64 = 7.0;

    /// What [`install_window_toolbar`] returns: the retained backing objects. AppKit
    /// references a control's target, a toolbar item's view, and a toolbar's delegate
    /// only WEAKLY, so they must outlive the window — `App` holds this in a field.
    pub struct ToolbarHandle {
        /// The "+" New Tab button's action target (owns the proxy + the window's
        /// `WindowId`). The "+" button references its target only weakly, so retain it.
        _tabs_target: Retained<TabsTarget>,
        /// The `NSToolbarDelegate` that vends the strip's single custom-view item. The
        /// toolbar references its delegate only weakly, so retain it here.
        _delegate: Retained<ToolbarDelegate>,
        /// The `NSToolbar` hosting the strip item, kept alive alongside its delegate.
        /// [`set_window_tabs`] toggles its visibility: HIDDEN at ≤1 tab so the titlebar
        /// collapses to a bare, seamless compact bar (just traffic lights), SHOWN at 2+
        /// tabs to reveal the tab strip.
        toolbar: Retained<NSToolbar>,
        /// The full-width container `NSView` (the toolbar item's custom view), holding
        /// the per-tab [`TabView`]s + the "+" button. Retained so [`set_window_tabs`]
        /// can rebuild its tab sub-views and `setHidden:` the whole strip at ≤1 tab,
        /// and so [`read_tab_chrome`] can read the hidden flag + the live tab views.
        container: Retained<NSView>,
        /// The proxy + window id used to build each rebuilt [`TabView`]'s relays. The
        /// container's tab views are rebuilt on every [`set_window_tabs`], so the
        /// builder needs these; they live as long as the handle.
        proxy: EventLoopProxy<Wake>,
        window: WindowId,
        /// The live [`TabView`]s, one per tab, in tab order — the source of truth for
        /// [`read_tab_chrome`] (count / active / labels) and [`set_window_tabs`]'s
        /// rebuild (it removes the old set as subviews and replaces this Vec). AppKit
        /// holds a subview only via its superview's array; we ALSO retain them here so
        /// the per-tab targets/labels stay live and introspection can read them.
        tabs: RefCell<Vec<Retained<TabView>>>,
        /// The trailing "+" New Tab button, pinned to the right end of the strip.
        /// Retained because the container holds its subviews weakly w.r.t. our Rust
        /// ownership; keeping it here documents ownership of the whole strip.
        _plus: Retained<NSButton>,
    }

    // SAFETY: `ToolbarHandle` is only ever created, read, and dropped on the main
    // thread (the event loop). It holds main-thread-only AppKit objects; `App` stores
    // it in a `BTreeMap` keyed by window and never sends it across threads. The
    // `EventLoopProxy` is `Send`. We assert thread-affinity by construction (every
    // method takes a `MainThreadMarker`), so the auto-derived non-Send is the safe
    // default and we add no unsafe Send/Sync.

    /// The ivars of [`TabsTarget`]: the `Wake` channel and the `WindowId` of the
    /// window this control was installed for, so the `newTab:` relay (the "+" button)
    /// posts the same `Wake::MenuAction { NewTab }` File ▸ New Tab fires.
    pub(crate) struct TabsIvars {
        proxy: EventLoopProxy<Wake>,
        #[allow(dead_code)]
        window: WindowId,
    }

    declare_class!(
        /// The target object for the "+" New Tab button. Owns the `EventLoopProxy<Wake>`
        /// and the owning [`WindowId`], and exposes `newTab:` — posting the SAME
        /// [`Wake::MenuAction`] `{ NewTab }` File ▸ New Tab fires → `App::open_tab`.
        pub(crate) struct TabsTarget;

        // SAFETY:
        // - NSObject imposes no subclassing requirements.
        // - InteriorMutable is the safe default; we never mutate the ivars.
        // - TabsTarget has no Drop impl beyond the auto-generated ivar drop.
        unsafe impl ClassType for TabsTarget {
            type Super = objc2::runtime::NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "ATermTabsTarget";
        }

        impl DeclaredClass for TabsTarget {
            type Ivars = TabsIvars;
        }

        unsafe impl TabsTarget {
            /// `newTab:` — the action wired to the "+" New Tab button. Fires the SAME
            /// `Wake::MenuAction { NewTab }` the File ▸ New Tab menu item fires, so the
            /// click reuses the existing `App::open_tab` dispatch.
            #[method(newTab:)]
            fn new_tab(&self, _sender: Option<&AnyObject>) {
                // Fire-and-forget: a closed loop (app shutting down) just drops the
                // event — mirrors every other `send_event` here.
                let _ = self
                    .ivars()
                    .proxy
                    .send_event(Wake::MenuAction { action: MenuAction::NewTab });
            }
        }
    );

    impl TabsTarget {
        /// Allocate a "+"-button target owning `proxy` and the owning `window`.
        /// `mtm` proves we are on the main thread (AppKit requirement).
        fn new(
            mtm: MainThreadMarker,
            proxy: EventLoopProxy<Wake>,
            window: WindowId,
        ) -> Retained<Self> {
            let this = mtm.alloc().set_ivars(TabsIvars { proxy, window });
            // SAFETY: plain `[super init]` on a freshly allocated instance.
            unsafe { msg_send_id![super(this), init] }
        }
    }

    /// The mutable per-tab state a [`TabView`] needs at click/draw time. Held in
    /// `Cell`s/`RefCell`s because AppKit messages the view (`mouseDown:`/`drawRect:`)
    /// through a shared `&self`, and `set_window_tabs` updates the active flag in
    /// place on a tab switch without rebuilding the whole view.
    pub(crate) struct TabIvars {
        /// The `Wake` channel — clicks/drags relay through it.
        proxy: EventLoopProxy<Wake>,
        /// The window this tab belongs to, so a relayed `Wake` addresses the RIGHT
        /// window (a click on a non-frontmost window's strip acts on THAT window).
        window: WindowId,
        /// This tab's 0-based index, used by the select / close / move relays. Set at
        /// build time; tabs are rebuilt (not re-indexed) on every `set_window_tabs`.
        index: Cell<usize>,
        /// Total tab count at build time, so a drag can clamp the destination index.
        count: Cell<usize>,
        /// Whether this is the ACTIVE tab — drives the accent in `drawRect:` and forces
        /// the close × to always show (inactive tabs reveal it only on hover).
        active: Cell<bool>,
        /// Whether the pointer is currently inside this tab (hover) — reveals the × on
        /// an inactive tab. Toggled by the tracking-area enter/exit.
        hovered: Cell<bool>,
        /// The mouse-down location in the tab's own coordinates, kept so `mouseDragged:`
        /// can measure horizontal travel and decide a reorder direction.
        press_x: Cell<f64>,
        /// Whether the current gesture has already fired a reorder (so a long drag
        /// fires at most one `Move` per press — avoids a stutter of swaps).
        dragged: Cell<bool>,
        /// The retained close `NSButton` and title `NSTextField`, so the view can
        /// show/hide the × on hover and so the handle keeps them alive.
        close_btn: RefCell<Option<Retained<NSButton>>>,
        label: RefCell<Option<Retained<NSTextField>>>,
        /// The retained tracking area, so `updateTrackingAreas` can swap it on a frame
        /// change without leaking the old one.
        tracking: RefCell<Option<Retained<NSTrackingArea>>>,
        /// The label text shown (title + "  ⌘N"), for `read_tab_chrome` introspection.
        text: RefCell<String>,
    }

    declare_class!(
        /// One tab's view: a custom `NSView` that draws its (in)active background +
        /// accent, hosts the title label + close ×, tracks hover to reveal the ×, and
        /// turns mouse-down/drag into select / reorder `Wake`s. Built fresh per tab on
        /// each [`set_window_tabs`].
        ///
        /// `MainThreadOnly` mutability is REQUIRED for an `NSView` subclass.
        pub(crate) struct TabView;

        // SAFETY:
        // - NSView is a valid superclass; we add ivars + override responder/draw hooks.
        // - MainThreadOnly is required for views and is sound: a view is only ever
        //   created and messaged on the main thread.
        // - TabView has no Drop impl beyond the auto-generated ivar drop.
        unsafe impl ClassType for TabView {
            type Super = NSView;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "ATermTabView";
        }

        impl DeclaredClass for TabView {
            type Ivars = TabIvars;
        }

        unsafe impl TabView {
            /// Paint the tab background: the ACTIVE tab is a SUBTLE lighter fill drawn as
            /// a CURVED, inset "pill" (rounded corners like iTerm/Safari — not a hard
            /// edge-to-edge block), reinforced by the brighter label; an inactive tab is
            /// flat (the seamless terminal-coloured titlebar shows through) so it recedes;
            /// a hovered inactive tab gets a fainter rounded pill. The fill is an
            /// `NSBezierPath` rounded rect — `bezierPathWithRoundedRect:xRadius:yRadius:`
            /// and `fill` are non-raising drawing calls (no raising initializer).
            #[method(drawRect:)]
            #[allow(non_snake_case)]
            fn drawRect(&self, _dirty: NSRect) {
                let bounds = self.bounds();
                let ivars = self.ivars();
                // The pill is inset from the full tab cell so it floats with a small
                // margin (the curved iTerm look) instead of butting edge-to-edge.
                let pill = CGRect::new(
                    CGPoint::new(bounds.origin.x + TAB_PILL_INSET_X, bounds.origin.y + TAB_PILL_INSET_Y),
                    CGSize::new(
                        (bounds.size.width - 2.0 * TAB_PILL_INSET_X).max(0.0),
                        (bounds.size.height - 2.0 * TAB_PILL_INSET_Y).max(0.0),
                    ),
                );
                // SAFETY: standard AppKit drawing primitives, on the main thread inside
                // a draw cycle (AppKit has set up the focused graphics context). The
                // colors are autoreleased; `set()`/`bezierPathWithRoundedRect:`/`fill`
                // are side-effect-free w.r.t. our state and never raise.
                unsafe {
                    if ivars.active.get() {
                        // Active tab: a SUBTLE lighter rounded pill — a clean selected
                        // panel like iTerm/Safari. The brighter label (labelColor vs
                        // secondaryLabelColor) reinforces which tab is live. A translucent
                        // white reads as "raised" on every dark theme without hard-coding
                        // a theme color.
                        let panel = NSColor::colorWithSRGBRed_green_blue_alpha(
                            1.0, 1.0, 1.0, 0.13,
                        );
                        panel.set();
                        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                            pill, TAB_PILL_RADIUS, TAB_PILL_RADIUS,
                        );
                        path.fill();
                    } else if ivars.hovered.get() {
                        // Inactive but hovered: a fainter rounded pill so the hover target
                        // reads (and the revealed × has a backing).
                        let hover = NSColor::colorWithSRGBRed_green_blue_alpha(
                            1.0, 1.0, 1.0, 0.06,
                        );
                        hover.set();
                        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                            pill, TAB_PILL_RADIUS, TAB_PILL_RADIUS,
                        );
                        path.fill();
                    }
                    // else: flat — the seamless terminal-coloured titlebar shows through.
                }
            }

            /// A click anywhere on the tab (that is not the close ×, which is a button
            /// with its own action) selects it: post `Wake::SelectTab { window, index }`.
            /// Also seeds the drag gesture (records the press X, clears the dragged
            /// latch).
            #[method(mouseDown:)]
            #[allow(non_snake_case)]
            fn mouseDown(&self, event: &NSEvent) {
                let ivars = self.ivars();
                // SAFETY: `locationInWindow` + `convertPoint_fromView` are
                // side-effect-free getters on the main thread; `None` source view means
                // window coordinates.
                let p = unsafe {
                    let win_pt = event.locationInWindow();
                    self.convertPoint_fromView(win_pt, None)
                };
                ivars.press_x.set(p.x);
                ivars.dragged.set(false);
                let _ = ivars.proxy.send_event(Wake::SelectTab {
                    window: ivars.window,
                    index: ivars.index.get(),
                });
            }

            /// A horizontal drag past one tab-width reorders this tab toward the drag
            /// direction (best-effort): post a `Wake::TabCmd { Move { from, to } }`
            /// exactly ONCE per press (the `dragged` latch), reusing `App::move_tab`.
            /// The reply channel is a throwaway (we don't block the UI thread on it).
            #[method(mouseDragged:)]
            #[allow(non_snake_case)]
            fn mouseDragged(&self, event: &NSEvent) {
                let ivars = self.ivars();
                if ivars.dragged.get() {
                    return; // already fired this gesture
                }
                let p = unsafe {
                    let win_pt = event.locationInWindow();
                    self.convertPoint_fromView(win_pt, None)
                };
                let width = self.bounds().size.width.max(1.0);
                let dx = p.x - ivars.press_x.get();
                // Require crossing roughly half a tab to commit a single-step reorder.
                if dx.abs() < width * 0.5 {
                    return;
                }
                let from = ivars.index.get();
                let count = ivars.count.get();
                let to = if dx > 0.0 {
                    (from + 1).min(count.saturating_sub(1))
                } else {
                    from.saturating_sub(1)
                };
                if to == from {
                    return;
                }
                ivars.dragged.set(true);
                let (tx, _rx) = std::sync::mpsc::channel();
                let _ = ivars.proxy.send_event(Wake::TabCmd {
                    action: TabAction::Move { from, to },
                    reply: tx,
                });
            }

            /// Pointer entered the tab — reveal the close × (inactive tabs hide it until
            /// hover) and repaint the faint hover highlight.
            #[method(mouseEntered:)]
            #[allow(non_snake_case)]
            fn mouseEntered(&self, _event: &NSEvent) {
                self.ivars().hovered.set(true);
                self.refresh_close_visibility();
                self.mark_dirty();
            }

            /// Pointer left the tab — hide the × again (unless this is the active tab,
            /// which always shows it) and repaint.
            #[method(mouseExited:)]
            #[allow(non_snake_case)]
            fn mouseExited(&self, _event: &NSEvent) {
                self.ivars().hovered.set(false);
                self.refresh_close_visibility();
                self.mark_dirty();
            }

            /// Rebuild the tracking area whenever the view's geometry changes, so hover
            /// detection follows a resize. Removes the previous area first (no leak).
            #[method(updateTrackingAreas)]
            #[allow(non_snake_case)]
            fn updateTrackingAreas(&self) {
                // SAFETY: standard NSResponder up-call, then our own area swap — all on
                // the main thread.
                unsafe {
                    let _: () = objc2::msg_send![super(self), updateTrackingAreas];
                }
                self.install_tracking_area();
            }

            /// Accept the FIRST click even when the window is not key, so clicking a tab
            /// in a background window both raises it AND selects the tab in one click
            /// (matches native tab behavior).
            #[method(acceptsFirstMouse:)]
            #[allow(non_snake_case)]
            fn acceptsFirstMouse(&self, _event: Option<&NSEvent>) -> bool {
                true
            }
        }

        unsafe impl NSObjectProtocol for TabView {}

        unsafe impl TabView {
            /// `closeTab:` — the action wired to this tab's close × button. Posts a
            /// `Wake::CloseTab { window, index }` so the main loop closes THIS tab via
            /// `App::close_tab_at` (the same whole-tab close the `tab close` verb takes).
            #[method(closeTab:)]
            fn close_tab(&self, _sender: Option<&AnyObject>) {
                let ivars = self.ivars();
                let _ = ivars.proxy.send_event(Wake::CloseTab {
                    window: ivars.window,
                    index: ivars.index.get(),
                });
            }
        }
    );

    impl TabView {
        /// Build a fresh tab view for `index`/`count` showing `text` (already the
        /// "title  ⌘N" label), wired to relay through `proxy`/`window`. Lays out the
        /// close × on the left and the title to its right, installs the hover tracking
        /// area, and sets the active flag (drives the accent + always-on ×). All
        /// construction is via NON-RAISING factory initializers on the main thread.
        #[allow(
            clippy::too_many_arguments,
            reason = "tab state (index/count/text/active) plus its render context (mtm/proxy/window); both are needed at construction and splitting them only relocates the list"
        )]
        fn build(
            mtm: MainThreadMarker,
            proxy: EventLoopProxy<Wake>,
            window: WindowId,
            index: usize,
            count: usize,
            text: &str,
            active: bool,
            frame: NSRect,
        ) -> Retained<Self> {
            let ivars = TabIvars {
                proxy,
                window,
                index: Cell::new(index),
                count: Cell::new(count),
                active: Cell::new(active),
                hovered: Cell::new(false),
                press_x: Cell::new(0.0),
                dragged: Cell::new(false),
                close_btn: RefCell::new(None),
                label: RefCell::new(None),
                tracking: RefCell::new(None),
                text: RefCell::new(text.to_string()),
            };
            let this = mtm.alloc().set_ivars(ivars);
            // SAFETY: `initWithFrame:` is the documented non-raising NSView initializer.
            let this: Retained<Self> = unsafe { msg_send_id![super(this), initWithFrame: frame] };

            // The close × button: a small borderless title button (factory initializer,
            // NEVER `initWithFrame`). Its action targets THIS view's `closeTab:`.
            // SAFETY: `buttonWithTitle:target:action:` is the documented factory; plain
            // setters follow on the fresh button; all on the main thread.
            let close = unsafe {
                let view_obj: &AnyObject = &this;
                let btn = NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("✕"),
                    Some(view_obj),
                    Some(sel!(closeTab:)),
                    mtm,
                );
                btn.setBordered(false);
                btn.setImagePosition(NSCellImagePosition::NSNoImage);
                let close_font = NSFont::systemFontOfSize(10.0);
                btn.setFont(Some(&close_font));
                let close_frame = CGRect::new(
                    CGPoint::new(3.0, (frame.size.height - CLOSE_W) / 2.0),
                    CGSize::new(CLOSE_W, CLOSE_W),
                );
                btn.setFrame(close_frame);
                btn.setToolTip(Some(&NSString::from_str("Close Tab")));
                this.addSubview(&btn);
                btn
            };

            // The title label: a non-editable, non-bezeled label (factory initializer).
            // SAFETY: `labelWithString:` is the documented non-raising factory; plain
            // setters follow; on the main thread.
            let label = unsafe {
                let lbl = NSTextField::labelWithString(&NSString::from_str(text), mtm);
                lbl.setDrawsBackground(false);
                lbl.setBezeled(false);
                lbl.setEditable(false);
                lbl.setSelectable(false);
                lbl.setAlignment(NSTextAlignment::Left);
                // Single-line, truncating-tail (the `labelWithString:` default is a
                // truncating single-line label; force single-line to be explicit and
                // keep the row height exact).
                lbl.setUsesSingleLineMode(true);
                // Active = full label color; inactive = secondary (dim), Ghostty-like.
                let color = if active {
                    NSColor::labelColor()
                } else {
                    NSColor::secondaryLabelColor()
                };
                lbl.setTextColor(Some(&color));
                let label_font = NSFont::systemFontOfSize(12.0);
                lbl.setFont(Some(&label_font));
                let label_x = 3.0 + CLOSE_W + 2.0;
                let label_w = (frame.size.width - label_x - 6.0).max(0.0);
                let label_frame = CGRect::new(
                    CGPoint::new(label_x, (frame.size.height - 17.0) / 2.0),
                    CGSize::new(label_w, 17.0),
                );
                lbl.setFrame(label_frame);
                this.addSubview(&lbl);
                lbl
            };

            *this.ivars().close_btn.borrow_mut() = Some(close);
            *this.ivars().label.borrow_mut() = Some(label);
            this.refresh_close_visibility();
            this.install_tracking_area();
            this
        }

        /// Request a repaint of the whole tab (after a hover / active change). Wraps the
        /// `unsafe` `setNeedsDisplay:` setter, which is sound on the main thread.
        fn mark_dirty(&self) {
            // SAFETY: side-effect-free invalidation request on the live view, main thread.
            unsafe { self.setNeedsDisplay(true) };
        }

        /// Show the close × when this tab is ACTIVE or HOVERED; hide it otherwise, so an
        /// idle inactive tab is just its label (Ghostty reveals the × on hover).
        fn refresh_close_visibility(&self) {
            let ivars = self.ivars();
            let show = ivars.active.get() || ivars.hovered.get();
            if let Some(btn) = ivars.close_btn.borrow().as_ref() {
                btn.setHidden(!show);
            }
        }

        /// (Re)install a `mouseEnteredAndExited` tracking area covering the whole view
        /// (it follows resizes via `InVisibleRect`), removing any prior one.
        fn install_tracking_area(&self) {
            let mtm = MainThreadMarker::from(self);
            // SAFETY: remove the previous area, then build + add a fresh one covering the
            // current bounds — all standard AppKit calls on the main thread.
            unsafe {
                if let Some(old) = self.ivars().tracking.borrow_mut().take() {
                    self.removeTrackingArea(&old);
                }
                let opts = NSTrackingAreaOptions::NSTrackingMouseEnteredAndExited
                    | NSTrackingAreaOptions::NSTrackingActiveAlways
                    | NSTrackingAreaOptions::NSTrackingInVisibleRect;
                let area = NSTrackingArea::initWithRect_options_owner_userInfo(
                    mtm.alloc(),
                    self.bounds(),
                    opts,
                    Some(self),
                    None,
                );
                self.addTrackingArea(&area);
                *self.ivars().tracking.borrow_mut() = Some(area);
            }
        }

        /// This tab's introspection label (title + "  ⌘N"), for `read_tab_chrome`.
        fn label_text(&self) -> String {
            self.ivars().text.borrow().clone()
        }

        /// Whether this tab is the active one (the selected segment, for introspection).
        fn is_active(&self) -> bool {
            self.ivars().active.get()
        }
    }

    /// The toolbar item identifier for the full-width tab-strip custom view.
    const STRIP_ITEM_ID: &str = "aterm.tabstrip";

    /// The delegate's ivars: the retained strip CONTAINER view (the toolbar item's
    /// custom view), wrapped into the item on demand.
    pub(crate) struct DelegateIvars {
        container: Retained<NSView>,
    }

    declare_class!(
        /// The `NSToolbarDelegate`: vends the toolbar's single item identifier and
        /// builds ONE `NSToolbarItem` whose custom `view` IS the full-width tab-strip
        /// container (per-tab views + "+"). `UnifiedCompact` then renders the whole
        /// titlebar+toolbar as a SINGLE compact row.
        ///
        /// `MainThreadOnly` mutability is REQUIRED: `NSToolbarDelegate: IsMainThreadOnly`.
        pub(crate) struct ToolbarDelegate;

        // SAFETY:
        // - NSObject imposes no subclassing requirements.
        // - MainThreadOnly is required by the NSToolbarDelegate protocol bound and is
        //   sound: the delegate is created and only ever messaged on the main thread.
        // - ToolbarDelegate has no Drop impl beyond the auto-generated ivar drop.
        unsafe impl ClassType for ToolbarDelegate {
            type Super = objc2::runtime::NSObject;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "ATermToolbarDelegate";
        }

        impl DeclaredClass for ToolbarDelegate {
            type Ivars = DelegateIvars;
        }

        unsafe impl ToolbarDelegate {}

        unsafe impl NSObjectProtocol for ToolbarDelegate {}

        unsafe impl NSToolbarDelegate for ToolbarDelegate {
            /// The items shown by DEFAULT: just the tab-strip custom-view item.
            #[method_id(toolbarDefaultItemIdentifiers:)]
            fn default_item_identifiers(
                &self,
                _toolbar: &NSToolbar,
            ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
                Self::item_identifiers()
            }

            /// The items the toolbar MAY contain: the same single set.
            #[method_id(toolbarAllowedItemIdentifiers:)]
            fn allowed_item_identifiers(
                &self,
                _toolbar: &NSToolbar,
            ) -> Retained<NSArray<NSToolbarItemIdentifier>> {
                Self::item_identifiers()
            }

            /// Build the `NSToolbarItem` for the strip identifier: an item whose custom
            /// `view` is the retained full-width strip container. Any other identifier
            /// yields `None`.
            #[method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:)]
            fn item_for_identifier(
                &self,
                _toolbar: &NSToolbar,
                identifier: &NSToolbarItemIdentifier,
                _will_be_inserted: bool,
            ) -> Option<Retained<NSToolbarItem>> {
                let mtm = MainThreadMarker::from(self);
                let ivars = self.ivars();
                if *identifier == *NSString::from_str(STRIP_ITEM_ID) {
                    // SAFETY: standard NSToolbarItem construction + plain setters on a
                    // fresh instance, on the main thread (`mtm`). The container view is
                    // retained in the delegate's ivar (and the handle), outliving it.
                    Some(unsafe {
                        let item =
                            NSToolbarItem::initWithItemIdentifier(mtm.alloc(), identifier);
                        let label = NSString::from_str("Tabs");
                        item.setLabel(&label);
                        item.setPaletteLabel(&label);
                        item.setView(Some(&ivars.container));
                        // A plain custom NSView has NO intrinsic size, so without an
                        // explicit min/max the `UnifiedCompact` toolbar collapses it to
                        // zero width and overflows it behind a `»` chevron. Give it a
                        // generous span — a small minimum so it never vanishes, and a
                        // very large maximum so it stretches to fill the whole toolbar
                        // (the container's `WidthSizable` mask + `set_window_tabs`
                        // re-layout then size the tabs to the real width).
                        //
                        // `setMinSize`/`setMaxSize` are soft-deprecated in favor of Auto
                        // Layout constraints, but they are the SIMPLEST non-raising way
                        // to size a custom-view toolbar item full-width here (an Auto
                        // Layout width constraint that also stretches inside a toolbar
                        // item is markedly more code + two more AppKit features, for no
                        // user-visible gain). Suppressed intentionally — they are plain,
                        // crash-safe setters.
                        #[allow(deprecated)]
                        {
                            item.setMinSize(CGSize::new(STRIP_MIN_WIDTH, STRIP_HEIGHT));
                            item.setMaxSize(CGSize::new(STRIP_MAX_WIDTH, STRIP_HEIGHT));
                        }
                        item
                    })
                } else {
                    None
                }
            }
        }
    );

    impl ToolbarDelegate {
        fn new(mtm: MainThreadMarker, container: Retained<NSView>) -> Retained<Self> {
            let this = mtm.alloc().set_ivars(DelegateIvars { container });
            // SAFETY: plain `[super init]` on a freshly allocated instance.
            unsafe { msg_send_id![super(this), init] }
        }

        fn item_identifiers() -> Retained<NSArray<NSToolbarItemIdentifier>> {
            let strip = NSString::from_str(STRIP_ITEM_ID);
            NSArray::from_id_slice(&[strip])
        }
    }

    /// Attach the native window chrome to `window`: a SINGLE compact Ghostty-style row
    /// — a full-width VIEW-BASED TAB STRIP (per-tab [`TabView`]s) plus a trailing "+"
    /// New Tab `NSButton`, hosted as ONE custom-view `NSToolbarItem` in a
    /// `UnifiedCompact` `NSToolbar`. The strip starts EMPTY (0 tabs) and HIDDEN; the
    /// caller's first `App::sync_window` calls [`set_window_tabs`] to populate + reveal
    /// it at 2+ tabs.
    ///
    /// Best-effort: off the main thread or with no AppKit `NSWindow`, the chrome is
    /// simply not installed (`None`) — never a panic.
    pub fn install_window_toolbar(
        window: &winit::window::Window,
        proxy: &EventLoopProxy<Wake>,
        wid: WindowId,
    ) -> Option<ToolbarHandle> {
        let mtm = MainThreadMarker::new()?;

        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return None;
        };
        // SAFETY: `ns_view` points at this window's live NSView (owned by winit for the
        // window's lifetime); we only borrow it — on the main thread — to read its
        // `window` and attach the toolbar.
        let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
        let ns_window = view.window()?;

        let tabs_target = TabsTarget::new(mtm, proxy.clone(), wid);

        let win_w = ns_window.frame().size.width.max(200.0);

        // The trailing "+" New Tab button, pinned to the RIGHT end of the strip.
        // SAFETY: `buttonWithImage:target:action:` / `buttonWithTitle:target:action:`
        // are the documented factory initializers; plain setters follow on the fresh
        // button; all on the main thread. `newTab:` exists on TabsTarget.
        let plus = unsafe {
            let frame = CGRect::new(
                CGPoint::new(win_w - PLUS_WIDTH, 0.0),
                CGSize::new(PLUS_WIDTH, STRIP_HEIGHT),
            );
            let tabs_obj: &AnyObject = &tabs_target;
            let action = Some(sel!(newTab:));
            let btn = match NSImage::imageNamed(NSImageNameAddTemplate) {
                Some(image) => {
                    NSButton::buttonWithImage_target_action(&image, Some(tabs_obj), action, mtm)
                }
                None => NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("+"),
                    Some(tabs_obj),
                    action,
                    mtm,
                ),
            };
            btn.setFrame(frame);
            btn.setBordered(false);
            btn.setImagePosition(NSCellImagePosition::NSImageOnly);
            btn.setToolTip(Some(&NSString::from_str("New Tab")));
            btn.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewMinXMargin
                    | NSAutoresizingMaskOptions::NSViewMaxYMargin,
            );
            btn
        };

        // The strip's container view, hosting the per-tab views (built later in
        // `set_window_tabs`) + the "+" button. Starts hidden.
        // SAFETY: standard NSView construction + setters on a fresh instance, on the
        // main thread; `addSubview:` takes the live "+" button (retained).
        let container = unsafe {
            let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(win_w, STRIP_HEIGHT));
            let v = NSView::initWithFrame(mtm.alloc(), frame);
            v.setAutoresizingMask(NSAutoresizingMaskOptions::NSViewWidthSizable);
            v.addSubview(&plus);
            v.setHidden(true);
            v
        };

        let delegate = ToolbarDelegate::new(mtm, container.clone());

        // SAFETY: standard NSToolbar / NSWindow setup, all on the main thread.
        let toolbar = unsafe {
            let identifier = NSString::from_str("aterm.toolbar");
            let toolbar = NSToolbar::initWithIdentifier(mtm.alloc(), &identifier);
            let delegate_proto = ProtocolObject::from_ref(&*delegate);
            toolbar.setDelegate(Some(delegate_proto));
            toolbar.setAllowsUserCustomization(false);
            toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
            ns_window.setToolbar(Some(&toolbar));
            ns_window.setToolbarStyle(NSWindowToolbarStyle::UnifiedCompact);
            ns_window.setTitleVisibility(NSWindowTitleVisibility::NSWindowTitleHidden);
            ns_window.setTitlebarAppearsTransparent(true);
            if let Some(dark) = NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua) {
                // SAFETY: a standard AppKit setter taking a nullable NSAppearance, on
                // the main thread; `dark` outlives the call.
                let _: () = objc2::msg_send![&*ns_window, setAppearance: Some(&*dark)];
            }
            // Start HIDDEN: a fresh window has a single tab, so there is no strip to
            // show. Hiding the whole toolbar collapses the titlebar to a bare, seamless
            // compact bar (just traffic lights) — `set_window_tabs` reveals it at 2+
            // tabs. This also avoids the empty macOS toolbar-item capsule flashing at
            // launch.
            toolbar.setVisible(false);
            toolbar
        };

        Some(ToolbarHandle {
            _tabs_target: tabs_target,
            _delegate: delegate,
            toolbar,
            container,
            proxy: proxy.clone(),
            window: wid,
            tabs: RefCell::new(Vec::new()),
            _plus: plus,
        })
    }

    /// Re-sync the title-bar tab STRIP to the current app tab state: `titles` (one
    /// label per tab) and the 0-based `active` index. Called from
    /// `App::refresh_window_tabs` after every tab open / close / switch / detach /
    /// migrate.
    ///
    /// HIDE-WHEN-≤1: a window with 0 or 1 tab shows NO strip (the container is hidden),
    /// exactly like Ghostty / native macOS apps. At 2+ tabs the per-tab [`TabView`]s
    /// are rebuilt: the old set is removed as subviews + dropped, a fresh view is built
    /// per title (laid out left→right between the traffic-light inset and the "+"), the
    /// `active` one accented, and the container un-hidden.
    pub fn set_window_tabs(handle: &ToolbarHandle, titles: &[String], active: usize) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let container = &handle.container;

        if titles.len() <= 1 {
            // ≤1 tab: NO strip. Hide the container AND the whole toolbar so the titlebar
            // collapses to a bare, seamless terminal-coloured compact bar (just traffic
            // lights) — "don't show the blank bar". Hiding the toolbar also removes the
            // empty macOS toolbar-item capsule. Old tab views stay in place (off screen);
            // the next 2+-tab sync rebuilds them.
            container.setHidden(true);
            // SAFETY: a plain main-thread setter on the live toolbar (mtm proven above).
            unsafe { handle.toolbar.setVisible(false) };
            return;
        }
        // 2+ tabs: reveal the toolbar so the tab strip shows (no-op if already visible).
        // SAFETY: a plain main-thread setter on the live toolbar (mtm proven above).
        unsafe { handle.toolbar.setVisible(true) };

        // Remove the previous tab views from the container (and drop our retained
        // copies). The "+" button stays (it was added once at install).
        // SAFETY: `removeFromSuperview` is a plain main-thread mutator.
        {
            let mut tabs = handle.tabs.borrow_mut();
            for old in tabs.drain(..) {
                unsafe { old.removeFromSuperview() };
            }
        }

        // Compute the tab band: between the traffic-light inset and the "+".
        let total_w = container
            .frame()
            .size
            .width
            .max(TRAFFIC_LIGHT_INSET + PLUS_WIDTH + 1.0);
        let band_w = (total_w - TRAFFIC_LIGHT_INSET - PLUS_WIDTH).max(1.0);
        let n = titles.len();
        // Equal share, clamped so tabs neither eat the whole bar nor vanish.
        let per = (band_w / n as f64).clamp(MIN_TAB_W, MAX_TAB_W);
        let active = active.min(n.saturating_sub(1));

        let mut new_tabs: Vec<Retained<TabView>> = Vec::with_capacity(n);
        let mut x = TRAFFIC_LIGHT_INSET;
        for (i, title) in titles.iter().enumerate() {
            // Ghostty-style label: title + ⌘-number switch hint (⌘1..⌘9).
            let shown = if i < 9 {
                format!("{title}  ⌘{}", i + 1)
            } else {
                title.clone()
            };
            // Last tab soaks up any rounding remainder up to the band edge so the row
            // tiles without a seam; a 1pt gutter separates tabs.
            let w = if i + 1 == n {
                (TRAFFIC_LIGHT_INSET + band_w - x).max(MIN_TAB_W).min(per)
            } else {
                per - 1.0
            };
            let frame = CGRect::new(CGPoint::new(x, 0.0), CGSize::new(w.max(1.0), STRIP_HEIGHT));
            let tab = TabView::build(
                mtm,
                handle.proxy.clone(),
                handle.window,
                i,
                n,
                &shown,
                i == active,
                frame,
            );
            // SAFETY: `addSubview:` on the live container, main thread.
            unsafe { container.addSubview(&tab) };
            new_tabs.push(tab);
            x += per;
        }
        *handle.tabs.borrow_mut() = new_tabs;
        container.setHidden(false);
    }

    /// Read the title-bar TAB STRIP's introspection line for the `chrome` verb, or
    /// `None` when the strip is HIDDEN (≤1 tab). When visible it returns one line of
    /// the form `toolbar-tabs count=<n> selected=<i> labels=["zsh  ⌘1", ...]`, read off
    /// the live [`TabView`]s (their count / active flag / label text) — the true tab
    /// set (the "+" is a separate button, not a tab view).
    pub fn read_tab_chrome(handle: &ToolbarHandle) -> Option<String> {
        MainThreadMarker::new()?;
        // SAFETY: `isHidden` is a side-effect-free getter on the live container, main
        // thread.
        let hidden = unsafe { handle.container.isHidden() };
        if hidden {
            return None;
        }
        let tabs = handle.tabs.borrow();
        let count = tabs.len() as isize;
        // The selected index is the active tab's position (or -1 if none — never, at
        // 2+ tabs there is always exactly one active).
        let selected = tabs
            .iter()
            .position(|t| t.is_active())
            .map_or(-1isize, |i| i as isize);
        let labels: Vec<String> = tabs.iter().map(|t| t.label_text()).collect();
        Some(format!(
            "toolbar-tabs count={count} selected={selected} labels={labels:?}"
        ))
    }
}
