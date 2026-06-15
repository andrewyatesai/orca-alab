// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Containment mode enum — the 4 security levels.
//!
//! Matches TLA+ spec `tla/Containment.tla` mode encoding:
//! Master=3, User=2, Safety=1, Containment=0.
//! Higher value = more capability. Non-escalation means mode can
//! only decrease or stay the same.

use std::fmt;

/// The 4 containment modes, ordered by decreasing capability.
///
/// These describe the POLICY intent per mode. The OS-level ENFORCEMENT of the
/// Containment policy (a real network/filesystem sandbox) is the deferred
/// actuator (see [`crate::actuator`] / `ATERM_DESIGN` §5.6), not yet implemented;
/// today the policy is actuated as the spawn-seam capability gate plus rlimits.
///
/// - **Master**: Full trust — developer mode. All capabilities unrestricted.
/// - **User**: Normal usage — standard safeguards. Output shadow-scanned.
/// - **Safety**: Reduced capability — allowlisted operations only.
/// - **Containment**: Hostile agent — most restrictive POLICY: no network, no
///   fork, filtered I/O, no MCP, no plugins (policy data model; OS enforcement
///   of these is the deferred Seatbelt actuator).
///
/// Mode is set ONLY by the launcher (env var `ATERM_CONTAINMENT_MODE` or
/// CLI `--mode`). aterm cannot upgrade its own mode. Mode is immutable
/// after initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum ContainmentMode {
    /// Hostile agent — most restrictive policy (OS enforcement is deferred; see
    /// the enum-level doc and [`crate::actuator`]).
    Containment = 0,
    /// Reduced capability — allowlisted operations only.
    Safety = 1,
    /// Normal usage — standard safeguards.
    User = 2,
    /// Full trust — developer mode.
    Master = 3,
}

impl ContainmentMode {
    /// Numeric capability level (TLA+ encoding). Higher = more capability.
    #[must_use]
    pub const fn level(self) -> u8 {
        self as u8
    }

    /// Whether this mode has equal or greater capability than `other`.
    #[must_use]
    #[cfg_attr(not(kani), allow(dead_code))]
    pub(crate) const fn at_least(self, other: Self) -> bool {
        self as u8 >= other as u8
    }

    /// Whether this mode has strictly less capability than `other`.
    #[must_use]
    #[cfg_attr(not(kani), allow(dead_code))]
    pub(crate) const fn below(self, other: Self) -> bool {
        (self as u8) < (other as u8)
    }

    /// Parse from string (case-insensitive). Used for env var / CLI parsing.
    #[must_use]
    pub(crate) fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "master" => Some(Self::Master),
            "user" => Some(Self::User),
            "safety" => Some(Self::Safety),
            "containment" => Some(Self::Containment),
            _ => None,
        }
    }
}

impl fmt::Display for ContainmentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Master => write!(f, "Master"),
            Self::User => write!(f, "User"),
            Self::Safety => write!(f, "Safety"),
            Self::Containment => write!(f, "Containment"),
        }
    }
}

impl PartialOrd for ContainmentMode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ContainmentMode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.level().cmp(&other.level())
    }
}

/// Error when parsing a containment mode from a string.
#[derive(Debug, Clone, aterm_error::Error)]
#[error("invalid containment mode: {0:?} (expected: master, user, safety, containment)")]
pub struct ParseModeError(pub(crate) String);

impl ParseModeError {
    /// The rejected input string.
    #[must_use]
    pub fn input(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for ContainmentMode {
    type Err = ParseModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str_loose(s).ok_or_else(|| ParseModeError(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_encoding_matches_tla() {
        assert_eq!(ContainmentMode::Containment.level(), 0);
        assert_eq!(ContainmentMode::Safety.level(), 1);
        assert_eq!(ContainmentMode::User.level(), 2);
        assert_eq!(ContainmentMode::Master.level(), 3);
    }

    #[test]
    fn test_ordering() {
        assert!(ContainmentMode::Master > ContainmentMode::User);
        assert!(ContainmentMode::User > ContainmentMode::Safety);
        assert!(ContainmentMode::Safety > ContainmentMode::Containment);
    }

    #[test]
    fn test_at_least() {
        assert!(ContainmentMode::Master.at_least(ContainmentMode::Master));
        assert!(ContainmentMode::Master.at_least(ContainmentMode::Containment));
        assert!(!ContainmentMode::Containment.at_least(ContainmentMode::User));
    }

    #[test]
    fn test_below() {
        assert!(ContainmentMode::Containment.below(ContainmentMode::Safety));
        assert!(!ContainmentMode::Master.below(ContainmentMode::Master));
    }

    #[test]
    fn test_parse() {
        assert_eq!(
            "master".parse::<ContainmentMode>().unwrap(),
            ContainmentMode::Master
        );
        assert_eq!(
            "SAFETY".parse::<ContainmentMode>().unwrap(),
            ContainmentMode::Safety
        );
        assert_eq!(
            "User".parse::<ContainmentMode>().unwrap(),
            ContainmentMode::User
        );
        assert!("invalid".parse::<ContainmentMode>().is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(ContainmentMode::Master.to_string(), "Master");
        assert_eq!(ContainmentMode::Containment.to_string(), "Containment");
    }

    /// Verify parse rejects inputs that could bypass mode selection.
    /// An attacker controlling ATERM_CONTAINMENT_MODE might try numeric
    /// values, padding, or similar to escalate.
    #[test]
    fn test_parse_rejects_bypass_attempts() {
        // Numeric values (TLA+ encoding) must not be accepted
        assert!("0".parse::<ContainmentMode>().is_err());
        assert!("1".parse::<ContainmentMode>().is_err());
        assert!("2".parse::<ContainmentMode>().is_err());
        assert!("3".parse::<ContainmentMode>().is_err());

        // Padding / whitespace
        assert!(" master".parse::<ContainmentMode>().is_err());
        assert!("master ".parse::<ContainmentMode>().is_err());
        assert!("master\n".parse::<ContainmentMode>().is_err());

        // Substring / prefix
        assert!("mast".parse::<ContainmentMode>().is_err());
        assert!("contain".parse::<ContainmentMode>().is_err());

        // Empty
        assert!("".parse::<ContainmentMode>().is_err());

        // Null byte injection
        assert!("master\0".parse::<ContainmentMode>().is_err());
    }

    /// Verify that all 4 display strings round-trip through parse.
    #[test]
    fn test_display_parse_roundtrip() {
        for mode in [
            ContainmentMode::Master,
            ContainmentMode::User,
            ContainmentMode::Safety,
            ContainmentMode::Containment,
        ] {
            let s = mode.to_string();
            let parsed: ContainmentMode = s.parse().unwrap();
            assert_eq!(parsed, mode, "roundtrip failed for {mode}");
        }
    }

    /// Verify ParseModeError includes the rejected input for diagnostics.
    #[test]
    fn test_parse_error_includes_input() {
        let err = "bogus".parse::<ContainmentMode>().unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bogus"),
            "error message should include rejected input, got: {msg}"
        );
    }
}
