// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! macOS Seatbelt (SBPL) profile generation for the OS-sandbox actuator.
//!
//! This is the REAL, ENFORCING OS-level sandbox the [`crate::actuator`] doc once
//! deferred. Given a resolved containment [`Capabilities`](crate::Capabilities) set
//! whose [`NetworkCapability`](crate::NetworkCapability) is
//! [`None`](crate::NetworkCapability::None) (i.e. `Containment` mode), it returns an
//! SBPL string that the spawn seam hands to `/usr/bin/sandbox-exec -p <sbpl>` so the
//! kernel Seatbelt enforces it on the child shell and everything it runs.
//!
//! ## Honest scope (`ATERM_DESIGN` §0.1 / §5.6) — what this profile DOES and DOES NOT do
//!
//! The profile is `(version 1)(allow default)(deny network*)` PLUS a conservative
//! deny of a small, fixed set of SECRET directories under `$HOME`:
//!
//! - **`(allow default)`** — start permissive so a normal interactive shell keeps
//!   working (it reads/writes files, forks, signals, sources the user's rc files,
//!   opens `/dev/tty`, etc.). A blanket `(deny file-*)` base tight enough to be
//!   meaningful also breaks `$SHELL` (dyld, `path_helper`, the user's rc files,
//!   `/dev/tty`, …), so we do NOT do that. General per-[`FsCapability`](crate::FsCapability)
//!   filesystem scoping is an explicit FOLLOW-UP, not silently implied here.
//! - **`(deny network*)`** — the clean, high-value, shell-safe denial: it removes
//!   ALL socket/network access (outbound connect, inbound bind, every domain)
//!   while leaving the local shell fully functional. A hostile agent must not be
//!   able to exfiltrate or call home, and this is verified to enforce on macOS
//!   (see the `actuator` enforcement-proof test).
//! - **`(deny file-read* file-write* …)` over the SECRET SET** — a CONSERVATIVE,
//!   targeted denial of the user's credential stores so an untrusted Containment
//!   shell cannot read or tamper with them, while the rest of `$HOME` (including
//!   `~/.zshrc`, `~/.bash_profile`, the user's normal files) stays fully readable
//!   and writable. The set is small and fixed (see [`SECRET_SUBDIRS`] /
//!   [`SECRET_LITERAL_FILES`]); each is denied as a `subpath`/`literal`, NOT as a
//!   blanket `~` deny, so it scopes the credentials WITHOUT breaking the shell.
//!
//! So for a `Containment` spawn: **network is enforced-denied AND a conservative
//! secret-directory set is enforced-denied (read+write) by the OS; the rest of the
//! filesystem is NOT scoped** (the larger per-capability follow-up). The actuator's
//! `os_sandbox_actuated()`, `network_sandbox_actuated()` and audit log say exactly
//! this — never more.
//!
//! ## Canonicalization (the `/tmp` → `/private/tmp` footgun)
//!
//! Seatbelt matches the CANONICAL path. On macOS `/tmp` is a symlink to
//! `/private/tmp`, and `$HOME` itself can be reached via a symlinked prefix. A
//! non-canonical literal/subpath would simply NOT match the real path and the deny
//! would silently do nothing (security theater). So [`profile_for_home`]
//! canonicalizes `$HOME` once (via [`std::fs::canonicalize`]) and joins the secret
//! components onto the CANONICAL home. The secret dirs themselves need not exist
//! (canonicalizing a non-existent `~/.gnupg` would fail), but the deny is still
//! emitted for them — Seatbelt matches by path, so a deny that pre-exists the
//! directory still fires the moment it is created.
//!
//! ## Per-user / dynamic profile (no longer a static const)
//!
//! Because the secret paths depend on `$HOME`, the Containment profile is now
//! per-user and built at profile-generation time; it is therefore an owned
//! `String`, not a `&'static str`. The network-only fallback
//! ([`NETWORK_DENY_PROFILE`]) is still a const, used verbatim when `$HOME` is unset
//! or empty (we refuse to emit denies for bogus paths).
//!
//! ## Why a pure string generator (no `sandbox_init`)
//!
//! The profile is applied by `exec`ing `/usr/bin/sandbox-exec -p <profile>` rather
//! than calling `sandbox_init(3)` in the post-`fork` child. aterm's spawn child is
//! async-signal-only (the frontend is multi-threaded; only async-signal-safe calls
//! are legal between `fork` and `exec` — see `aterm-pty`). `sandbox_init` is NOT
//! async-signal-safe (it allocates, parses, talks to the sandbox daemon), so
//! calling it in that child is unsound. `sandbox-exec` does that work in a FRESH
//! process image before it `exec`s the real target, which is the supported,
//! async-signal-safe path. This module's only job is to produce the profile
//! STRING; it resolves/canonicalizes `$HOME` in the PARENT (at generation time),
//! performs no post-fork syscalls, and is trivially testable.

