// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

#![deny(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Shell integration injection for aterm.
//!
//! Embeds shell integration scripts (zsh, bash, fish) in the Rust binary
//! and provides a cross-platform injection mechanism that auto-loads them
//! at shell startup without requiring user configuration.
//!
//! # Injection Strategies
//!
//! Each shell has its own auto-loading mechanism:
//!
//! | Shell | Mechanism | How |
//! |-------|-----------|-----|
//! | zsh   | ZDOTDIR override | Wrapper `.zshenv` sources user config then ours |
//! | bash  | `--rcfile` | Wrapper rcfile sources profiles then ours |
//! | fish  | `XDG_DATA_DIRS` | Vendor conf.d auto-loading |
//!
//! # Usage
//!
//! ```rust,no_run
//! use aterm_shell_integration::{ShellType, prepare};
//!
//! let shell = ShellType::detect("/bin/zsh");
//! if let Ok(Some(injection)) = prepare(shell) {
//!     // Add injection.env_add to SpawnConfig.env before fork
//!     // Use injection.argv_override if Some (bash --rcfile)
//! }
//! ```

use std::path::{Path, PathBuf};

/// Embedded shell integration scripts (compiled into the binary).
///
/// `aterm-core` is the canonical owner of the shell script bodies. The macOS
/// app bundle ships byte-identical copies in
/// `apps/aterm-mac/Sources/ATermMac/Resources/ShellIntegration/`, and the
/// shell-integration test module enforces that parity so cross-consumer drift
/// fails in Rust tests instead of shipping silently.
pub mod scripts {
    /// zsh shell integration (OSC 7/133 + prompt override).
    pub const ZSH: &str = include_str!("scripts/aterm_shell_integration.zsh");
    /// bash shell integration (OSC 7/133 + prompt override).
    pub const BASH: &str = include_str!("scripts/aterm_shell_integration.bash");
    /// fish shell integration (OSC 7/133 + prompt override).
    pub const FISH: &str = include_str!("scripts/aterm_shell_integration.fish");
}

/// Shell type detected from the command path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShellType {
    /// Zsh (injected via ZDOTDIR override).
    Zsh,
    /// Bash (injected via --rcfile wrapper).
    Bash,
    /// Fish (injected via XDG_DATA_DIRS vendor conf.d).
    Fish,
    /// Unknown shell (no injection available).
    Unknown,
}

impl ShellType {
    /// Detect shell type from a command path (e.g. "/bin/zsh", "bash").
    #[must_use]
    pub fn detect(shell_path: &str) -> Self {
        let name = Path::new(shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        match name {
            "zsh" => Self::Zsh,
            "bash" | "bash5" => Self::Bash,
            "fish" => Self::Fish,
            _ => Self::Unknown,
        }
    }

    /// Detect from the current `$SHELL` environment variable.
    #[must_use]
    pub fn detect_current() -> Self {
        match std::env::var("SHELL") {
            Ok(shell) => Self::detect(&shell),
            Err(_) => Self::Unknown,
        }
    }
}

/// Result of preparing shell integration injection.
///
/// Contains environment variable modifications to apply to the child
/// process before exec.
#[derive(Debug)]
pub struct InjectionEnv {
    /// Environment variables to set in the child process.
    pub env_add: Vec<(String, String)>,
    /// For bash: override argv to use `--rcfile`. `None` for other shells.
    pub argv_override: Option<Vec<String>>,
}

/// Byte length of the shell-integration capability-nonce (#7960).
pub const SHELL_NONCE_BYTES: usize = 32;

/// Hex-encoded length of the shell-integration nonce (#7960).
pub const SHELL_NONCE_HEX_LEN: usize = SHELL_NONCE_BYTES * 2;

/// A freshly generated 32-byte CSPRNG nonce for OSC 133/633 gating (#7960, #7987).
///
/// Produced by [`generate_nonce`]. Carries both the raw bytes (for
/// `Terminal::authorize_shell_integration` in `aterm-core`) and the hex
/// encoding (for the `ATERM_SHELL_NONCE` child env var).
#[derive(Debug, Clone)]
pub struct ShellNonce {
    raw: [u8; SHELL_NONCE_BYTES],
    hex: String,
}

impl ShellNonce {
    /// Raw 32-byte nonce to pass to `Terminal::authorize_shell_integration`.
    #[must_use]
    pub const fn raw(&self) -> &[u8; SHELL_NONCE_BYTES] {
        &self.raw
    }

    /// 64-char lowercase hex encoding to set as `ATERM_SHELL_NONCE` in the
    /// child shell environment.
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }

