// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The native macOS application MENU BAR (the Apple `NSMenu` main menu).
//!
//! aterm installs a standard Mac menu bar — App (aterm) / File / Edit / View /
//! Window / Help with the usual items — so it presents as a native app. The menu
//! is built and installed once, after the window exists, in [`crate::App::resumed`]
//! (skipped under `--headless`, so tests create no menu and stay byte-identical).
//!
//! **No behavior duplication.** A menu item is a thin DISPATCH stub: it carries a
//! VISUAL key-equivalent only (so the shortcut shows next to the item) and, when
//! clicked, posts a [`Wake::MenuAction`](crate::Wake) carrying a [`MenuAction`].
//! The main loop's `user_event` turns that into a call on the SAME `App` command
//! method the existing keybinding uses (see `App::dispatch_menu_action`). The
//! real keypresses still flow through `App::on_key` exactly as before — the menu
//! adds a second entry point to the existing commands, never a parallel one.
//!
//! Each item's [`MenuAction`] is encoded in its `NSMenuItem.tag` (a plain
//! integer), so a SINGLE Objective-C action selector (`menuAction:`) reads the
//! sender's tag and forwards it — no per-item method, no per-item Rust object.
//! The action target is a small custom `NSObject` subclass that owns an
//! [`EventLoopProxy<Wake>`]; AppKit holds a target only weakly, so [`install`]
//! returns the retained target for the caller (`App`) to keep alive for the whole
//! run loop.
//!
//! Everything imperative is `#[cfg(target_os = "macos")]`. On other targets the
//! [`MenuAction`] enum and a no-op [`install`] still exist so the workspace builds
//! everywhere and `Wake::MenuAction { action }` is a valid variant on every target.

/// One menu command, identified independently of AppKit. The discriminant is the
/// integer stored in the originating `NSMenuItem.tag` and round-tripped back via
/// [`MenuAction::from_tag`]; `user_event` matches on the value to call the
/// matching existing `App` command method (`App::dispatch_menu_action`).
///
/// Standard AppKit responder items (window minimise/zoom/fullscreen, hide, quit)
/// are routed through here too, rather than via `nil`-target responder selectors,
/// so the WHOLE menu has one uniform, auditable dispatch path that lands in `App`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    // App menu
    /// About aterm (no-op stub for now).
    About,
    /// Preferences… (no-op stub for now).
    Preferences,
    /// Hide aterm (the app — `NSApplication::hide`).
    Hide,
    /// Quit aterm.
    Quit,
    // File menu
    /// New Window — a fresh independent aterm process (`open_new_window`).
    NewWindow,
    /// New Tab — a new in-window session (`App::open_tab`).
    NewTab,
    /// Move Tab to New Window — pull the active tab out into a fresh in-process
    /// window (`App::detach_active_tab`).
    MoveTabToNewWindow,
    /// Move Tab to Next Window — move the active tab into the NEXT EXISTING window
    /// (wrapping; `App::migrate_active_tab_to_next_window`).
    MoveTabToNextWindow,
    /// Open Session in New Window — show the active session in a SECOND window
    /// (same live grid in two windows; `App::open_active_session_in_new_window`).
    ViewSessionInNewWindow,
    /// Close Tab — close the active tab (`App::close_active_tab`).
    CloseTab,
    // Edit menu
    /// Copy the selection (`App::copy_selection`).
    Copy,
    /// Paste the clipboard (`App::paste_clipboard`).
    Paste,
    /// Select All (`App::select_all`).
    SelectAll,
    /// Find… — enter Cmd-F find mode (`App::search_enter`).
    Find,
    // View menu
    /// Toggle the window's full-screen state (winit `set_fullscreen`).
    ToggleFullScreen,
    // Window menu
    /// Minimise the window.
    Minimize,
    /// Zoom (toggle maximised) the window.
    Zoom,
    // Help menu
    /// Help (no-op stub for now).
    Help,
}

