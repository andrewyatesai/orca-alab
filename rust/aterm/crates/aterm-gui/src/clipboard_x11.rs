// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! A self-contained X11 clipboard (CLIPBOARD + PRIMARY) over the pure-Rust x11rb
//! connection — NO external helper binary (`xclip`/`xsel`/`wl-copy`) and NO new
//! external crate (x11rb is already in the build as a winit transitive dependency).
//! On macOS the GUI shells out to `pbcopy`/`pbpaste`; this is the Linux/X11 twin so
//! copy + paste actually WORK as a daily driver (the old code hard-coded
//! `/usr/bin/pbcopy`, which does not exist on Linux, so every copy silently failed).
//!
//! X11 has no "set the clipboard and forget" primitive: the copying client must OWN
//! the selection and keep answering `SelectionRequest` conversions for as long as
//! the data should stay pasteable. So one background thread owns a single X
//! connection plus an unmapped 1×1 window, takes selection ownership on
//! [`set`](X11Clipboard::set), and serves conversions from its event loop. A
//! [`get`](X11Clipboard::get) returns our own stored text when we still own the
//! selection (the common "copy here, paste here" case) or asks the current owner via
//! `ConvertSelection` and waits briefly for the reply.
//!
//! Scope: UTF-8 text, single-shot transfers (no INCR — a multi-megabyte clipboard
//! payload is not served/read; ordinary terminal copy/paste is far below the X
//! max-request size). Wayland-only sessions (no `$DISPLAY`) get a `None` from
//! [`X11Clipboard::get_handle`] and the caller degrades gracefully.

use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SELECTION_NOTIFY_EVENT,
    SelectionNotifyEvent, SelectionRequestEvent, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

/// Which X selection to read/write. `Clipboard` is the explicit-copy buffer
/// (Ctrl+Shift+C / Ctrl+Shift+V); `Primary` is the select-to-copy / middle-click
/// buffer that X users also rely on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sel {
    Clipboard,
    Primary,
}

/// The atoms the clipboard protocol needs, interned once at startup.
struct Atoms {
    clipboard: Atom,
    primary: Atom,
    utf8: Atom,
    targets: Atom,
    text_plain: Atom,
    /// The property on our own window that `ConvertSelection` deposits a paste into.
    recv: Atom,
}

/// Shared state between the event-serving thread and the `set`/`get` callers. The
/// `RustConnection` is `Sync` and supports concurrent write-only requests (set
/// owner / convert) from a caller thread while the event thread blocks in
/// `wait_for_event`, which is exactly the access pattern here.
struct Inner {
    conn: RustConnection,
    win: Window,
    atoms: Atoms,
    /// Text we currently serve for CLIPBOARD (the selection we own), or `None`.
    clipboard: Mutex<Option<String>>,
    /// Text we currently serve for PRIMARY, or `None`.
    primary: Mutex<Option<String>>,
    /// A `get` awaiting its `SelectionNotify`; the event thread fulfils it.
    pending: Mutex<Option<Sender<Option<String>>>>,
}

impl Inner {
    /// The stored-text slot for `sel`.
    fn slot(&self, sel: Atom) -> Option<&Mutex<Option<String>>> {
        if sel == self.atoms.clipboard {
            Some(&self.clipboard)
        } else if sel == self.atoms.primary {
            Some(&self.primary)
        } else {
            None
        }
    }
}

/// A live X11 clipboard: an owning thread + the shared state it serves.
pub(crate) struct X11Clipboard {
    inner: std::sync::Arc<Inner>,
}

