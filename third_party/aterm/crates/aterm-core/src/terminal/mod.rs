// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Terminal emulator — combines the parser and grid.
//!
//! See handler modules (`handler_csi.rs`, `handler_esc.rs`, `handler_osc.rs`)
//! for per-sequence documentation.

mod bidi_stubs;
mod blocks_api;
mod buffer_api;
mod builder;
mod callback_clearers;
mod callback_setters;
mod callbacks;
pub(crate) mod clipboard_auth;
pub mod color_resolve;
mod colors_api;
mod config_api;
mod constructors;
mod content;
mod csi_dispatch_table;
pub(crate) mod dcs_auth;
mod grouped_state;
mod handler;
mod handler_dec;
mod handler_decrqss;
mod handler_osc;
mod handler_osc_color;
mod handler_osc_notify;
mod handler_osc_shell;
mod handler_report;
mod handler_sgr;
mod handler_state;
mod handler_window;
mod handler_xtgettcap;
mod handler_xtsmgraphics;
pub(crate) mod host_traits;
pub(crate) mod hyperlink_auth;
mod keyboard_mode;
#[cfg(test)]
pub mod mouse;
#[cfg(not(test))]
mod mouse;
mod policy_bridge;
mod processing;
mod render_cells;
mod reset;
mod response_capability;
mod response_rate_limiter;
mod selection;
mod shell;
pub(crate) mod shell_integration_auth;
mod window_auth;

// Phase 1+2: Universal block model, process detection, and AtermApi.
mod shell_api;

mod state;
mod state_accessors;
mod transient_state;
mod types;

use callbacks::{
    BufferActivationCallback, MAX_DCS_CALLBACK_BYTES, MAX_DCS_GLOBAL_BUDGET, SGR_STACK_MAX_DEPTH,
    TITLE_STACK_MAX_DEPTH, WindowCallback,
};
pub use callbacks::{ColorChangeOp, ColorTarget};
#[cfg(feature = "sixel")]
use grouped_state::SixelState;
use grouped_state::{
    ClipboardState, ColorState, CursorSaveState, DcsState, DcsType, Iterm2State, MarksState,
    NotificationState, SemanticState, ShellIntegrationState, TitleState,
};
use reset::{ResetGroups, reset_common_fields};
use transient_state::{TransientState, Vt52CursorState};

pub(crate) use aterm_types::charset::CharacterSetState;
pub use aterm_types::{ColorPalette, Rgb};
pub use aterm_types::{KittyKeyboardFlags, KittyKeyboardState};
pub use builder::TerminalBuilder;
pub use callbacks::{
    CALLBACK_REGISTRY, CallbackCategory, CallbackInfo, SshConductorCallbackEvent,
    TmuxCallbackEvent, callback_by_name, callback_count, callback_info,
};
pub use blocks_api::BlockText;
pub use shell::{
    Annotation, BlockState, CommandMark, OutputBlock, ShellEvent, ShellState, TerminalMark,
};
pub use types::{
    ClipboardOperation, ClipboardSelection, CopyToClipboardOperation, CurrentStyle, CursorStyle,
    Iterm2CellSize, Iterm2SetColor, Iterm2ShellIntegrationVersion, MouseEncoding, MouseMode,
    TerminalModes, TerminalSize, TerminalSnapshot,
};
#[allow(
    unused_imports,
    reason = "RemoteHost re-export used by session::terminal_state; dead_code propagation hides usage"
)]
pub(crate) use types::{
    MultipartFileOperation, RemoteHost, SavedCursorState, SemanticBlock, SemanticBlockEvent,
    SemanticButton, SemanticButtonEvent, SemanticButtonType,
};
// Terminal-internal: not re-exported to crate level
pub use aterm_types::XtermKeyboardState;
pub use aterm_types::{WindowOperation, WindowResponse};
pub use clipboard_auth::ClipboardAccess;
pub use render_cells::{RenderCell, UnderlineStyle};
pub use state::Terminal;
use types::{SgrPushMask, SgrStackEntry, TaskbarProgress};

use crate::grid::Cursor;

