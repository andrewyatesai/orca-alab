//! `orca-terminal` — headless terminal emulation for Orca.
//!
//! The native replacement for the `@xterm/headless`-based
//! `src/main/daemon/headless-emulator.ts`: maintains a server-side grid +
//! cursor and tracks the working directory via OSC-7, so terminal sessions
//! survive reconnect / SSH replay. Built on a vendored `vte` ANSI parser.

pub mod color_scheme_protocol;
pub mod headless;

pub use color_scheme_protocol::{
    mode_2031_sequence_for, resolve_terminal_color_scheme_mode, scan_mode_2031_sequences,
    Mode2031ScanResult, TerminalColorSchemeMode,
};
pub use headless::{
    Cell, CellAttrs, Color, HeadlessTerminal, MouseTracking, TerminalSnapshot, DEFAULT_SCROLLBACK,
};