impl X11Clipboard {
    /// The process-wide clipboard, connected lazily. `Some` on an X11 session,
    /// `None` when there is no X display to connect to (e.g. a pure-Wayland or
    /// headless session) — callers then degrade gracefully.
    pub(crate) fn get_handle() -> Option<&'static X11Clipboard> {
        static INSTANCE: OnceLock<Option<X11Clipboard>> = OnceLock::new();
        INSTANCE.get_or_init(X11Clipboard::connect).as_ref()
    }

    /// Connect, intern atoms, create the owner window, and spawn the serving
    /// thread. `None` if any step fails (no display, protocol error).
    fn connect() -> Option<X11Clipboard> {
        let (conn, screen_num) = RustConnection::connect(None).ok()?;
        let screen = conn.setup().roots.get(screen_num)?;
        let root = screen.root;
        let root_visual = screen.root_visual;
        let win = conn.generate_id().ok()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            win,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            root_visual,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .ok()?;
        let atoms = Atoms {
            clipboard: conn.intern_atom(false, b"CLIPBOARD").ok()?.reply().ok()?.atom,
            primary: AtomEnum::PRIMARY.into(),
            utf8: conn.intern_atom(false, b"UTF8_STRING").ok()?.reply().ok()?.atom,
            targets: conn.intern_atom(false, b"TARGETS").ok()?.reply().ok()?.atom,
            text_plain: conn
                .intern_atom(false, b"text/plain;charset=utf-8")
                .ok()?
                .reply()
                .ok()?
                .atom,
            recv: conn
                .intern_atom(false, b"ATERM_CLIPBOARD_RECV")
                .ok()?
                .reply()
                .ok()?
                .atom,
        };
        conn.flush().ok()?;
        let inner = std::sync::Arc::new(Inner {
            conn,
            win,
            atoms,
            clipboard: Mutex::new(None),
            primary: Mutex::new(None),
            pending: Mutex::new(None),
        });
        let thread_inner = std::sync::Arc::clone(&inner);
        // Detached: lives for the process. The OS reclaims the connection on exit.
        std::thread::Builder::new()
            .name("aterm-x11-clipboard".into())
            .spawn(move || serve(&thread_inner))
            .ok()?;
        Some(X11Clipboard { inner })
    }

    /// Take ownership of `sel` and serve `text` to any client that pastes it.
    /// Returns whether ownership was requested successfully.
    pub(crate) fn set(&self, sel: Sel, text: &str) -> bool {
        let atom = self.sel_atom(sel);
        if let Some(slot) = self.inner.slot(atom) {
            *slot.lock().unwrap() = Some(text.to_owned());
        }
        let ok = self
            .inner
            .conn
            .set_selection_owner(self.inner.win, atom, CURRENT_TIME)
            .is_ok();
        let _ = self.inner.conn.flush();
        ok
    }

    /// Read the current contents of `sel` as UTF-8 text, or `None` if empty /
    /// unavailable. Fast-pathed to our own stored text when we own the selection.
    pub(crate) fn get(&self, sel: Sel) -> Option<String> {
        let atom = self.sel_atom(sel);
        if let Some(slot) = self.inner.slot(atom)
            && let Some(t) = slot.lock().unwrap().clone()
        {
            return Some(t);
        }
        let (tx, rx) = channel();
        *self.inner.pending.lock().unwrap() = Some(tx);
        let req = self.inner.conn.convert_selection(
            self.inner.win,
            atom,
            self.inner.atoms.utf8,
            self.inner.atoms.recv,
            CURRENT_TIME,
        );
        if req.is_err() || self.inner.conn.flush().is_err() {
            *self.inner.pending.lock().unwrap() = None;
            return None;
        }
        let got = rx.recv_timeout(Duration::from_millis(1000)).ok().flatten();
        *self.inner.pending.lock().unwrap() = None;
        got
    }

    fn sel_atom(&self, sel: Sel) -> Atom {
        match sel {
            Sel::Clipboard => self.inner.atoms.clipboard,
            Sel::Primary => self.inner.atoms.primary,
        }
    }
}

/// The serving thread: answer conversion requests for selections we own and fulfil
/// our own pending `get`. Exits only if the connection dies.
fn serve(inner: &Inner) {
    loop {
        let ev = match inner.conn.wait_for_event() {
            Ok(ev) => ev,
            Err(_) => return, // connection gone — nothing more to serve
        };
        match ev {
            Event::SelectionRequest(req) => serve_request(inner, &req),
            Event::SelectionClear(clear) => {
                // Another client took the selection; stop serving our stale copy.
                if let Some(slot) = inner.slot(clear.selection) {
                    *slot.lock().unwrap() = None;
                }
            }
            Event::SelectionNotify(ev) => {
                let text = if ev.property == NONE {
                    None
                } else {
                    read_paste(inner, ev.property)
                };
                if let Some(tx) = inner.pending.lock().unwrap().take() {
                    let _ = tx.send(text);
                }
            }
            _ => {}
        }
    }
}

