// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Remote host information from OSC 1337.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Remote host information from OSC 1337 RemoteHost.
///
/// Tracks the current SSH session host as reported by shells via the
/// OSC 1337 RemoteHost=user@hostname sequence.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHost {
    /// Username on the remote host.
    pub user: String,
    /// Fully-qualified hostname.
    pub hostname: String,
}

impl RemoteHost {
    /// Parse "user@hostname" format.
    ///
    /// Returns `None` if the format is invalid:
    /// - Missing `@` symbol
    /// - Empty user (starts with `@`)
    /// - Empty hostname (ends with `@`)
    ///
    /// If multiple `@` symbols are present, the first one is used as the
    /// delimiter (e.g., "user@host@domain" -> user="user", hostname="host@domain").
    pub fn parse(value: &str) -> Option<Self> {
        let at_pos = value.find('@')?;
        if at_pos == 0 || at_pos == value.len() - 1 {
            return None;
        }
        Some(Self {
            user: value[..at_pos].to_string(),
            hostname: value[at_pos + 1..].to_string(),
        })
    }
}

impl std::fmt::Display for RemoteHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.user, self.hostname)
    }
}
