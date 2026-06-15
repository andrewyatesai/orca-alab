// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! OSC (Operating System Command) sequence handlers for the terminal.
//!
//! This module contains handlers for various OSC sequences:
//! - OSC 7: Current working directory
//! - OSC 8: Hyperlinks
//! - OSC 52: Clipboard operations
//! - OSC 60/61/62: xterm-401 feature reporting queries
//! - OSC 66: Text sizing (Kitty protocol)
//!
//! OSC 1337 (Terminal) handlers are in `handler_osc_1337.rs`.
//! Extracted from handler.rs as part of #485 (large files refactor).

use aterm_codec::base64;

use super::ClipboardSelection;
use super::handler::TerminalHandler;
use super::{MAX_HYPERLINK_URL_BYTES, MAX_OSC52_QUERY_RESPONSE_BYTES, MAX_TITLE_BYTES};

impl TerminalHandler<'_> {
    /// Inner dispatcher for OSC sequences.
    ///
    /// Routes OSC commands by number to the appropriate handler method.
    /// Called from the `ActionSink::osc_dispatch` trait impl in handler.rs.
    pub(super) fn osc_dispatch_inner(
        &mut self,
        cap: &super::response_capability::ResponseCapability,
        params: &[&[u8]],
    ) {
        if params.is_empty() {
            return;
        }

        // Parse the OSC command number
        let cmd = match std::str::from_utf8(params[0])
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
        {
            Some(n) => n,
            None => return,
        };

        match cmd {
            0 => self.set_title(params, true, true),
            1 => self.set_title(params, true, false),
            2 => self.set_title(params, false, true),
            4 => self.handle_osc_4(cap, params),
            19 => self.handle_osc_19(params),
            7 => self.handle_osc_7(params),
            8 => self.handle_osc_8(params),
            // OSC 9: simple desktop notification (Terminal / ConEmu style).
            // Gated by host notification authorization (handler_osc_notify.rs).
            9 => self.handle_osc_9(params),
            10 => self.handle_osc_10_11_12(cap, params, 0),
            11 => self.handle_osc_10_11_12(cap, params, 1),
            12 => self.handle_osc_10_11_12(cap, params, 2),
            // OSC 13-16, 18: mouse foreground/background and Tektronix colors.
            // These are defined in xterm but not relevant to modern terminals.
            // Silently ignored (#7555).
            13 | 14 | 15 | 16 | 18 => {}
            17 => self.handle_osc_17(cap, params),
            21 => self.handle_osc_21(cap, params),
            52 => self.handle_osc_52(cap, params),
            60..=62 => self.handle_osc_feature_reporting(cap, cmd),
            66 => self.handle_osc_66(params),
            // OSC 99: kitty desktop-notification protocol.
            // Gated by host notification authorization (handler_osc_notify.rs).
            99 => self.handle_osc_99(params),
            104 => self.handle_osc_104(params),
            110 => self.reset_dynamic_color(0),
            111 => self.reset_dynamic_color(1),
            112 => self.reset_dynamic_color(2),
            // OSC 113-116, 118: reset mouse/Tektronix colors (unused, #7555).
            113 | 114 | 115 | 116 | 118 => {}
            117 => self.reset_selection_background(),
            119 => self.reset_selection_foreground(),
            133 => self.handle_osc_133(params),
            633 => self.handle_osc_633(params),
            // OSC 777: rxvt-unicode `notify` desktop notification.
            // Gated by host notification authorization (handler_osc_notify.rs).
            777 => self.handle_osc_777(params),
            // OSC 1337 (iTerm2 proprietary): image/file subsystem not wired;
            // SetUserVar dispatch is handled via shell_api, not here — ignored.
            1337 => {}
            30001 => self.handle_osc_30001(),
            30101 => self.handle_osc_30101(),
            _ => {} // Unknown OSC
        }
    }

    /// Set window title and/or icon name from an OSC title param.
    ///
    /// OSC 0 sets both icon and window. OSC 1 sets icon only. OSC 2 sets window only.
    /// The legacy v2 callback fires whenever the window title changes.
    /// The v3 event callback fires for all title changes with the title type.
    /// Titles are capped at [`MAX_TITLE_BYTES`] to prevent unbounded memory growth.
    ///
    /// Control characters (C0: 0x00-0x1F except tab, C1: 0x80-0x9F) are stripped
    /// from title strings before storage to prevent rendering artifacts and
    /// potential security issues (#7588).
    pub(super) fn set_title(&mut self, params: &[&[u8]], icon: bool, window: bool) {
        // Title text starts at params[1]. The VTE parser splits on `;`, so
        // a title containing literal semicolons (e.g. "user@host: /foo;bar")
        // will be split across params[1..]. Reconstruct by joining with ";",
        // matching how OSC 7 and OSC 8 handle URIs with semicolons (#7681).
        let title_bytes: Vec<u8> = if params.len() > 2 {
            let mut combined = params[1].to_vec();
            for extra in &params[2..] {
                combined.push(b';');
                combined.extend_from_slice(extra);
            }
            combined
        } else if let Some(&p) = params.get(1) {
            p.to_vec()
        } else {
            return;
        };
        let text_utf8 = String::from_utf8_lossy(&title_bytes);
        let text = &text_utf8[..text_utf8.floor_char_boundary(MAX_TITLE_BYTES)];
        let sanitized = sanitize_title(text);
        let text = &sanitized;
        if icon {
            self.title.icon = text.as_str().into();
        }
        if window {
            self.title.window = text.as_str().into();
            if let Some(ref mut callback) = self.title.callback {
                callback(text);
            }
        }
        // v3 event callback fires for all title types (icon, window, or both).
        if let Some(ref mut callback) = self.title.event_callback {
            let title_type = match (icon, window) {
                (true, true) => aterm_types::TitleType::WindowAndIcon,
                (true, false) => aterm_types::TitleType::IconOnly,
                (false, true) => aterm_types::TitleType::WindowOnly,
                // Unreachable: at least one of icon/window is always true
                // when set_title is called from OSC dispatch.
                (false, false) => return,
            };
            callback(title_type, text);
        }
    }

    /// Handle OSC 7 current working directory.
    ///
    /// OSC 7 format: `OSC 7 ; file://hostname/path/to/dir ST`
    ///
    /// The URI is a file:// URL pointing to the current working directory.
    /// We extract and decode the path portion for use by the terminal.
    pub(super) fn handle_osc_7(&mut self, params: &[&[u8]]) {
        // OSC 7 format: OSC 7 ; URI ST
        // params[0] = "7" (the command number, already parsed)
        // params[1..] = URI (file://hostname/path/to/dir)
        // URIs can contain literal semicolons (RFC 3986 §3.3), which the OSC
        // parser splits into separate params. Reconstruct by joining with ";".
        let uri_bytes: Vec<u8> = if params.len() > 2 {
            let mut combined = params[1].to_vec();
            for extra in &params[2..] {
                combined.push(b';');
                combined.extend_from_slice(extra);
            }
            combined
        } else if let Some(&p) = params.get(1) {
            p.to_vec()
        } else {
            // No URI provided - clear CWD
            *self.current_working_directory = None;
            self.shell_directory_changed(None);
            return;
        };

        let Some(uri) = std::str::from_utf8(&uri_bytes).ok() else {
            // No URI provided - clear CWD
            *self.current_working_directory = None;
            self.shell_directory_changed(None);
            return;
        };

        if uri.is_empty() {
            // Empty URI - clear CWD
            *self.current_working_directory = None;
            self.shell_directory_changed(None);
            return;
        }

        // Parse the file:// URI
        if let Some(path) = Self::parse_file_uri(uri) {
            *self.current_working_directory = Some(path.clone());
            self.shell_directory_changed(Some(&path));
        }
        // If not a valid file:// URI, we leave CWD unchanged
    }

    /// Parse a file:// URI and extract the path.
    ///
    /// Handles percent-encoding in the path. Returns None if not a valid file:// URI.
    fn parse_file_uri(uri: &str) -> Option<String> {
        // Check for file:// prefix
        let rest = uri.strip_prefix("file://")?;

        // The format is file://hostname/path or file:///path (empty hostname)
        // Find the start of the path (first / after hostname)
        let path_start = rest.find('/')?;
        let encoded_path = &rest[path_start..];

        // Decode percent-encoding
        Some(Self::percent_decode(encoded_path))
    }

    /// Decode percent-encoded characters in a string.
    ///
    /// Percent-encoded bytes are decoded and interpreted as UTF-8.
    /// Invalid UTF-8 sequences are replaced with the Unicode replacement character.
    fn percent_decode(s: &str) -> String {
        let mut bytes = Vec::with_capacity(s.len());
        let mut chars = s.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                // Try to read two hex digits
                let mut hex = String::with_capacity(2);
                for _ in 0..2 {
                    if let Some(&next) = chars.peek() {
                        if next.is_ascii_hexdigit() {
                            if let Some(hex_digit) = chars.next() {
                                hex.push(hex_digit);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                if hex.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        bytes.push(byte);
                        continue;
                    }
                }
                // Invalid encoding, keep as-is
                bytes.push(b'%');
                bytes.extend(hex.as_bytes());
            } else if c.is_ascii() {
                // ASCII characters go directly as bytes
                bytes.push(c as u8);
            } else {
                // Non-ASCII char already in URL - encode as UTF-8 bytes
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                bytes.extend(encoded.as_bytes());
            }
        }

        // Interpret collected bytes as UTF-8
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Handle OSC 8 hyperlinks.
    ///
    /// OSC 8 format: `OSC 8 ; params ; URI ST`
    /// - params: Optional key=value pairs separated by `:` (e.g., `id=foo:line=42`)
    /// - URI: The hyperlink URL (empty to end hyperlink)
    ///
    /// The params are parsed but currently only stored for potential future use.
    /// The primary function is to set/clear the current hyperlink URL.
    pub(super) fn handle_osc_8(&mut self, params: &[&[u8]]) {
        // OSC 8 format: OSC 8 ; params ; URI ST
        // params[0] = "8" (the command number, already parsed)
        // params[1] = params field (may be empty, contains key=value pairs like id=xxx)
        // params[2] = URI (may be empty to clear hyperlink)
        //
        // Note: Some terminals only send 2 params when clearing (OSC 8 ; ; ST)
        // because the URI is empty. We handle both cases.

        // Get the URI (third+ parameters). URIs can contain literal
        // semicolons (RFC 3986 §3.3), which the OSC parser splits into
        // separate params. Reconstruct by joining params[2..] with ";". (#7412)
        let uri_bytes: Vec<u8> = if params.len() > 2 {
            let mut combined = params[2].to_vec();
            for extra in &params[3..] {
                combined.push(b';');
                combined.extend_from_slice(extra);
            }
            combined
        } else {
            Vec::new()
        };
        let uri = std::str::from_utf8(&uri_bytes).unwrap_or("");

        if uri.is_empty() {
            // Clear hyperlink
            self.transient.current_hyperlink = None;
            self.transient.current_hyperlink_id = None;
            self.transient.update_has_transient_extras();
        } else {
            // Set hyperlink - validate it's a reasonable URL
            // We don't strictly validate the URL format, but we do ensure it's not empty
            // and doesn't contain control characters that could cause issues.
            let url_valid = uri.chars().all(|c| !c.is_control() || c == '\t');
            // Trojan Source defense (#7958, CVE-2021-42574): reject OSC 8 URLs
            // containing BiDi directional overrides (U+202A-E, U+2066-9). A URL
            // like "http://safe.example.com\u{202E}moc.live" visually reorders
            // in status bars / previews to spoof the destination hostname.
            // Legitimate URLs never contain these codepoints; reject outright
            // rather than silently strip (a sanitized URL is not the URL the
            // sender requested, and dereffing it would be misleading).
            let url_no_bidi_override = !uri
                .chars()
                .any(|c| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'));
            if url_valid
                && url_no_bidi_override
                && uri.len() <= MAX_HYPERLINK_URL_BYTES
                && is_allowed_scheme(uri)
            {
                // CF-014: route through HyperlinkCapability. The capability
                // gate is orthogonal to the scheme-allowlist / BiDi / length
                // checks above — those validate the URI shape; this mints
                // a capability iff the host has authorized OSC 8 at all.
                if let Some(token) = self.hyperlink_auth.try_mint_capability() {
                    // Parse id from params field (key=value pairs separated by ':')
                    // e.g. "id=mylink" or "id=mylink:foo=bar"
                    let id = params
                        .get(1)
                        .and_then(|p| std::str::from_utf8(p).ok())
                        .and_then(|param_str| {
                            param_str.split(':').find_map(|kv| {
                                kv.strip_prefix("id=")
                                    .filter(|v| !v.is_empty() && v.len() <= 256)
                            })
                        });
                    super::hyperlink_auth::invoke_set_hyperlink(
                        &mut *self.transient,
                        token,
                        uri,
                        id,
                    );
                }
            }
            // Invalid URLs are silently ignored (consistent with other terminals)
        }
    }

    /// Handle OSC 52 clipboard operations.
    pub(super) fn handle_osc_52(
        &mut self,
        cap: &super::response_capability::ResponseCapability,
        params: &[&[u8]],
    ) {
        // OSC 52 requires at least 2 params: the selection target and the data
        if params.len() < 2 {
            return;
        }

        // Parse selection targets (Pc parameter)
        // This is a string of characters like "c", "p", "cp", etc.
        let selection_str = match std::str::from_utf8(params[1]) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Parse selection characters into ClipboardSelection variants
        // Empty selection defaults to clipboard ('c') per xterm.
        //
        // Security: cap + de-dupe selections to avoid unbounded allocation from a maliciously
        // long Pc parameter.
        let mut selections: Vec<ClipboardSelection> = Vec::with_capacity(4);
        if selection_str.is_empty() {
            selections.push(ClipboardSelection::Clipboard);
        } else {
            for c in selection_str.chars() {
                let Some(sel) = ClipboardSelection::from_char(c) else {
                    continue;
                };
                if !selections.contains(&sel) {
                    selections.push(sel);
                    if selections.len() == 12 {
                        break;
                    }
                }
            }
        }

        if selections.is_empty() {
            return;
        }

        let selection_param: String = selections.iter().map(|s| s.to_char()).collect();

        // Get the data parameter (Pd)
        let data = params.get(2).copied().unwrap_or(&[]);

        // Determine the operation based on data content
        if data == b"?" {
            // Query operation - request clipboard content
            self.handle_osc_52_query(cap, &selections, &selection_param);
        } else if data.is_empty() {
            // Clear operation - empty data means clear
            self.handle_osc_52_clear(&selections);
        } else {
            // Set operation - decode base64 and set clipboard
            self.handle_osc_52_set(&selections, data);
        }
    }

    /// Handle OSC 52 clipboard query (Pd = "?").
    ///
    /// **Security (CF-003 + CF-005):** this path is gated by both
    /// [`super::clipboard_auth::ClipboardAuth::try_mint_query_capability`]
    /// (query authorization) and the `ResponseCapability` (response channel
    /// authorization). Without a host-minted `ClipboardQueryCapability`, the
    /// callback is never invoked and no response is emitted. Without a
    /// `ResponseCapability`, the response bytes cannot be sent. Both tokens
    /// are unforgeable outside their respective modules (private `_seal: ()`),
    /// so the parser path has no structural way to bypass either gate.
    fn handle_osc_52_query(
        &mut self,
        cap: &super::response_capability::ResponseCapability,
        selections: &[ClipboardSelection],
        selection_param: &str,
    ) {
        // Structural capability check. Returns `None` when the host has
        // not authorized query access (default posture) — the callback
        // is not reached and no PTY response is emitted. Engine-consulting
        // variant (#7994): when a policy is installed, its rule decision
        // wins over the legacy `authorize_query` bool per design §6.3.
        let Some(token) = self.clipboard_auth.try_mint_query_capability_with_engine(
            self.policy_engine.as_ref(),
            aterm_policy::OriginTag::Pty,
        ) else {
            return;
        };
        let Some(content) =
            super::clipboard_auth::invoke_query(&mut self.clipboard.callback, token, selections)
        else {
            return; // Callback returned None or is unwired — don't respond.
        };
        // Cap on decoded bytes; see MAX_OSC52_QUERY_RESPONSE_BYTES doc for wire-size notes.
        if content.len() > MAX_OSC52_QUERY_RESPONSE_BYTES {
            return;
        }
        // Encode the clipboard content and send response.
        // Response format: OSC 52 ; Pc ; <base64> <terminator>
        // Use the same terminator (BEL vs ST) as the request for compatibility
        // with programs that only recognize BEL-terminated responses (#7548).
        let encoded = base64::encode(content.as_bytes());
        let terminator = if self.transient.last_osc_bel_terminated {
            "\x07"
        } else {
            "\x1b\\"
        };
        let response = format!("\x1b]52;{selection_param};{encoded}{terminator}");
        self.send_response(cap, response.as_bytes());
    }

    /// Handle OSC 52 clipboard clear (empty Pd).
    ///
    /// **Security (CF-004):** clear is gated by the same
    /// [`super::clipboard_auth::ClipboardWriteCapability`] as *set*. The
    /// policy choice is documented on [`super::clipboard_auth::invoke_clear`]:
    /// clear is a strictly-less-dangerous subset of set (an attacker can
    /// only empty the clipboard, not inject arbitrary content), and
    /// distinguishing the two tokens would make host configuration more
    /// confusing without adding meaningful defense in depth.
    fn handle_osc_52_clear(&mut self, selections: &[ClipboardSelection]) {
        // Engine-consulting variant (#7994): the OSC 52 *set* rule gates
        // *clear* too (per invoke_clear's doc, clear is strictly-less-
        // dangerous than set and shares the write capability).
        let Some(token) = self.clipboard_auth.try_mint_write_capability_with_engine(
            self.policy_engine.as_ref(),
            aterm_policy::OriginTag::Pty,
        ) else {
            return;
        };
        super::clipboard_auth::invoke_clear(&mut self.clipboard.callback, token, selections);
    }

    /// Handle OSC 52 clipboard set (Pd = base64-encoded data).
    ///
    /// **Security (CF-004):** gated by
    /// [`super::clipboard_auth::ClipboardAuth::try_mint_write_capability`].
    /// Without a host-minted [`super::clipboard_auth::ClipboardWriteCapability`],
    /// the callback is never invoked and no PTY-origin bytes reach the
    /// host clipboard delegate. The capability is unforgeable outside
    /// `clipboard_auth.rs` (private `_seal: ()` field), so the parser
    /// path has no structural way to bypass this gate.
    fn handle_osc_52_set(&mut self, selections: &[ClipboardSelection], data: &[u8]) {
        // Mint the capability early. If the host hasn't authorized
        // clipboard write, we skip the expensive base64 decode as well —
        // an attacker blasting ungated OSC 52 ; c ; <huge base64> at the
        // terminal should not burn CPU on decode we'll throw away.
        // Engine-consulting variant (#7994): policy decision wins over
        // the legacy bool per design §6.3.
        let Some(token) = self.clipboard_auth.try_mint_write_capability_with_engine(
            self.policy_engine.as_ref(),
            aterm_policy::OriginTag::Pty,
        ) else {
            return;
        };

        // Decode base64 data
        let data_str = match std::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return, // Invalid UTF-8 in base64, ignore
        };
        let decoded = match base64::decode(data_str) {
            Ok(bytes) => bytes,
            Err(_) => return, // Invalid base64, ignore
        };
        if decoded.len() > MAX_OSC52_QUERY_RESPONSE_BYTES {
            return;
        }

        // Convert to UTF-8 string
        let content = match String::from_utf8(decoded) {
            Ok(s) => s,
            Err(_) => return, // Invalid UTF-8, ignore
        };

        super::clipboard_auth::invoke_set(&mut self.clipboard.callback, token, selections, content);
    }

    /// Handle OSC 66 - Text sizing (Kitty protocol).
    ///
    /// Format: `OSC 66 ; metadata ; text ST`
    ///
    /// The metadata is a colon-separated list of key=value pairs controlling
    /// text rendering dimensions and alignment.
    ///
    /// # Reference
    ///
    /// <https://sw.kovidgoyal.net/kitty/text-sizing-protocol/>
    pub(super) fn handle_osc_66(&mut self, params: &[&[u8]]) {
        // Need at least: OSC code, metadata, text
        if params.len() < 3 {
            return;
        }

        // Parse metadata (second parameter)
        let metadata = match std::str::from_utf8(params[1]) {
            Ok(s) => s,
            Err(_) => return,
        };

        // Collect text (may span multiple params if semicolons in content)
        let text = if params.len() == 3 {
            match std::str::from_utf8(params[2]) {
                Ok(s) => s.to_string(),
                Err(_) => return,
            }
        } else {
            // Reconstruct text with embedded semicolons
            let mut text = String::new();
            for (idx, param) in params[2..].iter().enumerate() {
                if idx > 0 {
                    text.push(';');
                }
                match std::str::from_utf8(param) {
                    Ok(s) => text.push_str(s),
                    Err(_) => return,
                }
            }
            text
        };

        // Parse into operation and invoke callback
        let operation = super::types::TextSizingOperation::parse(metadata, &text);
        if let Some(callback) = self.text_sizing_callback {
            callback(operation);
        }
    }

    /// Handle OSC 60/61/62 - xterm feature reporting (xterm-401).
    ///
    /// These sequences allow applications to query which features the terminal
    /// supports. Introduced in xterm-401 (2025-07-02).
    ///
    /// # OSC Numbers
    ///
    /// - **OSC 60**: Obsolete/reserved, no response sent
    /// - **OSC 61**: Query allowWindowOps - which window manipulation operations are enabled
    /// - **OSC 62**: Query feature list - which terminal features are enabled
    ///
    /// # Response Format
    ///
    /// - OSC 61: `ESC ] 61 ; <value> ST` where value is "true" (all ops allowed)
    /// - OSC 62: `ESC ] 62 ; feature1 ; feature2 ; ... ST`
    ///
    /// # aterm-core Implementation
    ///
    /// Since aterm-core is a library (not a full terminal emulator), we report:
    /// - OSC 61: All window ops allowed (value "true") - actual control is UI layer
    /// - OSC 62: Features from `TerminalCapabilities::aterm_capabilities()`
    ///
    /// # Reference
    ///
    /// See: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
    pub(super) fn handle_osc_feature_reporting(
        &mut self,
        cap: &super::response_capability::ResponseCapability,
        cmd: u32,
    ) {
        match cmd {
            60 => {
                // OSC 60 is obsolete/reserved - no response per xterm behavior
            }
            61 => {
                // OSC 61 - allowWindowOps query
                // aterm-core doesn't control window operations (that's UI layer),
                // so we report all operations as allowed.
                //
                // xterm uses numeric bitmask, but "true" is simpler and compatible.
                // Match request terminator per #7548.
                let st = if self.transient.last_osc_bel_terminated {
                    "\x07"
                } else {
                    "\x1b\\"
                };
                let response = format!("\x1b]61;true{st}");
                self.send_response(cap, response.as_bytes());
            }
            62 => {
                // OSC 62 - Feature list query
                // Report features from TerminalCapabilities as semicolon-separated list.
                //
                // Feature names follow xterm conventions where possible.
                // Match request terminator per #7548.
                use super::types::TerminalCapabilities;
                let features = TerminalCapabilities::aterm_capabilities().feature_list_string();
                let st = if self.transient.last_osc_bel_terminated {
                    "\x07"
                } else {
                    "\x1b\\"
                };
                let response = format!("\x1b]62;{features}{st}");
                self.send_response(cap, response.as_bytes());
            }
            _ => {
                // Unreachable - only called for 60/61/62
            }
        }
    }
}