/// Feature-gated terminal-internal constants needed by extracted test crates.
#[cfg(test)]
pub mod testing {
    /// Maximum completed command marks (OSC 133).
    pub const COMMAND_MARKS_MAX: usize = super::shell::COMMAND_MARKS_MAX;
    /// Maximum completed output blocks (OSC 133).
    pub const OUTPUT_BLOCKS_MAX: usize = super::shell::OUTPUT_BLOCKS_MAX;
    /// Maximum number of terminal marks (OSC 1337 SetMark).
    pub const TERMINAL_MARKS_MAX: usize = super::shell::TERMINAL_MARKS_MAX;
    /// Maximum number of annotations (OSC 1337 AddAnnotation).
    pub const ANNOTATIONS_MAX: usize = super::shell::ANNOTATIONS_MAX;
    /// Maximum number of semantic code blocks (OSC 1337 Block).
    pub const SEMANTIC_BLOCKS_MAX: usize = super::shell::SEMANTIC_BLOCKS_MAX;
    /// Maximum number of semantic buttons (OSC 1337 Button).
    pub const SEMANTIC_BUTTONS_MAX: usize = super::shell::SEMANTIC_BUTTONS_MAX;
    /// Maximum OSC 52 clipboard query response size (64 KiB).
    pub const MAX_OSC52_QUERY_RESPONSE_BYTES: usize = super::MAX_OSC52_QUERY_RESPONSE_BYTES;
    // Re-export types needed by extracted tests (from their origin crate).
    pub use aterm_types::osc::{SemanticBlockEvent, SemanticButtonEvent};
}

// Type alias for user_vars HashMap.
pub(crate) type UserVarsMap = std::collections::HashMap<String, String>;

// Pending notification map.
pub(crate) type PendingNotificationsMap = std::collections::HashMap<String, types::Notification>;

/// Maximum **decoded** clipboard bytes for an OSC 52 query response (Pd = "?").
///
/// This caps the size of **raw/decoded** data that aterm-core will accept from
/// the clipboard callback before base64 encoding and emitting as an OSC 52
/// response. Responses exceeding this limit are silently dropped to avoid
/// excessive memory use and accidental large exfil.
///
/// # Wire size note
///
/// The actual emitted response is larger due to:
/// - Base64 encoding (~33% expansion: 3 input bytes → 4 output bytes)
/// - OSC framing overhead (~12 bytes for `ESC ] 52 ; Pc ; ... BEL`)
///
/// A 64KB decoded payload becomes ~85KB on the wire. If wire-size constraints
/// are critical, hosts should apply their own caps before returning data from
/// the clipboard callback.
///
/// # Rationale
///
/// The 64KB limit matches the parser's `MAX_OSC_DATA` input limit, providing a
/// symmetric cap on clipboard data in both directions.
pub(crate) const MAX_OSC52_QUERY_RESPONSE_BYTES: usize = 64 * 1024;

/// Maximum bytes captured by OSC 1337 `CopyToClipboard` text capture mode.
///
/// While capture mode is active, every printed character is appended until
/// `EndCopy` is received. This cap prevents unbounded growth when peers never
/// send `EndCopy` (accidental or malicious).
pub(crate) const MAX_COPY_TO_CLIPBOARD_CAPTURE_BYTES: usize = 10 * 1024 * 1024;

/// Maximum bytes allowed in the terminal response buffer.
///
/// Responses from query sequences (for example DSR/DA/DECRQSS/OSC queries) are
/// buffered until the host drains them via `take_response()`. This cap prevents
/// unbounded memory growth when a single input batch generates excessive
/// responses before the host has a chance to read.
pub(crate) const MAX_RESPONSE_BUFFER_SIZE: usize = 1024 * 1024;

/// Maximum title/icon-name length in bytes for OSC 0/1/2 and the public
/// `set_title`/`set_icon_name` API. Matches OSC 777's cap in
/// `handler_osc_notify.rs`. The parser's OSC buffer allows up to 65534
/// bytes, but real terminal titles are short strings.
const MAX_TITLE_BYTES: usize = 1024;

/// Maximum hyperlink URL length in bytes for OSC 8 sequences.
///
/// URLs exceeding this limit are silently ignored (consistent with other
/// terminals). 8 KiB is generous for any realistic URL while preventing
/// memory abuse from malicious sequences. Part of #7172.
const MAX_HYPERLINK_URL_BYTES: usize = 8192;

// ----------------------------------------------------------------------------
// Type-safe conversion helpers
// ----------------------------------------------------------------------------

/// Convert u16 SGR parameter to u8 color index.
/// SGR color parameters are in 0-255 range; values >255 saturate to 255.
#[inline]
pub(crate) fn sgr_color_u8(val: u16) -> u8 {
    val.try_into().unwrap_or(u8::MAX)
}

// Terminal struct, Debug impl, and root state accessors live in state.rs.