use crate::capability::NetworkCapability;
use crate::policy::Capabilities;
use std::path::Path;

/// Absolute path to the macOS Seatbelt wrapper. A fixed absolute path (never a
/// PATH search) so the spawn seam can resolve it in the parent and the child stays
/// async-signal-safe, and so a missing wrapper is detectable (fail-closed) rather
/// than silently resolved to something else.
pub const SANDBOX_EXEC_PATH: &str = "/usr/bin/sandbox-exec";

/// The network-only SBPL profile — the fallback used when `$HOME` is unset/empty.
///
/// `(allow default)` keeps the shell working; `(deny network*)` removes all
/// network access. This is the exact profile emitted when there is no resolvable
/// home to scope secrets under: we deny network (always safe and shell-compatible)
/// but emit NO file denies rather than deny bogus paths. The full Containment
/// profile ([`profile_for_home`]) is this PLUS the secret-set file denies.
pub const NETWORK_DENY_PROFILE: &str = "(version 1)(allow default)(deny network*)";

/// CONSERVATIVE secret-directory set, denied (read+write) as `subpath`s under the
/// canonical `$HOME`. Each entry is a credential/secret store an untrusted agent
/// shell has no business reading or tampering with. Kept SMALL and fixed on
/// purpose: every entry must be both (a) genuinely secret and (b) safe to deny
/// without breaking a normal `$SHELL` (none of these are sourced by the shell at
/// startup, unlike `~/.zshrc`).
///
/// Each path component is joined onto the canonical home, so e.g. `.config/gh`
/// becomes `<canonical-home>/.config/gh` (denying gh's token WITHOUT denying the
/// rest of `~/.config`).
pub const SECRET_SUBDIRS: &[&str] = &[
    ".ssh",          // SSH private keys / known_hosts / config
    ".aws",          // AWS credentials + config
    ".gnupg",        // GnuPG keyrings
    ".config/gh",    // GitHub CLI OAuth token
    ".config/aterm", // aterm's own config/secrets
    ".kube",         // Kubernetes kubeconfig (cluster creds/tokens)
    ".docker",       // Docker registry credentials (config.json)
    ".config/gcloud", // Google Cloud SDK credentials/tokens
    ".azure",        // Azure CLI tokens (accessTokens.json)
];

/// CONSERVATIVE secret FILE set, denied (read+write) as `literal`s under the
/// canonical `$HOME`. Unlike [`SECRET_SUBDIRS`] these are single files, not
/// directories, so they are emitted as `(literal …)` not `(subpath …)`.
pub const SECRET_LITERAL_FILES: &[&str] = &[
    ".netrc",           // FTP/curl/git credentials
    ".git-credentials", // git credential-store plaintext tokens
    ".npmrc",           // npm registry auth token (_authToken)
    ".pypirc",          // PyPI upload credentials
];

