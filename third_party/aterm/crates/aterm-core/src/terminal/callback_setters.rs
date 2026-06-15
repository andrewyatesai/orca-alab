// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Callback registration API for Terminal.
//!
//! Contains all `set_*_callback` methods, `resize()`, and related state queries.
//! Extracted from mod.rs to reduce file size.

use super::{
    ClipboardOperation, CopyToClipboardOperation, Terminal, WindowOperation, WindowResponse, types,
};

impl Terminal {
    /// Resize the terminal.
    ///
    /// The active grid is resized with reflow appropriate to its type:
    /// - Primary screen: reflow enabled (soft-wrapped lines unwrap/rewrap)
    /// - Alt screen: reflow disabled (app manages layout, redraws after SIGWINCH)
    ///
    /// The inactive grid (saved in `alt_grid`) uses the opposite reflow mode.
    /// This matches xterm, Alacritty, kitty, and Terminal behavior (#4164).
    ///
    /// Dimensions are clamped by the grid to
    /// `1..=`[`MAX_GRID_ROWS`](crate::grid::MAX_GRID_ROWS)`/`[`MAX_GRID_COLS`](crate::grid::MAX_GRID_COLS)
    /// (§5.8 ingress bound), so a hostile resize cannot request an
    /// arbitrarily large cell allocation.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.modes.alternate_screen {
            // Alt screen active: don't reflow current grid (app-managed content).
            // Saved primary grid should reflow normally.
            self.grid.resize_no_reflow(rows, cols);
            if let Some(ref mut saved_primary) = self.alt_grid {
                saved_primary.resize(rows, cols);
            }
        } else {
            // Primary screen active: reflow current grid.
            // Alt grid (if present) should not be reflowed.
            self.grid.resize(rows, cols);
            if let Some(ref mut alt) = self.alt_grid {
                alt.resize_no_reflow(rows, cols);
            }
        }
        // Reflow invalidates all row/col coordinates (#4056).
        self.text_selection.clear();
    }

    /// Set bell callback.
    pub fn set_bell_callback<F: FnMut() + Send + 'static>(&mut self, callback: F) {
        self.bell_callback = Some(Box::new(callback));
    }

    /// Clear bell callback.
    #[allow(dead_code, reason = "cleared via the FFI app-callback layer (ffi_bridge/)")]
    pub(crate) fn clear_bell_callback(&mut self) {
        self.bell_callback = None;
    }

    /// Set cursor style change callback (DECSCUSR).
    ///
    /// The callback is invoked when a DECSCUSR sequence changes the cursor style.
    /// The UI layer should use this to start/stop cursor blink timers and update
    /// cursor rendering.
    pub fn set_cursor_style_callback<F: FnMut(aterm_types::CursorStyle) + Send + 'static>(
        &mut self,
        callback: F,
    ) {
        self.cursor_style_callback = Some(Box::new(callback));
    }

    /// Set buffer activation callback.
    ///
    /// The callback is invoked when the terminal switches between the main and
    /// alternate screen buffers. The boolean parameter is `true` when switching
    /// to the alternate screen, `false` when switching back to the main screen.
    ///
    /// This is useful for SwiftTerm integration where `bufferActivated` callback
    /// needs to be notified of buffer switches (e.g., when vim/less starts).
    pub fn set_buffer_activation_callback<F: FnMut(bool) + Send + 'static>(&mut self, callback: F) {
        self.buffer_activation_callback = Some(Box::new(callback));
    }

    /// Clear buffer activation callback.
    #[allow(dead_code, reason = "cleared via the FFI app-callback layer (ffi_bridge/)")]
    pub(crate) fn clear_buffer_activation_callback(&mut self) {
        self.buffer_activation_callback = None;
    }

    /// Set title change callback.
    pub fn set_title_callback<F: FnMut(&str) + Send + 'static>(&mut self, callback: F) {
        self.title.callback = Some(Box::new(callback));
    }

    /// Clear title change callback.
    #[allow(dead_code, reason = "cleared via the FFI app-callback layer (ffi_bridge/)")]
    pub(crate) fn clear_title_callback(&mut self) {
        self.title.callback = None;
    }

    /// Set title event callback with type discriminator (v3).
    ///
    /// The callback receives the title type (WindowAndIcon, IconOnly, WindowOnly)
    /// and the title text for all OSC 0/1/2 title changes.
    pub fn set_title_event_callback<F: FnMut(aterm_types::TitleType, &str) + Send + 'static>(
        &mut self,
        callback: F,
    ) {
        self.title.event_callback = Some(Box::new(callback));
    }

    /// Clear title event callback.
    #[allow(dead_code, reason = "cleared via the FFI app-callback layer (ffi_bridge/)")]
    pub(crate) fn clear_title_event_callback(&mut self) {
        self.title.event_callback = None;
    }

    /// Set desktop notification callback (OSC 9).
    ///
    /// The callback is invoked when an application sends a notification escape
    /// sequence (OSC 9 without a subcommand). The UI layer should display a
    /// system notification with the provided message.
    ///
    /// # Example
    ///
    /// ```text
    /// terminal.set_notification_callback(|message| {
    ///     // Display system notification with the message
    ///     show_notification("Terminal", message);
    /// });
    /// ```
    ///
    /// The callback receives the notification message as a `&str`.
    ///
    /// # Supported Sequences
    ///
    /// - `ESC ] 9 ; message BEL` - Simple notification (Terminal/ConEmu style)
    /// - `ESC ] 9 ; message ST`  - ST terminator variant
    pub fn set_notification_callback<F: FnMut(&str) + Send + 'static>(&mut self, callback: F) {
        self.notifications.callback = Some(Box::new(callback));
    }

    /// Clear desktop notification callback.
    #[allow(dead_code, reason = "cleared via the FFI app-callback layer (ffi_bridge/)")]
    pub(crate) fn clear_notification_callback(&mut self) {
        self.notifications.callback = None;
    }

    /// Set a callback for dynamic color changes (OSC 10/11/12, OSC 110/111/112).
    ///
    /// The callback is invoked when the terminal's default foreground, background,
    /// or cursor color changes via escape sequences.
    ///
    /// # Arguments
    ///
    /// The callback receives:
    /// - `target: ColorTarget` — which color changed (foreground, background, cursor, etc.)
    /// - `color: Rgb` — the new color value
    /// - `op: ColorChangeOp` — whether the color was set or reset
    ///
    /// # Example
    ///
    /// ```text
    /// use aterm_core::terminal::ColorTarget;
    ///
    /// terminal.set_color_change_callback(|target, color, op| {
    ///     match target {
    ///         ColorTarget::Foreground => update_fg(color),
    ///         ColorTarget::Background => update_bg(color),
    ///         ColorTarget::Cursor => update_cursor(color),
    ///         _ => {}
    ///     }
    /// });
    /// ```
    pub fn set_color_change_callback<F>(&mut self, callback: F)
    where
        F: FnMut(super::ColorTarget, aterm_types::Rgb, super::ColorChangeOp) + Send + 'static,
    {
        self.color.change_callback = Some(Box::new(callback));
    }

    /// Clear the color change callback.
    pub fn clear_color_change_callback(&mut self) {
        self.color.change_callback = None;
    }

    /// Set a callback for dynamic color queries (OSC 10/11/12 with `?`).
    ///
    /// When an application queries the terminal's foreground, background, or
    /// cursor color, this callback is consulted first. If it returns
    /// `Some(Rgb)`, that color is used in the escape sequence response
    /// instead of the terminal's internal palette value. Returning `None`
    /// falls back to the palette color.
    ///
    /// This is useful when the host UI renders different colors than the
    /// terminal palette stores (e.g., theme overrides, system appearance).
    ///
    /// # Arguments
    ///
    /// The callback receives a [`ColorTarget`](super::ColorTarget):
    /// - `Foreground` — OSC 10 `?`
    /// - `Background` — OSC 11 `?`
    /// - `Cursor` — OSC 12 `?`
    pub fn set_color_query_callback<F>(&mut self, callback: F)
    where
        F: FnMut(super::ColorTarget) -> Option<aterm_types::Rgb> + Send + 'static,
    {
        self.color.query_callback = Some(Box::new(callback));
    }

    /// Clear the color query callback.
    pub fn clear_color_query_callback(&mut self) {
        self.color.query_callback = None;
    }

    /// Set advanced desktop notification callback (OSC 99/777).
    ///
    /// The callback is invoked when a complete notification is received via OSC 99
    /// (kitty protocol) or OSC 777 (rxvt-unicode protocol).
    ///
    /// The kitty notification protocol supports:
    /// - Separate title and body
    /// - Urgency levels (low, normal, critical)
    /// - Notification IDs for updates
    ///
    /// OSC 777 format: `ESC ] 777 ; notify ; title ; body ST`
    ///
    /// # Example
    ///
    /// ```text
    /// use aterm_core::terminal::{Notification, NotificationUrgency};
    ///
    /// terminal.set_advanced_notification_callback(|notification| {
    ///     let title = notification.title.as_deref().unwrap_or("Terminal");
    ///     let body = notification.body.as_deref().unwrap_or("");
    ///     let urgent = matches!(notification.urgency, NotificationUrgency::Critical);
    ///     show_system_notification(title, body, urgent);
    /// });
    /// ```
    ///
    /// # Supported Sequences
    ///
    /// - `ESC ] 99 ; i=ID:p=title:d=0 ST` + `ESC ] 99 ; i=ID:p=body:d=1 ST`
    /// - `ESC ] 99 ; p=body:u=2:d=1 ST` - Single message with critical urgency
    /// - `ESC ] 777 ; notify ; title ; body ST` - rxvt-unicode style notification
    ///
    /// See <https://sw.kovidgoyal.net/kitty/desktop-notifications/> for protocol details.
    pub fn set_advanced_notification_callback<F: FnMut(types::Notification) + Send + 'static>(
        &mut self,
        callback: F,
    ) {
        self.notifications.advanced_callback = Some(Box::new(callback));
    }

    /// Clear advanced notification callback (OSC 99/777).
    pub fn clear_advanced_notification_callback(&mut self) {
        self.notifications.advanced_callback = None;
    }

    /// Set clipboard callback for OSC 52 operations.
    ///
    /// The callback is invoked when an application sends OSC 52 to set or clear
    /// the clipboard, and (optionally) when querying clipboard contents.
    ///
    /// Clipboard queries (Pd = "?") are ignored by default for security.
    ///
    /// To enable queries:
    /// - Rust API: set `TerminalConfig::allow_osc52_query = true` via
    ///   [`apply_config`](Self::apply_config).
    /// - Direct toggle: [`set_osc52_query_allowed`](Self::set_osc52_query_allowed).
    ///
    /// The callback receives a [`ClipboardOperation`] and should:
    /// - For `Set` operations: copy the content to the appropriate clipboard(s)
    /// - For `Query` operations: return the clipboard content (or None if denied)
    /// - For `Clear` operations: clear the clipboard content
    ///
    /// # Example
    ///
    /// ```text
    /// use aterm_core::terminal::{ClipboardOperation, Terminal};
    ///
    /// let mut terminal = Terminal::new(24, 80);
    /// terminal.set_clipboard_callback(|op| {
    ///     match op {
    ///         ClipboardOperation::Set { content, .. } => {
    ///             // Copy to system clipboard (platform-specific)
    ///             println!("Set clipboard: {}", content);
    ///             None
    ///         }
    ///         ClipboardOperation::Query { .. } => {
    ///             // Return clipboard content (or None to deny)
    ///             Some("clipboard content".to_string())
    ///         }
    ///         ClipboardOperation::Clear { .. } => {
    ///             // Clear clipboard
    ///             None
    ///         }
    ///     }
    /// });
    /// ```
    pub fn set_clipboard_callback<F>(&mut self, callback: F)
    where
        F: FnMut(ClipboardOperation) -> Option<String> + Send + 'static,
    {
        self.clipboard.callback = Some(Box::new(callback));
    }

    /// Set a callback for OSC 1337 named pasteboard operations.
    ///
    /// This callback handles Terminal-style clipboard operations:
    /// - `CopyToClipboard=name` + `EndCopy`: Text capture to named pasteboard
    /// - `Copy=base64`: Direct copy of base64-decoded data
    ///
    /// Named pasteboards (on macOS) include "general", "rule", "find", "font".
    /// An empty pasteboard name typically means the general (system) clipboard.
    ///
    /// # Example
    ///
    /// ```text
    /// use aterm_core::terminal::{Terminal, CopyToClipboardOperation};
    ///
    /// let mut term = Terminal::new(24, 80);
    /// term.set_copy_to_clipboard_callback(|op| {
    ///     match op {
    ///         CopyToClipboardOperation::CaptureComplete { pasteboard, content } => {
    ///             println!("Copy to pasteboard '{}': {}", pasteboard, content);
    ///         }
    ///         CopyToClipboardOperation::DirectCopy { content } => {
    ///             println!("Direct copy: {}", content);
    ///         }
    ///     }
    /// });
    /// ```
    pub fn set_copy_to_clipboard_callback<F>(&mut self, callback: F)
    where
        F: FnMut(CopyToClipboardOperation) + Send + 'static,
    {
        self.clipboard.copy_callback = Some(Box::new(callback));
    }

    /// Check if a CopyToClipboard capture is currently active.
    ///
    /// Returns `true` if OSC 1337 CopyToClipboard was received but EndCopy has
    /// not yet been processed.
    #[must_use]
    pub fn is_copy_to_clipboard_active(&self) -> bool {
        self.clipboard.copy_state.is_some()
    }

    /// Set a callback for DCS payloads.
    ///
    /// The callback receives the raw DCS data bytes (payload only) and the final byte.
    /// Payload data is capped to a fixed size to avoid unbounded buffering.
    pub fn set_dcs_callback<F>(&mut self, callback: F)
    where
        F: FnMut(&[u8], u8) + Send + 'static,
    {
        self.dcs.callback = Some(Box::new(callback));
    }

    /// Clear the DCS callback.
    pub fn clear_dcs_callback(&mut self) {
        self.dcs.callback = None;
    }

    /// Set a callback for window operations (CSI t - XTWINOPS).
    ///
    /// The callback is invoked when window manipulation or query sequences are received.
    /// For manipulation operations (iconify, move, resize), perform the operation and
    /// return `None`. For query operations (report state, position, size), return the
    /// appropriate `WindowResponse`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use aterm_core::terminal::{Terminal, WindowOperation, WindowResponse};
    ///
    /// let mut terminal = Terminal::new(24, 80);
    /// terminal.set_window_callback(|op| {
    ///     match op {
    ///         WindowOperation::ReportTextAreaSizeCells => {
    ///             Some(WindowResponse::SizeCells { rows: 24, cols: 80 })
    ///         }
    ///         WindowOperation::Iconify => {
    ///             // Minimize window (platform-specific)
    ///             None
    ///         }
    ///         _ => None,
    ///     }
    /// });
    /// ```
    pub fn set_window_callback<F>(&mut self, callback: F)
    where
        F: FnMut(WindowOperation) -> Option<WindowResponse> + Send + 'static,
    {
        self.window_callback = Some(Box::new(callback));
    }

    /// Clear window callback.
    #[allow(dead_code, reason = "cleared via the FFI app-callback layer (ffi_bridge/)")]
    pub(crate) fn clear_window_callback(&mut self) {
        self.window_callback = None;
    }

    /// Get the current remote host (OSC 1337 RemoteHost).
    ///
    /// Returns `Some(RemoteHost)` if in an SSH session (as reported by the shell
    /// via OSC 1337 RemoteHost=user@host), or `None` if in a local session.
    ///
    /// # Example
    ///
    /// ```text
    /// use aterm_core::terminal::Terminal;
    ///
    /// let mut term = Terminal::new(24, 80);
    /// term.process(b"\x1b]1337;RemoteHost=alice@server.example.com\x07");
    /// if let Some(host) = term.remote_host() {
    ///     println!("Connected to {}@{}", host.user, host.hostname);
    /// }
    /// ```
    #[must_use]
    pub fn remote_host(&self) -> Option<&types::RemoteHost> {
        self.iterm2.remote_host.as_ref()
    }

    /// Set a callback for remote host change events.
    ///
    /// Called when OSC 1337 RemoteHost changes the current host (connect or
    /// disconnect). The callback receives `None` when returning to local session.
    ///
    /// # Example
    ///
    /// ```
    /// use aterm_core::terminal::Terminal;
    ///
    /// let mut term = Terminal::new(24, 80);
    /// term.set_remote_host_callback(|host| {
    ///     match host {
    ///         Some(h) => println!("SSH to {}@{}", h.user, h.hostname),
    ///         None => println!("Back to local session"),
    ///     }
    /// });
    /// ```
    #[cfg(test)]
    pub fn set_remote_host_callback<F>(&mut self, callback: F)
    where
        F: FnMut(Option<&types::RemoteHost>) + Send + 'static,
    {
        self.iterm2.remote_host_callback = Some(Box::new(callback));
    }

    /// Set a callback for text sizing events (OSC 66 - Kitty protocol).
    ///
    /// Called when text sizing escape sequences are received. The operation
    /// includes scale, width, alignment parameters, and the text content.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use aterm_core::terminal::Terminal;
    /// use aterm_core::testing::set_text_sizing_callback;
    ///
    /// let mut term = Terminal::new(24, 80);
    /// set_text_sizing_callback(&mut term, |op| {
    ///     println!("Text: {}, scale: {:?}", op.text, op.scale);
    /// });
    /// ```
    #[cfg(test)]
    pub(crate) fn set_text_sizing_callback<F>(&mut self, callback: F)
    where
        F: FnMut(types::TextSizingOperation) + Send + 'static,
    {
        self.text_sizing_callback = Some(Box::new(callback));
    }
}
