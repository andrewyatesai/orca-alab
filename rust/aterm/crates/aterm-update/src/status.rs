// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! The operator-readable status record (`…/aterm/Updates/status.toml`).
//!
//! A silent updater has no UI, so this file IS the observability surface: it
//! records when the updater last ran, what it decided, and whether a build is
//! staged. An operator can `cat` it to answer "is this machine receiving updates,
//! and why didn't the last one apply?" without any in-app prompt. Diagnostics also
//! go to the app log via [`crate::log`]/[`crate::warn`]; this file is the durable,
//! at-a-glance summary.

use serde::Serialize;

use crate::paths::Staging;

/// Snapshot written after each check / apply decision.
#[derive(Serialize)]
struct Status<'a> {
    schema: u32,
    /// RFC3339 UTC time this record was written.
    updated_at: String,
    /// Whether the updater is configured to act (pinned Team ID + not opted out).
    enabled: bool,
    /// The running build number.
    current_build: u64,
    /// Build number currently staged for next-launch apply, if any.
    staged_build: Option<u64>,
    /// Last decision, e.g. "up to date", "staged 0.3.0 (build N)", "idle: no
    /// token", "deferred: install location not writable".
    outcome: &'a str,
}

/// Atomically write the status record (temp + rename). Best-effort: failures are
/// silent — status is diagnostics, never load-bearing.
pub fn record(staging: &Staging, current_build: u64, outcome: &str) {
    let staged_build = crate::manifest::Ready::read(&staging.ready).map(|r| r.build_number);
    let status = Status {
        schema: 1,
        updated_at: crate::install::now_rfc3339(),
        enabled: crate::enabled(),
        current_build,
        staged_build,
        outcome,
    };
    let Ok(text) = toml::to_string(&status) else {
        return;
    };
    let tmp = staging.root.join("status.toml.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &staging.status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_writes_a_parseable_status_file() {
        let root = std::env::temp_dir().join(format!("aterm-st-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let s = Staging {
            apply_lock: root.join("apply.lock"),
            stage_lock: root.join("stage.lock"),
            download: root.join("download"),
            staged_app: root.join("staged").join("aterm.app"),
            ready: root.join("ready.toml"),
            status: root.join("status.toml"),
            root: root.clone(),
        };
        record(&s, 42, "up to date (test)");
        let text = std::fs::read_to_string(&s.status).expect("status file written");
        assert!(text.contains("current_build = 42"), "got: {text}");
        assert!(text.contains("up to date (test)"), "got: {text}");
        // It must be valid TOML.
        let _: toml::Value = toml::from_str(&text).expect("status is valid TOML");
        let _ = std::fs::remove_dir_all(&root);
    }
}
