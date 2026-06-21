// Copyright 2026 The aterm Authors, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Platform directory resolution (zero external dependencies).
//!
//! Replaces the `dirs` crate with direct environment variable lookups
//! and platform-specific conventions.

use std::path::PathBuf;

/// Return the user's home directory.
///
/// - **Unix/macOS**: `$HOME`, falling back to `/etc/passwd` lookup
/// - **Windows**: `%USERPROFILE%`
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| passwd_home_dir(current_uid()))
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid() is always safe — no failure mode, no args.
    unsafe { libc_getuid() }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

/// Parse `/etc/passwd` to find the home directory for a given UID.
#[cfg(unix)]
fn passwd_home_dir(uid: u32) -> Option<PathBuf> {
    let contents = std::fs::read_to_string("/etc/passwd").ok()?;
    home_from_passwd(&contents, uid)
}

/// Testable helper: extract home dir for `uid` from passwd-format text.
#[cfg(unix)]
fn home_from_passwd(contents: &str, uid: u32) -> Option<PathBuf> {
    let uid_str = uid.to_string();
    for line in contents.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 6 && fields[2] == uid_str {
            let home = fields[5];
            if !home.is_empty() {
                return Some(PathBuf::from(home));
            }
        }
    }
    None
}

/// Return the user's configuration directory.
///
/// - **macOS**: `$HOME/Library/Application Support`
/// - **Linux**: `$XDG_CONFIG_HOME` or `$HOME/.config`
/// - **Windows**: `%APPDATA%`
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library/Application Support"))
    }
    #[cfg(target_os = "linux")]
    {
        xdg_dir("XDG_CONFIG_HOME").or_else(|| home_dir().map(|h| h.join(".config")))
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
}

/// Return the user's data directory.
///
/// - **macOS**: `$HOME/Library/Application Support`
/// - **Linux**: `$XDG_DATA_HOME` or `$HOME/.local/share`
/// - **Windows**: `%LOCALAPPDATA%`
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        home_dir().map(|h| h.join("Library/Application Support"))
    }
    #[cfg(target_os = "linux")]
    {
        xdg_dir("XDG_DATA_HOME").or_else(|| home_dir().map(|h| h.join(".local/share")))
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
}

/// Read an XDG env var, returning `None` if unset or not an absolute path.
#[cfg(target_os = "linux")]
fn xdg_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_home_dir_returns_some() {
        // HOME should be set in any reasonable test environment
        assert!(home_dir().is_some());
    }

    #[test]
    fn test_config_dir_returns_some() {
        assert!(config_dir().is_some());
    }

    #[test]
    fn test_data_dir_returns_some() {
        assert!(data_dir().is_some());
    }

    #[test]
    fn test_home_dir_is_absolute() {
        if let Some(home) = home_dir() {
            assert!(home.is_absolute(), "home_dir should be absolute: {home:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_passwd_parsing_finds_uid() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n\
                      testuser:x:1000:1000:Test User:/home/testuser:/bin/zsh\n";
        assert_eq!(home_from_passwd(passwd, 0), Some(PathBuf::from("/root")));
        assert_eq!(
            home_from_passwd(passwd, 1000),
            Some(PathBuf::from("/home/testuser"))
        );
        assert_eq!(home_from_passwd(passwd, 9999), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_passwd_parsing_empty_home_returns_none() {
        let passwd = ["broken:", "x", ":500:500:Broken User::/bin/sh\n"].concat();
        assert_eq!(home_from_passwd(&passwd, 500), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_passwd_parsing_malformed_lines_skipped() {
        let passwd = "short:x\n\
                      valid:x:42:42:User:/home/valid:/bin/sh\n\
                      \n";
        assert_eq!(
            home_from_passwd(passwd, 42),
            Some(PathBuf::from("/home/valid"))
        );
    }
}