/// Generate the SBPL profile string for the given resolved capability set, or
/// `None` if no OS sandbox is required for it.
///
/// Returns `Some(profile)` exactly when the capability set DENIES network
/// ([`NetworkCapability::None`](crate::NetworkCapability::None)) — i.e. for
/// `Containment` mode. The returned profile is the network deny PLUS the
/// conservative secret-set file deny, scoped under the canonical `$HOME` (see
/// [`profile_for_home`]); if `$HOME` is unset/empty it is exactly the
/// network-only [`NETWORK_DENY_PROFILE`]. For every other capability set (network
/// `Allowlist` or `Full` — `Safety`/`User`/`Master`) it returns `None`, meaning
/// "no `sandbox-exec` wrap; spawn exactly as before". This is the load-bearing
/// safety property: the OS sandbox is applied ONLY when the policy denies network,
/// never otherwise, so the default User-mode spawn is byte-identical.
///
/// `$HOME` is read from the process environment here (parent side, at generation
/// time). The profile is therefore per-user and an owned `String`.
#[must_use]
pub fn profile_for(caps: &Capabilities) -> Option<String> {
    match caps.network {
        // Containment: network fully denied → the network-deny + secret-deny
        // Seatbelt profile, scoped under the current $HOME.
        NetworkCapability::None => Some(profile_for_home(home_dir().as_deref())),
        // Safety (Allowlist) / User+Master (Full): no OS network sandbox. Spawn
        // unchanged.
        NetworkCapability::Allowlist | NetworkCapability::Full => None,
    }
}

/// Read `$HOME` from the environment, treating unset OR empty as "no home".
///
/// Empty is treated as absent so we never join secret components onto `""` (which
/// would produce absolute `/.ssh`-style denies for a bogus root-relative path).
fn home_dir() -> Option<String> {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => Some(h),
        _ => None,
    }
}

/// Build the full Containment SBPL profile for a given (optional) `$HOME`.
///
/// Pure with respect to its argument: it does NOT read the environment (the caller
/// passes the home), so it is exhaustively unit-testable. It DOES touch the
/// filesystem for ONE thing only — canonicalizing the home directory via
/// [`std::fs::canonicalize`] — because Seatbelt matches the canonical path (the
/// `/tmp` → `/private/tmp` footgun). The secret subdirectories/files need not exist.
///
/// - `home == None` (or canonicalization of a present home fails) ⇒ returns exactly
///   [`NETWORK_DENY_PROFILE`] (network-only; we do NOT deny bogus paths).
/// - `home == Some(h)` ⇒ canonicalizes `h`, joins each [`SECRET_SUBDIRS`] entry as
///   a `(subpath …)` and each [`SECRET_LITERAL_FILES`] entry as a `(literal …)`,
///   and emits `(deny file-read* file-write* …)` for the whole set, appended to the
///   network deny.
///
/// SBPL string-escaping: path components here are aterm-fixed ASCII literals joined
/// onto a canonicalized home, so the only metacharacter that can appear is a
/// backslash or double-quote in a pathological home path; both are backslash-escaped
/// before emission so the emitted SBPL literal is always well-formed.
#[must_use]
pub fn profile_for_home(home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else {
        // No resolvable home → network-only; do NOT deny bogus paths.
        return NETWORK_DENY_PROFILE.to_string();
    };
    // Canonicalize the HOME itself (it must exist). Seatbelt matches the canonical
    // path; a non-canonical prefix would silently fail to match (security theater).
    // If canonicalization fails (home doesn't exist / unreadable) fall back to the
    // network-only profile rather than emit a deny on a path that may be wrong.
    let Ok(canon_home) = std::fs::canonicalize(home) else {
        return NETWORK_DENY_PROFILE.to_string();
    };

    let mut clauses = String::new();
    for sub in SECRET_SUBDIRS {
        let p = canon_home.join(sub);
        clauses.push_str(" (subpath \"");
        push_sbpl_escaped(&mut clauses, &p);
        clauses.push_str("\")");
    }
    for file in SECRET_LITERAL_FILES {
        let p = canon_home.join(file);
        clauses.push_str(" (literal \"");
        push_sbpl_escaped(&mut clauses, &p);
        clauses.push_str("\")");
    }
    // network deny + the secret-set file deny (read AND write).
    format!("{NETWORK_DENY_PROFILE}(deny file-read* file-write*{clauses})")
}

