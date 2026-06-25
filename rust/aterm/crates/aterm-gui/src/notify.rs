// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Native desktop notification DELIVERY for OSC 9 / OSC 99 / OSC 777.
//!
//! The engine (`aterm-core`) already PARSES these escapes and fires
//! `set_notification_callback` (OSC 9 / 777, a plain body string) and
//! `set_advanced_notification_callback` (OSC 99 / kitty, a structured
//! [`Notification`] with title + body), gated by the host's
//! `authorize_notifications`. This module is the GUI side that turns those
//! callbacks into real macOS notifications, with two hard rules borrowed from
//! the OSC 52 clipboard path:
//!
//! 1. **Never block the engine.** The callbacks fire on a tab's PTY reader
//!    thread, under the `Terminal` lock. They MUST NOT spawn a subprocess
//!    there. Instead each callback does a single lock-free `mpsc::Sender::send`
//!    of a [`NotifyMsg`]; a dedicated delivery thread ([`spawn_delivery`]) owns
//!    the receiver and runs the (blocking) notifier subprocess off the hot path.
//!
//! 2. **Untrusted content.** A notification body is program output — over SSH it
//!    is fully attacker-controlled. The osascript fallback builds an AppleScript
//!    string literal, so the body is run through [`applescript_escape`] (and
//!    every argument is passed as a real argv entry, never a shell string) to
//!    foreclose AppleScript / shell injection. `terminal-notifier` takes argv
//!    directly, so no escaping is needed on that path.
//!
//! **Focus-aware suppression.** The delivery thread reads a shared SUPPRESSION SET
//! the main (UI) thread keeps current: the active-tab focused-pane session id of
//! EVERY focused window. A notification is suppressed ONLY when its originating
//! session is in that set (the user is already looking at it in some focused
//! window). App unfocused (empty set), OR a background tab fired it → delivered.
//! Carrying the SET (not a single `focused` bool + one `active` id) makes this
//! per-window-correct: with two windows, a focused non-front window's active tab
//! suppresses correctly, and the front window's active tab does NOT suppress when
//! the front window is unfocused. At one window the set is `{active}` (focused) or
//! `{}` (unfocused) — byte-identical to the old two-atomic behavior. This matches
//! the iTerm2 / Terminal.app default and keeps background-tab activity visible.

// macOS-only delivery: on Linux this module is a channel-draining stub
// (`spawn_delivery`), so the real-notification helpers/fields are intentionally
// unused there.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

/// One pending notification handed from a tab's engine callback to the delivery
/// thread. `session` is the originating tab's id (for focus-aware suppression).
pub struct NotifyMsg {
    /// Originating session/tab id (matched against the active tab to suppress
    /// self-notifications the user is already watching).
    pub session: u64,
    /// Notification title (OSC 99 carries one; OSC 9/777 do not — `None`).
    pub title: Option<String>,
    /// Notification body.
    pub body: String,
}

/// Spawn the single, process-wide notification delivery thread and return the
/// `Sender` each tab clones into its engine callbacks. The thread parks on
/// `recv()` (0% idle when no notifications arrive) and exits when every sender
/// is dropped. `suppress` is the live suppression set the UI thread keeps current
/// (the active-tab focused-pane id of every focused window); the thread reads it
/// to apply focus-aware suppression.
#[cfg(target_os = "macos")]
pub fn spawn_delivery(suppress: Arc<Mutex<HashSet<u64>>>) -> Sender<NotifyMsg> {
    let (tx, rx) = std::sync::mpsc::channel::<NotifyMsg>();
    std::thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            // Suppress ONLY when the firing session is the active tab of SOME
            // focused window — the user is already looking at it. App unfocused
            // (empty set) OR a background tab fired it → deliver (a background
            // tab's activity still surfaces, mirroring `App::on_bell`).
            if suppress
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains(&msg.session)
            {
                continue;
            }
            deliver(msg.title.as_deref(), &msg.body);
        }
    });
    tx
}

