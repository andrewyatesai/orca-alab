// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The application-runtime (`apprt`) seam: the ONE place that names every native
//! OS-integration operation aterm performs (window chrome colour/appearance, the
//! menu bar, the per-window toolbar tab strip, and the desktop-notification
//! delivery thread). It exists so `main.rs` / `app_window.rs` / `app_tabs.rs`
//! stay platform-NEUTRAL: they call through an [`AppRt`] instance and never name
//! AppKit/objc2 directly.
//!
//! Two impls back the one trait:
//!
//! * [`AppRtMacOS`] WRAPS the existing objc2 calls EXACTLY — the NSWindow
//!   colour-space + NSAppearance/titlebar logic moved here verbatim from
//!   `app_window.rs`, and the menu/toolbar/notify methods forward straight to the
//!   already-`cfg(macos)`-guarded [`crate::menu`] / [`crate::toolbar`] /
//!   [`crate::notify`] modules. So the macOS binary is behaviorally identical: the
//!   same objc2 operations run, just reached through the trait.
//! * [`AppRtLinux`] is the no-op fallback for every non-macOS target: chrome
//!   colour/appearance do nothing, the menu/toolbar install nothing (`None`), the
//!   tab-strip sync + chrome read are no-ops, and notification delivery spins the
//!   same channel-draining stub `notify::spawn_delivery` already provides. The
//!   terminal renders + input works; native chrome is gracefully absent.
//!
//! [`PlatformAppRt`] is the cfg-selected concrete type the `App` stores: the macOS
//! impl on macOS, the Linux impl everywhere else. Both are zero-sized, so the
//! field costs nothing.

use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::app_config::WindowTheme;
use crate::notify::NotifyMsg;
use crate::{Wake, WindowId, menu, toolbar};

/// The native application-runtime seam. Every method is a platform OS-integration
/// operation aterm performs; the macOS impl runs the real objc2 calls, the Linux
/// impl is a graceful no-op. Implementors are zero-sized.
pub(crate) trait AppRt {
    /// Paint the OS window background the terminal's theme background colour
    /// (`bg`, as `0x00RRGGBB`) so the transparent titlebar / bare compact bar reads
    /// as a seamless extension of the terminal body. No-op off macOS.
    fn window_set_background_color(&self, window: &Window, bg: u32);

    /// Apply the window-CHROME appearance: match the NSWindow colour space to
    /// softbuffer's device-RGB content (dropping the per-frame CoreAnimation gamut
    /// conversion) and set the titlebar light/dark appearance from `theme`. No-op
    /// off macOS.
    fn window_set_appearance(&self, window: &Window, theme: WindowTheme);

    /// Spawn the process-wide notification delivery thread and return the `Sender`
    /// each tab clones into its engine callbacks. Off macOS this is the
    /// channel-draining stub (senders never block; nothing is delivered).
    fn send_notification_init(&self, suppress: Arc<Mutex<HashSet<u64>>>) -> Sender<NotifyMsg>;

    /// Build + install the native application menu bar, returning the retained
    /// action target the caller keeps alive. `None` off macOS (no menu installed).
    fn install_menu(&self, proxy: &EventLoopProxy<Wake>) -> Option<menu::MenuHandle>;

    /// Install the per-window native toolbar (the full-width tab strip + "+"
    /// button) for logical window `wid`, returning the retained backing handle.
    /// `None` off macOS (no toolbar installed).
    fn install_toolbar(
        &self,
        window: &Window,
        proxy: &EventLoopProxy<Wake>,
        wid: WindowId,
    ) -> Option<toolbar::ToolbarHandle>;

    /// Re-sync a window's native toolbar tab strip to `titles` with the `active`
    /// index selected. No-op off macOS / when no handle exists.
    fn set_toolbar_tabs(&self, handle: &toolbar::ToolbarHandle, titles: &[String], active: usize);

    /// Read the native toolbar's tab-switcher chrome as one introspection line
    /// (segment count / selected / labels), or `None` when there is nothing to
    /// report. `None` off macOS.
    fn read_toolbar_chrome(&self, handle: &toolbar::ToolbarHandle) -> Option<String>;
}

/// macOS application-runtime: WRAPS the existing objc2 integration exactly. The
/// chrome methods carry the verbatim NSWindow colour-space + NSAppearance logic
/// relocated from `app_window.rs`; the rest forward to the `cfg(macos)`-guarded
/// menu/toolbar/notify modules. Zero-sized.
#[cfg(target_os = "macos")]
pub(crate) struct AppRtMacOS;