/// Strip control characters from a title string (#7588, #7958).
///
/// Removes:
/// - C0 controls (0x00-0x1F) except tab (0x09)
/// - C1 controls (0x80-0x9F)
/// - Unicode bidirectional override codepoints (U+202A-U+202E, U+2066-U+2069)
///
/// The control-character strip (#7588) prevents rendering artifacts, line breaks
/// in title bars, and embedded-ESC attacks. The BiDi override strip (#7958,
/// CVE-2021-42574 / "Trojan Source") prevents window-title spoofing where a
/// title like `"OSC]2;safe\u{202E}livemaster.com\u{202C}.ru\x07"` visually
/// reorders to a different apparent hostname in the title bar.
///
/// Title surfaces do not flow through the grid's `BidiSecurity::Strict`
/// filter (handler_write.rs, #7913), so the strip is unconditional here —
/// matching the sibling unconditional strip in `handler_osc_notify.rs` for
/// OSC 9 / Terminal notifications.
fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .filter(|&c| {
            // Allow tab (0x09), reject other C0 (0x00-0x1F) and all C1 (0x80-0x9F)
            if c == '\t' {
                return true;
            }
            let code = c as u32;
            // C0 range: 0x00-0x1F
            if code <= 0x1F {
                return false;
            }
            // C1 range: 0x80-0x9F
            if (0x80..=0x9F).contains(&code) {
                return false;
            }
            // BiDi directional overrides (CVE-2021-42574 / Trojan Source).
            // U+202A LRE, U+202B RLE, U+202C PDF, U+202D LRO, U+202E RLO
            // U+2066 LRI, U+2067 RLI, U+2068 FSI, U+2069 PDI
            if matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') {
                return false;
            }
            true
        })
        .collect()
}