    /// Consume the nonce and return both halves. Callers typically use
    /// [`raw`](Self::raw) to authorize the terminal, then [`hex`](Self::hex)
    /// to inject into the child environment.
    #[must_use]
    pub fn into_parts(self) -> ([u8; SHELL_NONCE_BYTES], String) {
        (self.raw, self.hex)
    }
}

/// Generate a fresh 32-byte shell-integration capability-nonce (#7960, #7987).
///
/// Uses [`rand_core::OsRng`] — the operating-system CSPRNG — so the nonce
/// is unpredictable across restarts. The host is responsible for:
///
/// 1. Installing the raw bytes via `Terminal::authorize_shell_integration`.
/// 2. Setting `ATERM_SHELL_NONCE=<hex>` in the spawned shell's environment
///    (see [`augment_with_nonce`]).
/// 3. Flipping `TerminalModes::require_shell_integration_nonce` on after
///    (1) and (2) are wired.
#[must_use]
pub fn generate_nonce() -> ShellNonce {
    use rand_core::{OsRng, RngCore};
    let mut raw = [0u8; SHELL_NONCE_BYTES];
    // OsRng::fill_bytes is documented infallible — `getrandom` retries
    // EINTR internally and panics on unrecoverable OS errors, which we
    // prefer to a silent fallback to a weaker RNG.
    OsRng.fill_bytes(&mut raw);
    let hex = hex_encode(&raw);
    ShellNonce { raw, hex }
}

/// Lowercase hex-encode a 32-byte nonce. Exposed for host-side helpers
/// that wire a caller-provided nonce (e.g. test fixtures that want
/// deterministic bytes).
#[must_use]
pub fn hex_encode(bytes: &[u8; SHELL_NONCE_BYTES]) -> String {
    let mut out = String::with_capacity(SHELL_NONCE_HEX_LEN);
    for b in bytes {
        out.push(nibble_to_hex(b >> 4));
        out.push(nibble_to_hex(b & 0x0F));
    }
    out
}

const fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '0', // unreachable: caller masks to 4 bits
    }
}

/// Append `ATERM_SHELL_NONCE=<hex>` to an [`InjectionEnv`]'s env list.
///
/// Idempotent with respect to the `ATERM_SHELL_NONCE` key — a prior entry
/// for that key is removed before the new one is appended. Other entries
/// are preserved in order.
pub fn augment_with_nonce(injection: &mut InjectionEnv, hex: &str) {
    injection.env_add.retain(|(k, _)| k != "ATERM_SHELL_NONCE");
    injection
        .env_add
        .push(("ATERM_SHELL_NONCE".to_string(), hex.to_string()));
}

/// Prepare shell integration for the given shell type.
///
/// Writes embedded scripts to a cache directory and returns the
/// environment modifications needed to auto-load them at shell startup.
///
/// Returns `None` for unknown shell types.
pub fn prepare(shell: ShellType) -> Result<Option<InjectionEnv>, std::io::Error> {
    let base = cache_dir();
    prepare_into(shell, &base)
}

/// Prepare shell integration using a specific base directory.
///
/// Exposed for testing and for callers that want to control the cache location.
pub fn prepare_into(shell: ShellType, base: &Path) -> Result<Option<InjectionEnv>, std::io::Error> {
    ensure_scripts(base)?;

    match shell {
        ShellType::Zsh => Ok(Some(prepare_zsh(base))),
        ShellType::Bash => Ok(Some(prepare_bash(base))),
        ShellType::Fish => Ok(Some(prepare_fish(base))),
        ShellType::Unknown => Ok(None),
    }
}

/// Cache directory for shell integration files.
///
/// Follows XDG Base Directory Specification:
/// `$XDG_CACHE_HOME/aterm/shell-integration/` (default: `~/.cache/aterm/shell-integration/`).
///
/// In restricted containment modes (Containment/Safety), writes go to
/// `/tmp/aterm-shell-integration` to comply with `FsCapability::TmpOnly`
/// and `FsCapability::ProjectRW` policies. Part of #5575.
fn cache_dir() -> PathBuf {
    // In restricted containment modes, use /tmp to comply with FS policy (#5575).
    #[cfg(feature = "local-pty")]
    {
        use aterm_containment::{ContainmentPolicy, FsCapability, mode_or_containment};
        let caps = ContainmentPolicy::capabilities(mode_or_containment());
        if caps.fs <= FsCapability::ProjectReadWrite {
            return PathBuf::from("/tmp/aterm-shell-integration");
        }
    }

    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(cache).join("aterm").join("shell-integration")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".cache")
            .join("aterm")
            .join("shell-integration")
    } else {
        PathBuf::from("/tmp/aterm-shell-integration")
    }
}