#[cfg(target_os = "macos")]
impl AppRt for AppRtMacOS {
    /// Paint the NSWindow background the terminal's theme background colour (`bg`,
    /// as `0x00RRGGBB`), so the transparent titlebar and the bare single-tab
    /// compact bar read as a SEAMLESS extension of the terminal body rather than a
    /// distinct, lighter chrome strip. This is the window-level half of the Ghostty
    /// "transparent" titlebar look (the toolbar.rs strip toggling is the other
    /// half). The terminal content view (softbuffer/Metal layer) paints its own
    /// background over the content area, so this colour only ever shows in the
    /// titlebar region the content view does not cover.
    ///
    /// Best-effort, mirroring [`Self::window_set_appearance`]: off the main thread
    /// or with no AppKit `NSWindow`, it is simply a no-op.
    fn window_set_background_color(&self, window: &Window, bg: u32) {
        use objc2_app_kit::{NSColor, NSView};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return;
        };
        // SAFETY: `ns_view` points at this window's live NSView (owned by winit for
        // the window's lifetime); we only borrow it — on the main thread, as AppKit
        // requires — to reach its `window` and set the background colour.
        let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
        let Some(ns_window) = view.window() else {
            return;
        };
        let r = f64::from((bg >> 16) & 0xff) / 255.0;
        let g = f64::from((bg >> 8) & 0xff) / 255.0;
        let b = f64::from(bg & 0xff) / 255.0;
        // SAFETY: standard AppKit colour construction + a plain setter on the main
        // thread; the colour is autoreleased and consumed within this call.
        unsafe {
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
            ns_window.setBackgroundColor(Some(&color));
        }
    }

    /// Make the window's colour space match softbuffer's device-RGB content so
    /// CoreAnimation does NOT run a per-frame colour-space conversion on the main
    /// thread. softbuffer (`backends/cg.rs`) builds its CGImage with
    /// `CGColorSpace::new_device_rgb()`; on a wide-gamut (P3) display CoreAnimation
    /// otherwise converts device-RGB → display-P3 on *every* commit
    /// (`CA::Render::prepare_image` → `vImageConvert_AnyToAny`) — profiled at ~half
    /// of all present cost during heavy output. Tagging the NSWindow device-RGB
    /// makes content and window the same space, so the conversion is skipped; the
    /// final space→panel mapping is done once by the WindowServer, not per app
    /// frame. aterm's framebuffer pixels are unchanged — only the redundant gamut
    /// round-trip is removed. `$ATERM_NO_COLORSPACE_MATCH` opts out.
    fn window_set_appearance(&self, window: &Window, theme: WindowTheme) {
        use objc2_app_kit::{NSColorSpace, NSView};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return;
        };
        // SAFETY: `ns_view` points at this window's live NSView (owned by winit for
        // the window's lifetime); we only borrow it — on the main thread, as AppKit
        // requires — to read its `window` and configure it.
        let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
        let Some(ns_window) = view.window() else {
            return;
        };
        // Colour-space match (device-RGB) — see fn doc. SAFETY: standard AppKit calls.
        if std::env::var_os("ATERM_NO_COLORSPACE_MATCH").is_none() {
            unsafe {
                let cs = NSColorSpace::deviceRGBColorSpace();
                ns_window.setColorSpace(Some(&cs));
            }
        }
        // Ghostty-style unified chrome: a transparent titlebar so the window frame
        // (titlebar + traffic lights) reads as a seamless extension of the terminal
        // body. The titlebar's LIGHT/DARK appearance now follows config `window_theme`
        // ([`WindowTheme`]): Dark -> NSAppearanceNameDarkAqua, Light ->
        // NSAppearanceNameAqua, Auto -> leave the appearance UNSET so the window tracks
        // the OS `effectiveAppearance` (including live day-night switches). This
        // replaces the old unconditional dark force that left light-desktop users with
        // permanently dark chrome. `ATERM_NO_DARK_CHROME` still forces Auto (no
        // override) regardless of config, for callers that scripted the old opt-out.
        // SAFETY: `appearanceNamed:`/`setAppearance:`/`setTitlebarAppearsTransparent:`
        // are standard NSWindow/NSAppearance calls on the main thread; the appearance
        // object is autoreleased and used immediately within this pool.
        let resolved = if std::env::var_os("ATERM_NO_DARK_CHROME").is_some() {
            WindowTheme::Auto
        } else {
            theme
        };
        let appearance_name: Option<&str> = match resolved {
            WindowTheme::Auto => None,
            WindowTheme::Light => Some("NSAppearanceNameAqua"),
            WindowTheme::Dark => Some("NSAppearanceNameDarkAqua"),
        };
        unsafe {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            use objc2_foundation::NSString;
            if let Some(name) = appearance_name {
                let name = NSString::from_str(name);
                let appearance: *mut AnyObject =
                    msg_send![class!(NSAppearance), appearanceNamed: &*name];
                if !appearance.is_null() {
                    let _: () = msg_send![&*ns_window, setAppearance: appearance];
                }
            }
            // Transparent titlebar is desired in every mode (it is the
            // chrome-unification half, independent of light/dark).
            let _: () = msg_send![&*ns_window, setTitlebarAppearsTransparent: true];
        }
    }

    fn send_notification_init(&self, suppress: Arc<Mutex<HashSet<u64>>>) -> Sender<NotifyMsg> {
        crate::notify::spawn_delivery(suppress)
    }

    fn install_menu(&self, proxy: &EventLoopProxy<Wake>) -> Option<menu::MenuHandle> {
        menu::install(proxy)
    }

    fn install_toolbar(
        &self,
        window: &Window,
        proxy: &EventLoopProxy<Wake>,
        wid: WindowId,
    ) -> Option<toolbar::ToolbarHandle> {
        toolbar::install_window_toolbar(window, proxy, wid)
    }

    fn set_toolbar_tabs(&self, handle: &toolbar::ToolbarHandle, titles: &[String], active: usize) {
        toolbar::set_window_tabs(handle, titles, active);
    }

    fn read_toolbar_chrome(&self, handle: &toolbar::ToolbarHandle) -> Option<String> {
        toolbar::read_tab_chrome(handle)
    }
}