/// Answer one `SelectionRequest`: place the requested data on the requestor's
/// property and reply with a `SelectionNotify` (property = `NONE` on refusal).
fn serve_request(inner: &Inner, req: &SelectionRequestEvent) {
    let a = &inner.atoms;
    // Obsolete clients send property = None; the convention is to use `target`.
    let property = if req.property == NONE {
        req.target
    } else {
        req.property
    };
    let text = inner
        .slot(req.selection)
        .and_then(|s| s.lock().unwrap().clone());
    let mut reported = property;
    let mut ok = false;
    if let Some(text) = text {
        if req.target == a.utf8
            || req.target == Atom::from(AtomEnum::STRING)
            || req.target == a.text_plain
        {
            ok = inner
                .conn
                .change_property8(PropMode::REPLACE, req.requestor, property, req.target, text.as_bytes())
                .is_ok();
        } else if req.target == a.targets {
            let list = [a.targets, a.utf8, Atom::from(AtomEnum::STRING), a.text_plain];
            ok = inner
                .conn
                .change_property32(
                    PropMode::REPLACE,
                    req.requestor,
                    property,
                    Atom::from(AtomEnum::ATOM),
                    &list,
                )
                .is_ok();
        }
    }
    if !ok {
        reported = NONE;
    }
    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: req.time,
        requestor: req.requestor,
        selection: req.selection,
        target: req.target,
        property: reported,
    };
    let _ = inner
        .conn
        .send_event(false, req.requestor, EventMask::NO_EVENT, notify);
    let _ = inner.conn.flush();
}

/// Read (and delete) the paste text the owner deposited on our `recv` property.
/// `None` for an empty value or an unsupported INCR (large) transfer.
fn read_paste(inner: &Inner, property: Atom) -> Option<String> {
    let reply = inner
        .conn
        .get_property(true, inner.win, property, AtomEnum::ANY, 0, u32::MAX)
        .ok()?
        .reply()
        .ok()?;
    if reply.value.is_empty() {
        return None;
    }
    String::from_utf8(reply.value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end CROSS-CLIENT round-trip: two independent `X11Clipboard` handles
    /// (each its own X connection + owner window) — `a` copies, then `b` pastes by
    /// asking the server for the CLIPBOARD selection, which exercises the FULL X
    /// `SetSelectionOwner` → `ConvertSelection` → `SelectionRequest`/`SelectionNotify`
    /// protocol exactly as a real second app (a browser, another terminal) would.
    /// Proves copy in aterm is pasteable elsewhere on this X11 box. Skipped (not
    /// failed) when there is no X display to connect to.
    #[test]
    fn cross_client_clipboard_round_trip() {
        let (Some(a), Some(b)) = (X11Clipboard::connect(), X11Clipboard::connect()) else {
            eprintln!("SKIP: no X display for the clipboard round-trip test");
            return;
        };
        let payload = "aterm clipboard ✓ 你好 😀";
        assert!(a.set(Sel::Clipboard, payload), "a must take CLIPBOARD ownership");
        // Let `a`'s serving thread register ownership before `b` asks for it.
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(
            b.get(Sel::Clipboard).as_deref(),
            Some(payload),
            "b must read back exactly what a copied (cross-client)"
        );
        // PRIMARY is an independent buffer (select-to-copy / middle-click).
        let prim = "primary selection text";
        assert!(a.set(Sel::Primary, prim));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(b.get(Sel::Primary).as_deref(), Some(prim));
        // CLIPBOARD is unchanged by the PRIMARY write (the two never alias).
        assert_eq!(b.get(Sel::Clipboard).as_deref(), Some(payload));
    }
}