/// Check if a URI has a scheme that is allowed for OSC 8 hyperlinks.
///
/// This is an **allowlist** check: only URIs with schemes in the default
/// safe list (`http`, `https`, `mailto`, `sftp`, `tel`) are accepted.
/// Everything else — including `ssh:`/`git:` (rejected since #7989, see
/// below), attacker-registered macOS URL handlers (`slack:`, `zoom:`,
/// `vscode:`, `ms-word:`, `applefeedback:`, arbitrary custom schemes) and
/// dangerous schemes (`javascript:`, `data:`, `file:`, `ftp:`, etc.) — is
/// refused at parse time. Case-insensitive.
///
/// Converted from blocklist (`has_dangerous_scheme`) to allowlist in #7919
/// after F01-4 (HN-P1) demonstrated that attacker-registered URL handlers
/// could slip past the blocklist and launch native apps via `NSWorkspace.open`.
///
/// Since #7989 (CVE-2023-51385 class) `ssh` and `git` are rejected by default;
/// `file`/`ftp` and all custom app schemes are likewise refused.
/// (#7413, #7495, #7700, #7919, #7989)
#[must_use]
fn is_allowed_scheme(uri: &str) -> bool {
    /// RFC 3986 safe scheme allowlist for OSC 8 hyperlinks.
    const SAFE_SCHEMES: &[&str] = &["http", "https", "mailto", "sftp", "tel"];

    // Extract the RFC 3986 scheme: the run of characters before the first
    // ':'. A valid scheme starts with ALPHA and continues with
    // ALPHA / DIGIT / '+' / '-' / '.'. Anything else (missing colon, empty
    // scheme, digit/space lead, illegal character) is not a valid scheme.
    let Some(colon) = uri.find(':') else {
        return false;
    };
    let scheme = &uri[..colon];
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return false;
    }
    SAFE_SCHEMES
        .iter()
        .any(|safe| scheme.eq_ignore_ascii_case(safe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Terminal;
    use aterm_policy::{engine::PolicyEngine, profiles};

    // ---- sanitize_title (#7588, #7958) --------------------------------------

    #[test]
    fn sanitize_title_plain_ascii_unchanged() {
        assert_eq!(sanitize_title("Hello World"), "Hello World");
    }

    #[test]
    fn sanitize_title_tab_preserved() {
        // Tab (0x09) is explicitly allowed.
        assert_eq!(sanitize_title("Col1\tCol2"), "Col1\tCol2");
    }

    #[test]
    fn sanitize_title_strips_c0_controls() {
        assert_eq!(sanitize_title("a\x00b\x01c"), "abc");
        assert_eq!(sanitize_title("a\x1bESCb"), "aESCb");
        assert_eq!(sanitize_title("a\x0Ab"), "ab"); // LF
        assert_eq!(sanitize_title("a\x0Db"), "ab"); // CR
    }

    #[test]
    fn sanitize_title_strips_c1_controls() {
        assert_eq!(sanitize_title("a\u{0080}b"), "ab");
        assert_eq!(sanitize_title("a\u{009B}31mb"), "a31mb"); // C1 CSI
        assert_eq!(sanitize_title("a\u{009F}b"), "ab");
    }

    #[test]
    fn sanitize_title_strips_bidi_overrides_202a_202e() {
        // CVE-2021-42574 / Trojan Source — U+202A..U+202E (LRE/RLE/PDF/LRO/RLO).
        assert_eq!(sanitize_title("safe\u{202A}evil"), "safeevil"); // LRE
        assert_eq!(sanitize_title("safe\u{202B}evil"), "safeevil"); // RLE
        assert_eq!(sanitize_title("safe\u{202C}evil"), "safeevil"); // PDF
        assert_eq!(sanitize_title("safe\u{202D}evil"), "safeevil"); // LRO
        assert_eq!(sanitize_title("safe\u{202E}evil"), "safeevil"); // RLO
    }

    #[test]
    fn sanitize_title_strips_bidi_isolates_2066_2069() {
        // CVE-2021-42574 — U+2066..U+2069 (LRI/RLI/FSI/PDI).
        assert_eq!(sanitize_title("safe\u{2066}evil"), "safeevil"); // LRI
        assert_eq!(sanitize_title("safe\u{2067}evil"), "safeevil"); // RLI
        assert_eq!(sanitize_title("safe\u{2068}evil"), "safeevil"); // FSI
        assert_eq!(sanitize_title("safe\u{2069}evil"), "safeevil"); // PDI
    }

    #[test]
    fn sanitize_title_strips_all_nine_bidi_overrides_concatenated() {
        // Full Trojan Source payload — all 9 override codepoints in one string.
        let payload = "X\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}Y";
        assert_eq!(sanitize_title(payload), "XY");
    }

    #[test]
    fn sanitize_title_preserves_legitimate_unicode() {
        // CJK, Arabic, Hebrew, and other non-override Unicode must pass
        // through — only the 9 explicit-override codepoints are stripped.
        assert_eq!(
            sanitize_title("\u{65E5}\u{672C}\u{8A9E}"),
            "\u{65E5}\u{672C}\u{8A9E}"
        );
        let arabic = "\u{0627}\u{0644}\u{0639}"; // alef lam ain
        assert_eq!(sanitize_title(arabic), arabic);
        // Pure-RTL scripts (without override codepoints) are safe.
        let hebrew = "\u{05D0}\u{05D1}\u{05D2}"; // aleph bet gimel
        assert_eq!(sanitize_title(hebrew), hebrew);
    }

    #[test]
    fn sanitize_title_boundary_codepoints_below_and_above_override_ranges() {
        // U+2029 is one below U+202A — must pass through.
        assert_eq!(sanitize_title("a\u{2029}b"), "a\u{2029}b");
        // U+202F (NARROW NO-BREAK SPACE) is one above U+202E — must pass through.
        assert_eq!(sanitize_title("a\u{202F}b"), "a\u{202F}b");
        // U+2065 is one below U+2066 — must pass through.
        assert_eq!(sanitize_title("a\u{2065}b"), "a\u{2065}b");
        // U+206A is one above U+2069 — must pass through.
        assert_eq!(sanitize_title("a\u{206A}b"), "a\u{206A}b");
    }

    // ---- OSC 0/1/2 end-to-end (Terminal::process) --------------------------

    #[test]
    fn osc_2_title_with_rlo_bidi_override_is_sanitized() {
        // CVE-2021-42574 repro — OSC 2 sets the window title. A title like
        // `"safe\u{202E}moc.livemaster\u{202C}.ru"` visually reorders in the
        // title bar to spoof `safesemaster.com.ru` style displays.
        let mut term = Terminal::new(24, 80);
        let payload = "safe\u{202E}evil.example";
        let seq = format!("\x1b]2;{payload}\x07");
        term.process(seq.as_bytes());
        assert_eq!(
            term.title(),
            "safeevil.example",
            "U+202E must be stripped from OSC 2 window title"
        );
    }

    #[test]
    fn osc_0_title_strips_all_nine_bidi_overrides() {
        let mut term = Terminal::new(24, 80);
        // All 9 codepoints concatenated with surrounding benign text.
        let payload = "X\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}Y";
        let seq = format!("\x1b]0;{payload}\x07");
        term.process(seq.as_bytes());
        assert_eq!(term.title(), "XY", "all 9 BiDi overrides must be stripped");
        assert_eq!(
            term.icon_name(),
            "XY",
            "OSC 0 icon name must also be sanitized"
        );
    }

    #[test]
    fn osc_1_icon_strips_bidi_override() {
        let mut term = Terminal::new(24, 80);
        let payload = "safe\u{2066}evil";
        let seq = format!("\x1b]1;{payload}\x07");
        term.process(seq.as_bytes());
        assert_eq!(
            term.icon_name(),
            "safeevil",
            "U+2066 LRI must be stripped from OSC 1 icon name"
        );
    }

    #[test]
    fn osc_0_invalid_utf8_title_is_lossy_decoded() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b]0;Title\x9cMore\x07");
        assert_eq!(
            term.title(),
            "Title\u{FFFD}More",
            "invalid UTF-8 title payload must be lossily decoded"
        );
        assert_eq!(
            term.icon_name(),
            "Title\u{FFFD}More",
            "invalid UTF-8 icon payload must be lossily decoded"
        );

        term.process(b"\x1b]0;Recovery\x07");
        assert_eq!(term.title(), "Recovery");
        assert_eq!(term.icon_name(), "Recovery");
    }

    // ---- OSC 8 URL bidi-override rejection (#7958) --------------------------

    #[test]
    fn osc_8_url_with_rlo_bidi_override_rejected() {
        // CVE-2021-42574 — a crafted URL like
        // "http://safe.example\u{202E}moc.live" reorders in status bars and
        // link previews to spoof a different hostname. Reject outright.
        let mut term = Terminal::new(24, 80);
        term.process("\x1b]8;;http://safe.example\u{202E}moc.live\x07".as_bytes());
        assert!(
            term.current_hyperlink().is_none(),
            "OSC 8 URL containing U+202E must be rejected outright (#7958)"
        );
    }

    #[test]
    fn osc_8_url_with_each_of_nine_bidi_overrides_rejected() {
        // Verify each of the 9 codepoints individually triggers rejection.
        let codepoints = [
            '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2067}',
            '\u{2068}', '\u{2069}',
        ];
        for cp in codepoints {
            let mut term = Terminal::new(24, 80);
            let url = format!("https://example.com/{cp}path");
            let seq = format!("\x1b]8;;{url}\x07");
            term.process(seq.as_bytes());
            assert!(
                term.current_hyperlink().is_none(),
                "OSC 8 URL containing U+{:04X} must be rejected",
                cp as u32
            );
        }
    }

    #[test]
    fn osc_8_clean_url_still_accepted_after_bidi_filter() {
        // Regression guard: the bidi-override rejection must not break
        // legitimate https:// URLs (which have no overrides).
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b]8;;https://example.com/path\x07");
        assert_eq!(
            term.current_hyperlink().map(|s| s.as_ref()),
            Some("https://example.com/path"),
            "clean URL must remain accepted"
        );
    }

    #[test]
    fn osc_8_url_with_bidi_override_does_not_disturb_prior_hyperlink() {
        // If a valid hyperlink is already set and then an override-carrying
        // URL arrives, the attacker must not be able to clear or replace the
        // prior hyperlink. The invalid URL is silently ignored (matching the
        // existing is_allowed_scheme rejection path) and the prior URL stays.
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b]8;;https://safe.example/a\x07");
        assert_eq!(
            term.current_hyperlink().map(|s| s.as_ref()),
            Some("https://safe.example/a")
        );
        term.process("\x1b]8;;http://attacker\u{202E}example.com\x07".as_bytes());
        // Prior hyperlink is preserved — the attacker's URL was invalid and
        // silently dropped, same as an unknown-scheme URL.
        assert_eq!(
            term.current_hyperlink().map(|s| s.as_ref()),
            Some("https://safe.example/a"),
            "prior valid hyperlink must be preserved when new URL contains BiDi override"
        );
    }

    #[test]
    fn osc_52_standard_policy_wildcard_does_not_overgrant_revoked_set() {
        use crate::terminal::ClipboardOperation;
        use std::sync::{Arc, Mutex};

        let mut term = Terminal::new(24, 80);
        let captured = Arc::new(Mutex::new(None::<String>));
        let captured_clone = Arc::clone(&captured);
        term.set_clipboard_callback(move |op| {
            if let ClipboardOperation::Set { content, .. } = op {
                *captured_clone.lock().expect("poisoned") = Some(content);
            }
            None
        });
        term.apply_policy_engine(PolicyEngine::new(profiles::standard()));

        term.process(b"\x1b]52;c;SGVsbG8=\x07");

        assert_eq!(*captured.lock().expect("poisoned"), None);
    }

    #[test]
    fn osc_52_standard_policy_wildcard_does_not_overgrant_revoked_query() {
        let mut term = Terminal::new(24, 80);
        term.set_clipboard_callback(|op| match op {
            crate::terminal::ClipboardOperation::Query { .. } => Some("secret".to_string()),
            _ => None,
        });
        term.apply_policy_engine(PolicyEngine::new(profiles::standard()));

        term.process(b"\x1b]52;c;?\x07");

        assert!(
            term.take_response().is_none(),
            "standard wildcard Execute must not reopen OSC 52 query"
        );
    }
}