/// Non-macOS application-runtime: every native OS-integration operation is a
/// graceful no-op. Chrome colour/appearance do nothing, the menu/toolbar install
/// nothing (`None`), the tab-strip sync + chrome read are no-ops, and notification
/// delivery forwards to `notify::spawn_delivery`'s channel-draining stub. So the
/// Linux binary builds + runs (terminal renders + input works) with native chrome,
/// menus, toolbars, and notifications gracefully absent. Zero-sized.
#[cfg(not(target_os = "macos"))]
pub(crate) struct AppRtLinux;

#[cfg(not(target_os = "macos"))]
impl AppRt for AppRtLinux {
    fn window_set_background_color(&self, _window: &Window, _bg: u32) {}

    fn window_set_appearance(&self, _window: &Window, _theme: WindowTheme) {}

    fn send_notification_init(&self, suppress: Arc<Mutex<HashSet<u64>>>) -> Sender<NotifyMsg> {
        crate::notify::spawn_delivery(suppress)
    }

    // The branches below delegate to the `menu::`/`toolbar::` stubs (themselves
    // no-ops off macOS) rather than returning `None`/`()` inline, so those stubs
    // stay live call sites — one platform surface, no dead code on Linux.
    fn install_menu(&self, proxy: &EventLoopProxy<Wake>) -> Option<menu::MenuHandle> {
        menu::install(proxy)
    }

    fn install_toolbar(
        &self,
        window: &Window,
        proxy: &EventLoopProxy<Wake>,
        wid: WindowId,
    ) -> Option<toolbar::ToolbarHandle> {
        toolbar::install_window_toolbar(window, proxy, wid)
    }

    fn set_toolbar_tabs(&self, handle: &toolbar::ToolbarHandle, titles: &[String], active: usize) {
        toolbar::set_window_tabs(handle, titles, active);
    }

    fn read_toolbar_chrome(&self, handle: &toolbar::ToolbarHandle) -> Option<String> {
        toolbar::read_tab_chrome(handle)
    }
}

/// The concrete application-runtime the `App` stores, selected at compile time:
/// [`AppRtMacOS`] on macOS, [`AppRtLinux`] everywhere else. Zero-sized, so the
/// `App.apprt` field costs nothing.
#[cfg(target_os = "macos")]
pub(crate) type PlatformAppRt = AppRtMacOS;

/// See the macOS variant above — this is the non-macOS selection.
#[cfg(not(target_os = "macos"))]
pub(crate) type PlatformAppRt = AppRtLinux;

/// Construct the platform application-runtime for this build target. The single
/// place `App` mints its `apprt` field, so the cfg lives here, not at the
/// construction sites.
pub(crate) fn platform_apprt() -> PlatformAppRt {
    #[cfg(target_os = "macos")]
    {
        AppRtMacOS
    }
    #[cfg(not(target_os = "macos"))]
    {
        AppRtLinux
    }
}