impl MenuAction {
    /// The integer stored in the menu item's `tag`. Stable, dense, starting at 1
    /// (0 is the `NSMenuItem` default tag, reserved so an untagged item never
    /// looks like a real action).
    #[must_use]
    pub fn tag(self) -> isize {
        match self {
            MenuAction::About => 1,
            MenuAction::Preferences => 2,
            MenuAction::Hide => 3,
            MenuAction::Quit => 4,
            MenuAction::NewWindow => 5,
            MenuAction::NewTab => 6,
            MenuAction::CloseTab => 7,
            MenuAction::Copy => 8,
            MenuAction::Paste => 9,
            MenuAction::SelectAll => 10,
            MenuAction::Find => 11,
            MenuAction::ToggleFullScreen => 12,
            MenuAction::Minimize => 13,
            MenuAction::Zoom => 14,
            MenuAction::Help => 15,
            MenuAction::MoveTabToNewWindow => 16,
            MenuAction::ViewSessionInNewWindow => 17,
            MenuAction::MoveTabToNextWindow => 18,
        }
    }

    /// Inverse of [`MenuAction::tag`]: recover the action from a menu item's tag,
    /// or `None` for an unknown/zero tag (defensive — the action selector ignores
    /// a tag it can't decode rather than dispatching the wrong command).
    #[must_use]
    pub fn from_tag(tag: isize) -> Option<MenuAction> {
        Some(match tag {
            1 => MenuAction::About,
            2 => MenuAction::Preferences,
            3 => MenuAction::Hide,
            4 => MenuAction::Quit,
            5 => MenuAction::NewWindow,
            6 => MenuAction::NewTab,
            7 => MenuAction::CloseTab,
            8 => MenuAction::Copy,
            9 => MenuAction::Paste,
            10 => MenuAction::SelectAll,
            11 => MenuAction::Find,
            12 => MenuAction::ToggleFullScreen,
            13 => MenuAction::Minimize,
            14 => MenuAction::Zoom,
            15 => MenuAction::Help,
            16 => MenuAction::MoveTabToNewWindow,
            17 => MenuAction::ViewSessionInNewWindow,
            18 => MenuAction::MoveTabToNextWindow,
            _ => return None,
        })
    }
}

#[cfg(target_os = "macos")]
pub use macos::{MenuHandle, install, show_about_panel};

/// Non-macOS no-op handle: there is no platform menu off macOS. Held by `App` in
/// the same field on every target so the struct shape is platform-independent.
#[cfg(not(target_os = "macos"))]
pub type MenuHandle = ();

/// Non-macOS stub: no platform menu bar exists, so installing one is a no-op that
/// installs nothing (`None`). Returns `Option<MenuHandle>` so the `resumed` call
/// site (`self._menu = menu::install(..)`) is identical on every target.
#[cfg(not(target_os = "macos"))]
pub fn install(_proxy: &winit::event_loop::EventLoopProxy<crate::Wake>) -> Option<MenuHandle> {
    None
}

