// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! OSC desktop-notification handlers for the terminal.
//!
//! This module contains handlers for the three notification OSC families:
//! - OSC 9: simple notification (Terminal / ConEmu style)
//! - OSC 99: kitty desktop-notification protocol (title/body/urgency, multi-part)
//! - OSC 777: rxvt-unicode `notify` notification
//!
//! All three paths are gated by the host's notification authorization
//! ([`Terminal::authorize_notifications`][an] /
//! [`Terminal::is_notifications_authorized`][ina], mirrored into
//! `modes.allow_notifications`). When the host has not authorized
//! notifications (the post-#7918 default), the parser path cannot reach a
//! callback. Wiring a callback alone is insufficient — the host must also
//! authorize dispatch.
//!
//! [an]: super::Terminal::authorize_notifications
//! [ina]: super::Terminal::is_notifications_authorized

use super::handler::TerminalHandler;
use aterm_types::osc::{Notification, NotificationUrgency};

/// Maximum number of in-flight multi-part OSC 99 notifications retained in the
/// pending map. A flood of distinct `i=<id>` chunks must not grow the map
/// without bound; once the cap is hit, new ids are dropped.
const MAX_PENDING_NOTIFICATIONS: usize = 64;

/// Maximum byte length accepted for any single notification field (title or
/// body). Longer payloads are truncated at a UTF-8 boundary. Desktop
/// notification surfaces are small; an unbounded title is a memory/DoS vector.
const MAX_NOTIFICATION_FIELD_BYTES: usize = 4096;

impl TerminalHandler<'_> {
    /// Handle OSC 9 — simple desktop notification (Terminal / ConEmu style).
    ///
    /// Format: `ESC ] 9 ; message BEL` (or ST terminator).
    ///
    /// The `9;4;...` ConEmu *taskbar progress* sub-protocol is a distinct
    /// feature and is not a notification; it is intentionally not handled here.
    /// Everything else is treated as a notification body and forwarded to the
    /// simple notification callback registered via
    /// [`Terminal::set_notification_callback`][cb].
    ///
    /// Gated by host notification authorization (see module docs). When
    /// unauthorized, or when no callback is wired, this is a silent no-op.
    ///
    /// [cb]: super::Terminal::set_notification_callback
    pub(super) fn handle_osc_9(&mut self, params: &[&[u8]]) {
        if !self.modes.allow_notifications {
            return;
        }
        // params[0] = "9" (already parsed). The message is params[1..], which
        // the OSC parser splits on ';' — rejoin so a message with literal
        // semicolons round-trips.
        let Some(message) = join_params(params, 1) else {
            return;
        };
        // ConEmu taskbar progress (`9;4;...`) is not a notification; ignore it
        // here (it has no notification callback semantics).
        if message.starts_with("4;") || message == "4" {
            return;
        }
        let message = sanitize_notification(&message);
        if message.is_empty() {
            return;
        }
        if let Some(ref mut callback) = self.notifications.callback {
            callback(&message);
        }
    }

    /// Handle OSC 99 — kitty desktop-notification protocol.
    ///
    /// Format: `ESC ] 99 ; <metadata> ; <payload> ST` where metadata is a
    /// colon-separated list of `key=value` pairs:
    /// - `i=<id>`   notification id (groups multi-part notifications)
    /// - `p=<what>` payload type: `title` (default) or `body`
    /// - `u=<n>`    urgency: 0=low, 1=normal (default), 2=critical
    /// - `d=<0|1>`  done flag: `d=0` = more chunks follow, `d=1`/absent = final
    ///
    /// Multi-part notifications sharing an `i=<id>` are accumulated until the
    /// final chunk (`d=1` or no `d`), then dispatched to the advanced
    /// notification callback registered via
    /// [`Terminal::set_advanced_notification_callback`][cb].
    ///
    /// Gated by host notification authorization (see module docs).
    ///
    /// [cb]: super::Terminal::set_advanced_notification_callback
    pub(super) fn handle_osc_99(&mut self, params: &[&[u8]]) {
        if !self.modes.allow_notifications {
            return;
        }
        // params[1] = metadata (key=value pairs), params[2..] = payload.
        let metadata = params
            .get(1)
            .and_then(|p| std::str::from_utf8(p).ok())
            .unwrap_or("");
        let payload = join_params(params, 2).unwrap_or_default();

        let mut id: Option<String> = None;
        let mut payload_kind = Osc99Payload::Title;
        let mut urgency = NotificationUrgency::Normal;
        let mut done = true; // absent `d` means final.

        for pair in metadata.split(':') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "i" => {
                    if !value.is_empty() && value.len() <= 256 {
                        id = Some(value.to_string());
                    }
                }
                "p" => {
                    payload_kind = match value {
                        "body" => Osc99Payload::Body,
                        _ => Osc99Payload::Title,
                    };
                }
                "u" => urgency = NotificationUrgency::from_param(value),
                "d" => done = value != "0",
                _ => {} // Unknown / unsupported key — ignore.
            }
        }

        let text = sanitize_notification(&payload);

        // Build/update the accumulator entry for this notification.
        let key = id.clone().unwrap_or_else(|| {
            // Anonymous notifications get a unique synthetic key so concurrent
            // anonymous notifications don't clobber each other.
            let n = self.notifications.anon_counter;
            self.notifications.anon_counter = n.wrapping_add(1);
            format!("\u{0}anon-{n}")
        });

        let entry = if let Some(existing) = self.notifications.pending.remove(&key) {
            existing
        } else if self.notifications.pending.len() >= MAX_PENDING_NOTIFICATIONS {
            // Pending map is full — drop rather than grow unbounded.
            return;
        } else {
            Notification {
                id: id.clone(),
                ..Notification::default()
            }
        };

        let mut entry = entry;
        entry.urgency = urgency;
        match payload_kind {
            Osc99Payload::Title => {
                if !text.is_empty() {
                    entry.title = Some(text);
                }
            }
            Osc99Payload::Body => {
                if !text.is_empty() {
                    entry.body = Some(text);
                }
            }
        }

        if done {
            if entry.has_content() {
                if let Some(ref mut callback) = self.notifications.advanced_callback {
                    callback(entry);
                }
            }
        } else {
            // More chunks expected — re-insert for accumulation.
            self.notifications.pending.insert(key, entry);
        }
    }

    /// Handle OSC 777 — rxvt-unicode `notify` notification.
    ///
    /// Format: `ESC ] 777 ; notify ; <title> ; <body> ST`. Only the `notify`
    /// sub-command is a desktop notification; other OSC 777 sub-commands are
    /// ignored. Dispatched to the advanced notification callback registered via
    /// [`Terminal::set_advanced_notification_callback`][cb].
    ///
    /// Gated by host notification authorization (see module docs).
    ///
    /// [cb]: super::Terminal::set_advanced_notification_callback
    pub(super) fn handle_osc_777(&mut self, params: &[&[u8]]) {
        if !self.modes.allow_notifications {
            return;
        }
        // params[1] = sub-command. Only "notify" is a notification.
        let subcmd = params.get(1).and_then(|p| std::str::from_utf8(p).ok());
        if subcmd != Some("notify") {
            return;
        }
        let title = params
            .get(2)
            .and_then(|p| std::str::from_utf8(p).ok())
            .map(sanitize_notification)
            .filter(|s| !s.is_empty());
        // The body may contain literal semicolons; rejoin params[3..].
        let body = join_params(params, 3)
            .map(|s| sanitize_notification(&s))
            .filter(|s| !s.is_empty());

        if title.is_none() && body.is_none() {
            return;
        }
        let notification = Notification {
            id: None,
            title,
            body,
            urgency: NotificationUrgency::Normal,
        };
        if let Some(ref mut callback) = self.notifications.advanced_callback {
            callback(notification);
        }
    }
}