/// Write embedded scripts and wrapper files to the cache directory.
fn ensure_scripts(base: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(base)?;

    // Write canonical scripts
    std::fs::write(base.join("aterm_shell_integration.zsh"), scripts::ZSH)?;
    std::fs::write(base.join("aterm_shell_integration.bash"), scripts::BASH)?;
    std::fs::write(base.join("aterm_shell_integration.fish"), scripts::FISH)?;

    // zsh: ZDOTDIR wrapper .zshenv
    let zdotdir = base.join("zdotdir");
    std::fs::create_dir_all(&zdotdir)?;
    std::fs::write(zdotdir.join(".zshenv"), ZSH_WRAPPER)?;

    // bash: rcfile wrapper
    let bash_dir = base.join("bash");
    std::fs::create_dir_all(&bash_dir)?;
    std::fs::write(bash_dir.join("rcfile"), BASH_WRAPPER)?;

    // fish: XDG vendor conf.d structure
    let fish_conf = base.join("fish-xdg").join("fish").join("vendor_conf.d");
    std::fs::create_dir_all(&fish_conf)?;
    std::fs::write(
        fish_conf.join("aterm_shell_integration.fish"),
        scripts::FISH,
    )?;

    Ok(())
}

/// zsh wrapper .zshenv that restores ZDOTDIR and sources our integration.
///
/// The wrapper reads `ATERM_ORIGINAL_ZDOTDIR` (set by [`prepare_zsh`]) to
/// restore the user's original ZDOTDIR before sourcing their `.zshenv`.
/// This is the same ZDOTDIR-override technique used by Kitty, Ghostty,
/// and VS Code terminal integrations.
const ZSH_WRAPPER: &str = "\
# aterm shell integration loader
# Restore original ZDOTDIR before sourcing user config
if [ -n \"$ATERM_ORIGINAL_ZDOTDIR\" ]; then
  ZDOTDIR=\"$ATERM_ORIGINAL_ZDOTDIR\"
  unset ATERM_ORIGINAL_ZDOTDIR
elif [ -n \"$ATERM_UNSET_ZDOTDIR\" ]; then
  unset ZDOTDIR
  unset ATERM_UNSET_ZDOTDIR
fi
# Source user's .zshenv
[ -f \"${ZDOTDIR:-$HOME}/.zshenv\" ] && source \"${ZDOTDIR:-$HOME}/.zshenv\"
# Load aterm integration
source \"$ATERM_SHELL_INTEGRATION_DIR/aterm_shell_integration.zsh\"
";

/// bash wrapper rcfile that sources standard profile chain then our integration.
///
/// `bash --rcfile` launches an interactive non-login shell which normally reads
/// only `.bashrc`. We source the login profile chain (since terminal sessions
/// conventionally behave like login shells) AND `.bashrc` (since many users keep
/// aliases/functions/PATH additions there separately from `.bash_profile`).
/// `.bashrc` is sourced last before integration to handle the common case where
/// `.bash_profile` does NOT source `.bashrc`.
const BASH_WRAPPER: &str = "\
# aterm shell integration loader
# Source standard profile chain (login-style)
[ -f /etc/profile ] && . /etc/profile
if [ -f \"$HOME/.bash_profile\" ]; then
  . \"$HOME/.bash_profile\"
elif [ -f \"$HOME/.bash_login\" ]; then
  . \"$HOME/.bash_login\"
elif [ -f \"$HOME/.profile\" ]; then
  . \"$HOME/.profile\"
fi
# Source .bashrc (--rcfile skips it; .bash_profile may or may not source it)
[ -f \"$HOME/.bashrc\" ] && . \"$HOME/.bashrc\"
# Load aterm integration
. \"$ATERM_SHELL_INTEGRATION_DIR/aterm_shell_integration.bash\"
";

fn prepare_zsh(base: &Path) -> InjectionEnv {
    let zdotdir = base.join("zdotdir");
    let mut env_add = vec![
        (
            "ATERM_SHELL_INTEGRATION_DIR".to_string(),
            base.to_string_lossy().into_owned(),
        ),
        (
            "ZDOTDIR".to_string(),
            zdotdir.to_string_lossy().into_owned(),
        ),
    ];

    // Preserve original ZDOTDIR so the wrapper can restore it.
    // Treat empty ZDOTDIR the same as unset to avoid infinite recursion:
    // the wrapper checks `[ -n "$ATERM_ORIGINAL_ZDOTDIR" ]`, which is false
    // for empty strings, leaving ZDOTDIR pointing at our wrapper dir.
    match std::env::var("ZDOTDIR") {
        Ok(original) if !original.is_empty() => {
            env_add.push(("ATERM_ORIGINAL_ZDOTDIR".to_string(), original));
        }
        _ => {
            env_add.push(("ATERM_UNSET_ZDOTDIR".to_string(), "1".to_string()));
        }
    }

    InjectionEnv {
        env_add,
        argv_override: None,
    }
}

fn prepare_bash(base: &Path) -> InjectionEnv {
    let rcfile = base.join("bash").join("rcfile");
    InjectionEnv {
        env_add: vec![(
            "ATERM_SHELL_INTEGRATION_DIR".to_string(),
            base.to_string_lossy().into_owned(),
        )],
        argv_override: Some(vec![
            "bash".to_string(),
            "--rcfile".to_string(),
            rcfile.to_string_lossy().into_owned(),
        ]),
    }
}

fn prepare_fish(base: &Path) -> InjectionEnv {
    let fish_xdg = base.join("fish-xdg");
    let mut xdg_data = fish_xdg.to_string_lossy().into_owned();

    // Prepend to existing XDG_DATA_DIRS so fish's vendor conf.d finds our script.
    // When XDG_DATA_DIRS is unset, fall back to the XDG spec default
    // (/usr/local/share:/usr/share) so third-party vendor conf.d scripts
    // (fzf, conda, etc.) continue loading.
    let existing = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    xdg_data.push(':');
    xdg_data.push_str(&existing);

    InjectionEnv {
        env_add: vec![
            (
                "ATERM_SHELL_INTEGRATION_DIR".to_string(),
                base.to_string_lossy().into_owned(),
            ),
            ("XDG_DATA_DIRS".to_string(), xdg_data),
        ],
        argv_override: None,
    }
}

#[cfg(test)]
mod tests {
    include!("tests.rs");

    /// Regression test for #5959/#5960: `autoload -Uz add-zsh-hook` must
    /// appear before any `add-zsh-hook` call in the zsh script. Violating
    /// this ordering causes zsh to exit immediately when ATERM_PROMPT_STYLE
    /// is set to a non-"none" value.
    #[test]
    fn test_zsh_autoload_before_hook_usage() {
        let script = scripts::ZSH;
        let autoload_pos = script
            .find("autoload -Uz add-zsh-hook")
            .expect("zsh script must contain 'autoload -Uz add-zsh-hook'");

        // Every `add-zsh-hook` call (outside comments) must come after autoload.
        for (i, line) in script.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            if trimmed.contains("add-zsh-hook") && !trimmed.contains("autoload") {
                let byte_offset: usize = script.lines().take(i).map(|l| l.len() + 1).sum();
                assert!(
                    byte_offset > autoload_pos,
                    "line {}: `add-zsh-hook` call appears before \
                     `autoload -Uz add-zsh-hook` — this will crash zsh \
                     when ATERM_PROMPT_STYLE is set. Line: {trimmed}",
                    i + 1,
                );
            }
        }
    }

    /// The zsh script must define `__aterm_precmd` and `__aterm_preexec`
    /// before installing them as hooks.
    #[test]
    fn test_zsh_functions_defined_before_hooks() {
        let script = scripts::ZSH;
        let precmd_def = script
            .find("__aterm_precmd()")
            .expect("must define __aterm_precmd()");
        let preexec_def = script
            .find("__aterm_preexec()")
            .expect("must define __aterm_preexec()");

        let hook_precmd = script
            .find("add-zsh-hook precmd __aterm_precmd")
            .expect("must install precmd hook");
        let hook_preexec = script
            .find("add-zsh-hook preexec __aterm_preexec")
            .expect("must install preexec hook");

        assert!(
            precmd_def < hook_precmd,
            "__aterm_precmd() must be defined before add-zsh-hook installs it"
        );
        assert!(
            preexec_def < hook_preexec,
            "__aterm_preexec() must be defined before add-zsh-hook installs it"
        );
    }

    /// The ATERM_PROMPT_STYLE conditional block must come after autoload.
    /// This is the specific regression from #5959.
    #[test]
    fn test_zsh_prompt_style_block_after_autoload() {
        let script = scripts::ZSH;
        let autoload_pos = script
            .find("autoload -Uz add-zsh-hook")
            .expect("must have autoload");
        let conditional = script
            .find(r#"if [[ -n "$ATERM_PROMPT_STYLE""#)
            .expect("must have ATERM_PROMPT_STYLE conditional block");

        assert!(
            conditional > autoload_pos,
            "ATERM_PROMPT_STYLE conditional (which calls add-zsh-hook) must \
             come after autoload -Uz add-zsh-hook. Bug: #5959"
        );
    }
}