/// Non-macOS stub: no platform About panel exists off macOS.
#[cfg(not(target_os = "macos"))]
pub fn show_about_panel() {}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Sel};
    use objc2::{ClassType, DeclaredClass, class, declare_class, msg_send, msg_send_id, mutability, sel};
    use objc2_app_kit::{
        NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem,
    };
    use objc2_foundation::{MainThreadMarker, NSString};
    use winit::event_loop::EventLoopProxy;

    use super::MenuAction;
    use crate::Wake;

    /// What [`install`] returns: the retained action target. AppKit references a
    /// menu item's target WEAKLY, so this must outlive the run loop — `App` holds
    /// it in a field for the process lifetime. Aliased so the `App` field type is
    /// the same name on every platform (`()` off macOS).
    pub type MenuHandle = Retained<MenuTarget>;

    declare_class!(
        /// The target object for every menu item. Owns the `EventLoopProxy<Wake>`
        /// and exposes one `menuAction:` selector: it reads the sending
        /// `NSMenuItem`'s `tag`, decodes a [`MenuAction`], and posts a
        /// [`Wake::MenuAction`] so the main loop dispatches it on `App` (off the
        /// AppKit menu-tracking call, on the next loop turn). No menu logic lives
        /// here — it is a pure relay from AppKit into the existing `Wake` channel.
        ///
        /// `pub(crate)` so the `MenuHandle` alias (held in an `App` field) and
        /// `install`'s return type are not "more private than the item" — the
        /// type itself is never named outside this module.
        pub(crate) struct MenuTarget;

        // SAFETY:
        // - NSObject imposes no subclassing requirements.
        // - InteriorMutable is the safe default; we never mutate the proxy.
        // - MenuTarget has no Drop impl beyond the auto-generated ivar drop.
        unsafe impl ClassType for MenuTarget {
            type Super = objc2::runtime::NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "ATermMenuTarget";
        }

        impl DeclaredClass for MenuTarget {
            type Ivars = EventLoopProxy<Wake>;
        }

        unsafe impl MenuTarget {
            /// `menuAction:` — the single selector wired to every item. `sender`
            /// is the clicked `NSMenuItem`; its `tag` decodes to the action. A tag
            /// that doesn't decode is ignored (no dispatch), so a stray/zero tag
            /// is inert rather than firing the wrong command.
            #[method(menuAction:)]
            fn menu_action(&self, sender: Option<&NSMenuItem>) {
                let Some(item) = sender else { return };
                // SAFETY: `item` is the live NSMenuItem AppKit passed as the
                // action sender; `tag` is a plain getter with no side effects.
                let tag = unsafe { item.tag() };
                if let Some(action) = MenuAction::from_tag(tag) {
                    // Fire-and-forget: a closed loop (app shutting down) just
                    // drops the event — mirrors every other `send_event` here.
                    let _ = self.ivars().send_event(Wake::MenuAction { action });
                }
            }
        }
    );

    impl MenuTarget {
        /// Allocate a target owning `proxy`. `mtm` proves we are on the main
        /// thread (AppKit requirement), which the winit loop guarantees.
        fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<Wake>) -> Retained<Self> {
            let this = mtm.alloc().set_ivars(proxy);
            // SAFETY: plain `[super init]` on a freshly allocated instance.
            unsafe { msg_send_id![super(this), init] }
        }
    }

    /// Build aterm's menu bar and install it as the shared application's main
    /// menu. Returns the retained action [`MenuTarget`] for the caller to keep
    /// alive (AppKit holds menu-item targets only weakly). Called from `resumed`
    /// after the window exists and only when NOT headless.
    ///
    /// Best-effort: if we are somehow off the main thread (`MainThreadMarker::new`
    /// is `None`) the menu is simply not installed — never a panic. The winit
    /// event loop always runs `resumed` on the main thread, so in practice the
    /// marker is always present.
    pub fn install(proxy: &EventLoopProxy<Wake>) -> Option<MenuHandle> {
        let mtm = MainThreadMarker::new()?;
        let app = NSApplication::sharedApplication(mtm);
        let target = MenuTarget::new(mtm, proxy.clone());

        let main = NSMenu::new(mtm);

        // --- App menu (titled with the app name by convention) ----------------
        let app_menu = NSMenu::new(mtm);
        add_item(mtm, &app_menu, &target, "About aterm", MenuAction::About, "", false);
        add_separator(mtm, &app_menu);
        add_item(mtm, &app_menu, &target, "Preferences…", MenuAction::Preferences, ",", true);
        add_separator(mtm, &app_menu);
        add_item(mtm, &app_menu, &target, "Hide aterm", MenuAction::Hide, "h", true);
        add_separator(mtm, &app_menu);
        add_item(mtm, &app_menu, &target, "Quit aterm", MenuAction::Quit, "q", true);
        attach_submenu(mtm, &main, "aterm", app_menu);

        // --- File -------------------------------------------------------------
        let file = NSMenu::new(mtm);
        add_item(mtm, &file, &target, "New Window", MenuAction::NewWindow, "n", true);
        add_item(mtm, &file, &target, "New Tab", MenuAction::NewTab, "t", true);
        // Cmd-Shift-N moves the active tab out into a new in-process window.
        add_item_mods(
            mtm,
            &file,
            &target,
            "Move Tab to New Window",
            MenuAction::MoveTabToNewWindow,
            "n",
            command_shift_mask(),
        );
        // Cmd-Shift-M moves the active tab into the NEXT existing window (wrapping).
        add_item_mods(
            mtm,
            &file,
            &target,
            "Move Tab to Next Window",
            MenuAction::MoveTabToNextWindow,
            "m",
            command_shift_mask(),
        );
        // Cmd-Shift-O opens the active session in a SECOND window (same live grid in
        // two windows — watch a log in one, type in another). The key MUST match
        // on_key's Cmd-Shift-O: AppKit's performKeyEquivalent intercepts a menu key
        // equivalent BEFORE the keyDown reaches on_key, so a "d" here would shadow
        // Cmd-Shift-D (SplitHorizontal) and make that primary chord keyboard-dead.
        add_item_mods(
            mtm,
            &file,
            &target,
            "Open Session in New Window",
            MenuAction::ViewSessionInNewWindow,
            "o",
            command_shift_mask(),
        );
        add_separator(mtm, &file);
        add_item(mtm, &file, &target, "Close Tab", MenuAction::CloseTab, "w", true);
        attach_submenu(mtm, &main, "File", file);

        // --- Edit -------------------------------------------------------------
        let edit = NSMenu::new(mtm);
        add_item(mtm, &edit, &target, "Copy", MenuAction::Copy, "c", true);
        add_item(mtm, &edit, &target, "Paste", MenuAction::Paste, "v", true);
        add_item(mtm, &edit, &target, "Select All", MenuAction::SelectAll, "a", true);
        add_separator(mtm, &edit);
        add_item(mtm, &edit, &target, "Find…", MenuAction::Find, "f", true);
        attach_submenu(mtm, &main, "Edit", edit);

        // --- View -------------------------------------------------------------
        let view = NSMenu::new(mtm);
        // Cmd-Ctrl-F is the macOS-standard Enter Full Screen equivalent.
        add_item_mods(
            mtm,
            &view,
            &target,
            "Enter Full Screen",
            MenuAction::ToggleFullScreen,
            "f",
            command_control_mask(),
        );
        attach_submenu(mtm, &main, "View", view);

        // --- Window -----------------------------------------------------------
        let window = NSMenu::new(mtm);
        add_item(mtm, &window, &target, "Minimize", MenuAction::Minimize, "m", true);
        add_item(mtm, &window, &target, "Zoom", MenuAction::Zoom, "", false);
        attach_submenu(mtm, &main, "Window", window);

        // --- Help -------------------------------------------------------------
        let help = NSMenu::new(mtm);
        add_item(mtm, &help, &target, "aterm Help", MenuAction::Help, "", false);
        attach_submenu(mtm, &main, "Help", help);

        app.setMainMenu(Some(&main));
        Some(target)
    }

    /// `Cmd` modifier mask (the default for a single-letter key equivalent).
    fn command_mask() -> NSEventModifierFlags {
        NSEventModifierFlags::NSEventModifierFlagCommand
    }

    /// `Cmd-Ctrl` mask (Enter Full Screen's standard equivalent).
    fn command_control_mask() -> NSEventModifierFlags {
        NSEventModifierFlags(
            NSEventModifierFlags::NSEventModifierFlagCommand.0
                | NSEventModifierFlags::NSEventModifierFlagControl.0,
        )
    }

    /// `Cmd-Shift` mask (Move Tab to New Window's ⇧⌘N equivalent).
    fn command_shift_mask() -> NSEventModifierFlags {
        NSEventModifierFlags(
            NSEventModifierFlags::NSEventModifierFlagCommand.0
                | NSEventModifierFlags::NSEventModifierFlagShift.0,
        )
    }

    /// Build one menu item wired to `menuAction:` on `target`, tagged with
    /// `action`, and append it to `menu`. `key` is the lowercase key-equivalent
    /// character ("" for none); `cmd` adds the ⌘ modifier (a Cmd shortcut). The
    /// equivalent is VISUAL only — it just renders next to the item; the actual
    /// keystroke is still handled by `App::on_key`.
    fn add_item(
        mtm: MainThreadMarker,
        menu: &NSMenu,
        target: &MenuTarget,
        title: &str,
        action: MenuAction,
        key: &str,
        cmd: bool,
    ) {
        let mods = if cmd { command_mask() } else { NSEventModifierFlags(0) };
        add_item_mods(mtm, menu, target, title, action, key, mods);
    }

    /// As [`add_item`] but with an explicit modifier mask (for non-⌘ equivalents
    /// like Enter Full Screen's ⌃⌘F).
    fn add_item_mods(
        mtm: MainThreadMarker,
        menu: &NSMenu,
        target: &MenuTarget,
        title: &str,
        action: MenuAction,
        key: &str,
        mods: NSEventModifierFlags,
    ) {
        let title = NSString::from_str(title);
        let key = NSString::from_str(key);
        // Build with the menuAction: selector so AppKit dispatches to `target`.
        let sel: Sel = sel!(menuAction:);
        // SAFETY: standard NSMenuItem construction + plain setters on a fresh
        // instance, all on the main thread (`mtm`). The selector exists on
        // MenuTarget (declared above). `setTarget`/`setTag`/`setKeyEquivalent*`
        // have no preconditions beyond a live receiver.
        unsafe {
            let item: Retained<NSMenuItem> = NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &title,
                Some(sel),
                &key,
            );
            // Deref-coerce MenuTarget -> NSObject -> AnyObject for the `id`
            // target argument (same pattern as accessibility.rs).
            let target_obj: &AnyObject = target;
            item.setTarget(Some(target_obj));
            item.setTag(action.tag());
            if !key.is_empty() {
                item.setKeyEquivalentModifierMask(mods);
            }
            menu.addItem(&item);
        }
    }

    /// Append a separator line to `menu`.
    fn add_separator(mtm: MainThreadMarker, menu: &NSMenu) {
        let sep = NSMenuItem::separatorItem(mtm);
        // `addItem` is a safe binding in objc2-app-kit; no `unsafe` needed.
        menu.addItem(&sep);
    }

    /// Attach `submenu` under a new top-level item titled `title` on `bar`. The
    /// item carries no action (its only job is to hold the submenu).
    fn attach_submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, submenu: Retained<NSMenu>) {
        let title = NSString::from_str(title);
        // SAFETY: standard top-level menu-item creation + setSubmenu, main thread.
        unsafe {
            let item: Retained<NSMenuItem> = NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &title,
                None,
                &NSString::from_str(""),
            );
            item.setSubmenu(Some(&submenu));
            bar.addItem(&item);
        }
    }

    /// Show the standard macOS About panel (App menu ▸ About aterm). It displays the
    /// bundle's app name + version (from Info.plist) and auto-loads the credits from
    /// `Contents/Resources/Credits.html` (written by `apps/aterm-mac/build-app.sh`).
    /// Best-effort: silently does nothing if somehow off the main thread.
    pub fn show_about_panel() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        // Override the panel's version line with live build provenance (version ·
        // commit · build-time · binary signature); the byline stays in the bundled
        // Credits.html. Equivalent to:
        //   [NSApp orderFrontStandardAboutPanelWithOptions:
        //       @{ @"ApplicationVersion": <about_line> }]
        let line = NSString::from_str(&crate::build_info::about_line());
        let key = NSString::from_str("ApplicationVersion");
        // SAFETY: `dictionaryWithObject:forKey:` and
        // `orderFrontStandardAboutPanelWithOptions:` are standard Foundation/AppKit
        // methods; both objects are valid and retained for the call.
        unsafe {
            let options: Retained<AnyObject> =
                msg_send_id![class!(NSDictionary), dictionaryWithObject: &*line, forKey: &*key];
            let _: () = msg_send![&app, orderFrontStandardAboutPanelWithOptions: &*options];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MenuAction;

    /// Every action's tag round-trips through `from_tag`, and the tags are
    /// distinct (so the integer carried in the NSMenuItem identifies exactly one
    /// command — no two items share a dispatch).
    #[test]
    fn tags_round_trip_and_are_unique() {
        let all = [
            MenuAction::About,
            MenuAction::Preferences,
            MenuAction::Hide,
            MenuAction::Quit,
            MenuAction::NewWindow,
            MenuAction::NewTab,
            MenuAction::MoveTabToNewWindow,
            MenuAction::MoveTabToNextWindow,
            MenuAction::ViewSessionInNewWindow,
            MenuAction::CloseTab,
            MenuAction::Copy,
            MenuAction::Paste,
            MenuAction::SelectAll,
            MenuAction::Find,
            MenuAction::ToggleFullScreen,
            MenuAction::Minimize,
            MenuAction::Zoom,
            MenuAction::Help,
        ];
        let mut seen = std::collections::HashSet::new();
        for a in all {
            assert!(a.tag() >= 1, "tag 0 is reserved for untagged items");
            assert!(seen.insert(a.tag()), "duplicate tag {} for {a:?}", a.tag());
            assert_eq!(MenuAction::from_tag(a.tag()), Some(a), "round-trip failed for {a:?}");
        }
    }

    /// The default `NSMenuItem` tag (0) and any unknown tag decode to `None`, so
    /// an untagged item never dispatches a real command.
    #[test]
    fn unknown_tag_is_none() {
        assert_eq!(MenuAction::from_tag(0), None);
        assert_eq!(MenuAction::from_tag(-1), None);
        assert_eq!(MenuAction::from_tag(9999), None);
    }
}