/// OSC 99 payload kind (the `p=` metadata key).
enum Osc99Payload {
    Title,
    Body,
}

/// Rejoin OSC params from `start..` with `;`, returning `None` if there is no
/// param at `start`. The VTE OSC parser splits on `;`, so a payload containing
/// literal semicolons arrives as multiple params; this reconstructs it.
fn join_params(params: &[&[u8]], start: usize) -> Option<String> {
    if params.len() <= start {
        return None;
    }
    let mut bytes = params[start].to_vec();
    for extra in &params[start + 1..] {
        bytes.push(b';');
        bytes.extend_from_slice(extra);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Strip control characters and BiDi-override codepoints from a notification
/// field, then truncate to [`MAX_NOTIFICATION_FIELD_BYTES`] at a UTF-8
/// boundary.
///
/// Mirrors the title sanitizer in `handler_osc.rs`: notification surfaces (like
/// title bars) render outside the grid's BiDi-security filter, so the strip is
/// unconditional. Removes:
/// - C0 controls (0x00-0x1F) except tab (0x09)
/// - C1 controls (0x80-0x9F)
/// - BiDi directional overrides (U+202A-U+202E, U+2066-U+2069), CVE-2021-42574
fn sanitize_notification(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|&c| {
            if c == '\t' {
                return true;
            }
            let code = c as u32;
            if code <= 0x1F || (0x80..=0x9F).contains(&code) {
                return false;
            }
            !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    if filtered.len() <= MAX_NOTIFICATION_FIELD_BYTES {
        filtered
    } else {
        let end = filtered.floor_char_boundary(MAX_NOTIFICATION_FIELD_BYTES);
        filtered[..end].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Terminal;
    use std::sync::{Arc, Mutex};

    #[test]
    fn osc_9_fires_notification_callback_when_authorized() {
        let mut term = Terminal::new(24, 80);
        let captured = Arc::new(Mutex::new(None::<String>));
        let captured_clone = Arc::clone(&captured);
        term.set_notification_callback(move |msg| {
            *captured_clone.lock().expect("poisoned") = Some(msg.to_string());
        });
        term.authorize_notifications();

        term.process(b"\x1b]9;Build finished\x07");

        assert_eq!(
            captured.lock().expect("poisoned").as_deref(),
            Some("Build finished"),
            "OSC 9 must fire the notification callback after authorization"
        );
    }

    #[test]
    fn osc_9_is_noop_when_not_authorized() {
        let mut term = Terminal::new(24, 80);
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = Arc::clone(&fired);
        term.set_notification_callback(move |_| {
            *fired_clone.lock().expect("poisoned") = true;
        });
        // No authorize_notifications() call: default posture is denied.

        term.process(b"\x1b]9;Should not fire\x07");

        assert!(
            !*fired.lock().expect("poisoned"),
            "OSC 9 must not fire the callback without host authorization"
        );
    }

    #[test]
    fn osc_9_strips_bidi_override() {
        let mut term = Terminal::new(24, 80);
        let captured = Arc::new(Mutex::new(None::<String>));
        let captured_clone = Arc::clone(&captured);
        term.set_notification_callback(move |msg| {
            *captured_clone.lock().expect("poisoned") = Some(msg.to_string());
        });
        term.authorize_notifications();

        term.process("\x1b]9;safe\u{202E}evil\x07".as_bytes());

        assert_eq!(
            captured.lock().expect("poisoned").as_deref(),
            Some("safeevil"),
            "U+202E must be stripped from OSC 9 notification text"
        );
    }

    #[test]
    fn osc_9_ignores_conemu_taskbar_progress() {
        let mut term = Terminal::new(24, 80);
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = Arc::clone(&fired);
        term.set_notification_callback(move |_| {
            *fired_clone.lock().expect("poisoned") = true;
        });
        term.authorize_notifications();

        // ConEmu taskbar progress, not a notification.
        term.process(b"\x1b]9;4;1;50\x07");

        assert!(
            !*fired.lock().expect("poisoned"),
            "OSC 9;4 taskbar progress must not be treated as a notification"
        );
    }

    #[test]
    fn osc_99_single_message_fires_advanced_callback() {
        let mut term = Terminal::new(24, 80);
        let captured = Arc::new(Mutex::new(None::<Notification>));
        let captured_clone = Arc::clone(&captured);
        term.set_advanced_notification_callback(move |n| {
            *captured_clone.lock().expect("poisoned") = Some(n);
        });
        term.authorize_notifications();

        term.process(b"\x1b]99;u=2:p=title;Hello\x07");

        let got = captured.lock().expect("poisoned").clone();
        let got = got.expect("OSC 99 must fire the advanced callback");
        assert_eq!(got.title.as_deref(), Some("Hello"));
        assert_eq!(got.urgency, NotificationUrgency::Critical);
    }

    #[test]
    fn osc_99_multipart_accumulates_title_and_body() {
        let mut term = Terminal::new(24, 80);
        let captured = Arc::new(Mutex::new(None::<Notification>));
        let captured_clone = Arc::clone(&captured);
        term.set_advanced_notification_callback(move |n| {
            *captured_clone.lock().expect("poisoned") = Some(n);
        });
        term.authorize_notifications();

        // First chunk (title, more to come), then final chunk (body).
        term.process(b"\x1b]99;i=42:p=title:d=0;My Title\x07");
        assert!(
            captured.lock().expect("poisoned").is_none(),
            "non-final OSC 99 chunk must not dispatch yet"
        );
        term.process(b"\x1b]99;i=42:p=body:d=1;My Body\x07");

        let got = captured.lock().expect("poisoned").clone().expect("dispatched");
        assert_eq!(got.id.as_deref(), Some("42"));
        assert_eq!(got.title.as_deref(), Some("My Title"));
        assert_eq!(got.body.as_deref(), Some("My Body"));
    }

    #[test]
    fn osc_777_notify_fires_advanced_callback() {
        let mut term = Terminal::new(24, 80);
        let captured = Arc::new(Mutex::new(None::<Notification>));
        let captured_clone = Arc::clone(&captured);
        term.set_advanced_notification_callback(move |n| {
            *captured_clone.lock().expect("poisoned") = Some(n);
        });
        term.authorize_notifications();

        term.process(b"\x1b]777;notify;Title Here;Body Here\x07");

        let got = captured.lock().expect("poisoned").clone().expect("dispatched");
        assert_eq!(got.title.as_deref(), Some("Title Here"));
        assert_eq!(got.body.as_deref(), Some("Body Here"));
    }

    #[test]
    fn osc_777_non_notify_subcmd_ignored() {
        let mut term = Terminal::new(24, 80);
        let fired = Arc::new(Mutex::new(false));
        let fired_clone = Arc::clone(&fired);
        term.set_advanced_notification_callback(move |_| {
            *fired_clone.lock().expect("poisoned") = true;
        });
        term.authorize_notifications();

        term.process(b"\x1b]777;something;Title;Body\x07");

        assert!(
            !*fired.lock().expect("poisoned"),
            "non-notify OSC 777 sub-command must be ignored"
        );
    }
}