/// Append `path` to `out` as the inside of an SBPL string literal, backslash-
/// escaping `\` and `"` so the emitted `"…"` is always well-formed regardless of
/// the home path. Non-UTF-8 path bytes are emitted lossily (a non-UTF-8 home is not
/// a supported credential location; the worst case is a deny that does not match).
fn push_sbpl_escaped(out: &mut String, path: &Path) {
    for ch in path.to_string_lossy().chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::NetworkCapability;
    use crate::mode::ContainmentMode;
    use crate::policy::ContainmentPolicy;

    #[test]
    fn network_only_profile_is_the_exact_documented_sbpl() {
        // The bytes are load-bearing — the network-only fallback and the prefix of
        // the full profile both depend on this exact string.
        assert_eq!(
            NETWORK_DENY_PROFILE,
            "(version 1)(allow default)(deny network*)"
        );
    }

    #[test]
    fn home_unset_yields_network_only_profile() {
        // No resolvable home → network deny ONLY, no file denies for bogus paths.
        assert_eq!(profile_for_home(None), NETWORK_DENY_PROFILE);
        assert_eq!(profile_for_home(Some("")), NETWORK_DENY_PROFILE);
    }

    #[test]
    fn full_profile_starts_with_network_deny_and_denies_each_secret() {
        // Use a real, canonicalizable home so the path-joins resolve. The crate's
        // own temp dir is canonical on the test box.
        let tmp = std::env::temp_dir();
        // Canonicalize so our expected literals match what the generator emits.
        let canon = std::fs::canonicalize(&tmp).expect("canonicalize temp dir");
        let profile = profile_for_home(canon.to_str());

        // (1) It is the network deny PLUS a file deny — never less.
        assert!(
            profile.starts_with(NETWORK_DENY_PROFILE),
            "full profile must begin with the exact network deny; got {profile}"
        );
        assert!(
            profile.contains("(deny file-read* file-write*"),
            "full profile must deny read AND write of the secret set; got {profile}"
        );

        // (2) Every secret subdir appears as a CANONICAL subpath.
        for sub in SECRET_SUBDIRS {
            let expect = format!("(subpath \"{}\")", canon.join(sub).display());
            assert!(
                profile.contains(&expect),
                "profile must deny secret subdir {sub} as canonical subpath {expect}; got {profile}"
            );
        }
        // (3) Every secret literal file appears as a CANONICAL literal.
        for file in SECRET_LITERAL_FILES {
            let expect = format!("(literal \"{}\")", canon.join(file).display());
            assert!(
                profile.contains(&expect),
                "profile must deny secret file {file} as canonical literal {expect}; got {profile}"
            );
        }
    }

    #[test]
    fn full_profile_does_not_deny_the_whole_home() {
        // CRITICAL non-breakage invariant: the home itself (and thus ~/.zshrc) must
        // NOT be denied — only the secret subpaths/literals. There must be no
        // (subpath "<home>") or (literal "<home>") clause for the bare home.
        let tmp = std::env::temp_dir();
        let canon = std::fs::canonicalize(&tmp).expect("canonicalize temp dir");
        let profile = profile_for_home(canon.to_str());
        let bare_subpath = format!("(subpath \"{}\")", canon.display());
        let bare_literal = format!("(literal \"{}\")", canon.display());
        assert!(
            !profile.contains(&bare_subpath) && !profile.contains(&bare_literal),
            "profile must NOT deny the entire home (would break ~/.zshrc etc.); got {profile}"
        );
    }

    #[test]
    fn secret_paths_are_canonicalized_not_raw() {
        // The /tmp → /private/tmp footgun: on macOS, a home reached via /tmp
        // canonicalizes to /private/tmp. Emitting the RAW (non-canonical) prefix
        // would silently fail to match. Assert the generator emits the CANONICAL
        // prefix, not the raw one we passed in. (Skipped if /tmp is not a symlink.)
        let raw = std::path::Path::new("/tmp");
        if let Ok(canon) = std::fs::canonicalize(raw) {
            if canon != raw {
                let profile = profile_for_home(raw.to_str());
                let canon_marker = format!("(subpath \"{}/.ssh\")", canon.display());
                let raw_marker = "(subpath \"/tmp/.ssh\")";
                assert!(
                    profile.contains(&canon_marker),
                    "must emit CANONICAL secret path {canon_marker}; got {profile}"
                );
                assert!(
                    !profile.contains(raw_marker),
                    "must NOT emit the non-canonical raw path (Seatbelt would not match it); got {profile}"
                );
            }
        }
    }

    #[test]
    fn nonexistent_home_falls_back_to_network_only() {
        // A home that cannot be canonicalized (does not exist) → network-only, not
        // a deny on a possibly-wrong path.
        let bogus = "/nonexistent/aterm-no-such-home-xyz";
        assert_eq!(profile_for_home(Some(bogus)), NETWORK_DENY_PROFILE);
    }

    #[test]
    fn sbpl_escaping_quotes_and_backslashes() {
        // A pathological home with a quote/backslash must stay a well-formed SBPL
        // string literal (escaped), so the profile never breaks out of the "…".
        let mut out = String::new();
        push_sbpl_escaped(&mut out, std::path::Path::new("/a\"b\\c"));
        assert_eq!(out, "/a\\\"b\\\\c");
    }

    #[test]
    fn containment_caps_yield_a_network_plus_secret_profile() {
        // profile_for reads the real $HOME. Whatever it resolves to, the result for
        // Containment must be Some and must at least contain the network deny.
        let caps = ContainmentPolicy::capabilities(ContainmentMode::Containment);
        assert_eq!(caps.network, NetworkCapability::None, "precondition");
        let profile = profile_for(&caps).expect("Containment must yield a profile");
        assert!(
            profile.starts_with(NETWORK_DENY_PROFILE),
            "Containment profile must begin with the network deny; got {profile}"
        );
    }

    #[test]
    fn non_containment_modes_get_no_sandbox() {
        // User/Master have Full network; Safety has Allowlist. NONE of them is
        // sandboxed — profile_for must return None so their spawn is byte-identical
        // (no sandbox-exec wrap).
        for mode in [
            ContainmentMode::User,
            ContainmentMode::Master,
            ContainmentMode::Safety,
        ] {
            let caps = ContainmentPolicy::capabilities(mode);
            assert_eq!(
                profile_for(&caps),
                None,
                "{mode} must NOT be OS-sandboxed (network not denied)"
            );
        }
    }

    #[test]
    fn profile_selection_keys_only_on_network_capability_none() {
        // The contract is precisely "network == None ⇒ Some(profile)". Construct
        // capability sets directly across the NetworkCapability axis to lock that
        // it is the network field — and only that field — that decides.
        let base = ContainmentPolicy::capabilities(ContainmentMode::Containment);
        let mut none = base;
        none.network = NetworkCapability::None;
        assert!(profile_for(&none).is_some());

        let mut allow = base;
        allow.network = NetworkCapability::Allowlist;
        assert!(profile_for(&allow).is_none());

        let mut full = base;
        full.network = NetworkCapability::Full;
        assert!(profile_for(&full).is_none());
    }

    #[test]
    fn sandbox_exec_path_is_the_fixed_absolute_wrapper() {
        assert_eq!(SANDBOX_EXEC_PATH, "/usr/bin/sandbox-exec");
        assert!(
            SANDBOX_EXEC_PATH.starts_with('/'),
            "must be an absolute path (no PATH search in the child)"
        );
    }
}