/// Non-macOS stub: drain the channel so senders never block and the workspace
/// builds everywhere. There is no portable native notifier wired up off macOS.
#[cfg(not(target_os = "macos"))]
pub fn spawn_delivery(_suppress: Arc<Mutex<HashSet<u64>>>) -> Sender<NotifyMsg> {
    let (tx, rx) = std::sync::mpsc::channel::<NotifyMsg>();
    std::thread::spawn(move || while rx.recv().is_ok() {});
    tx
}

/// Deliver one notification natively. Prefers `terminal-notifier` (click-to-
/// activate, app sender) when installed; otherwise falls back to `osascript`'s
/// `display notification`. Runs ONLY on the delivery thread (blocking is fine
/// there). Best-effort: a missing notifier or a non-zero exit is swallowed.
#[cfg(target_os = "macos")]
pub fn deliver(title: Option<&str>, body: &str) {
    use std::process::{Command, Stdio};

    let title = title.unwrap_or("aterm");

    // Preferred path: terminal-notifier. Arguments are real argv entries, so the
    // (untrusted) title/body cannot inject. `.status()` both waits (reaping the
    // child) and tells us whether the binary exists — `Err` means not installed,
    // so we fall through to osascript. A non-zero exit means it IS installed but
    // failed; we do NOT double-notify via osascript in that case.
    let tn = Command::new("terminal-notifier")
        .arg("-title")
        .arg(title)
        .arg("-message")
        .arg(body)
        .arg("-sender")
        .arg("com.aterm.aterm")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if tn.is_ok() {
        return;
    }

    // Fallback: osascript. The body + title go into an AppleScript string
    // literal, so both are escaped. The script itself is passed as a single
    // `-e` argv entry (never via a shell), closing the shell-injection vector;
    // `applescript_escape` closes the AppleScript-injection vector.
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        applescript_escape(body),
        applescript_escape(title),
    );
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Non-macOS stub.
#[cfg(not(target_os = "macos"))]
pub fn deliver(_title: Option<&str>, _body: &str) {}

/// Escape a string for embedding inside an AppleScript double-quoted string
/// literal. Backslash and double-quote are the literal's two metacharacters and
/// are backslash-escaped; newlines/carriage-returns would break the single-line
/// literal and are folded to spaces; any other C0 control byte is also folded to
/// a space (notifications are one-liners — control bytes carry no display value
/// and only invite terminal/AppleScript quirks). Defined platform-independently
/// so it is unit-tested on every target.
fn applescript_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::applescript_escape;

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(applescript_escape("build finished"), "build finished");
        assert_eq!(applescript_escape("café — 100% ✅"), "café — 100% ✅");
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        assert_eq!(applescript_escape(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(applescript_escape(r"a\b"), r"a\\b");
        // A backslash immediately before a quote must not let the quote escape
        // the literal: `\"` -> `\\\"` (escaped backslash, then escaped quote).
        assert_eq!(applescript_escape("\\\""), "\\\\\\\"");
    }

    #[test]
    fn applescript_injection_is_neutralized() {
        // The classic break-out: close the string, run a shell command, reopen.
        // After escaping, the embedded quotes are inert — the whole thing stays
        // one string literal, so nothing executes.
        let attack = r#"" & (do shell script "rm -rf ~") & ""#;
        let escaped = applescript_escape(attack);
        assert!(!escaped.contains('"') || escaped.contains("\\\""));
        // Every double-quote in the output is backslash-escaped.
        let bytes = escaped.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'"' {
                assert!(i > 0 && bytes[i - 1] == b'\\', "unescaped quote at {i}");
            }
        }
    }

    #[test]
    fn control_chars_and_newlines_are_folded() {
        assert_eq!(applescript_escape("line1\nline2"), "line1 line2");
        assert_eq!(applescript_escape("a\r\nb"), "a  b");
        assert_eq!(applescript_escape("tab\there"), "tab here");
        assert_eq!(applescript_escape("bell\x07x"), "bell x");
        assert_eq!(applescript_escape("esc\x1b[0m"), "esc [0m");
        // NUL is folded, not dropped or terminating.
        assert_eq!(applescript_escape("a\0b"), "a b");
    }
}
