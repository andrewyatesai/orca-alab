//! `orca-ssh` — SSH remote-runtime support for Orca.
//!
//! This first cut is the pure OpenSSH **config parsing** ported from
//! `src/main/ssh/ssh-config-parser.ts`. The transport (a vendored SSH crate
//! behind a `Connection` boundary, like `orca-git`'s `GitRunner`) is added on
//! top; the parsing here is what populates connection targets either way.

pub mod config_parser;

pub use config_parser::{parse_ssh_config, SshConfigHost};
