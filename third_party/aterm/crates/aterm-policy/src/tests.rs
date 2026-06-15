// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the Phase 0 scaffold (#7991).
//!
//! These tests cover the three acceptance criteria from the techlead brief:
//!
//! 1. Round-trip: `hardened → TOML → parse → equal`.
//! 2. Schema validation: malformed TOML / wrong `schema_version` falls back to
//!    Hardened.
//! 3. Profile refinement ordering: `hardened ⊆ standard ⊆ permissive` over
//!    the unmatched-default response rank.
//!
//! A wider profile matrix against every concrete sequence lives with the
//! engine (#7992) once the decision tree exists.

use super::{
    Defaults, MirrorField, MirrorSnapshot, OriginTag, Policy, Profile, RateLimit, Response, Rule,
    SCHEMA_VERSION, aliases, engine::PolicyEngine, profiles, profiles::refinement::response_rank,
};

// ---------------------------------------------------------------------------
// Round-trip
// ---------------------------------------------------------------------------

#[test]
fn hardened_roundtrips_through_toml() {
    let original = profiles::hardened();
    let ser = original.to_toml().expect("hardened serializes");
    let parsed: Policy = toml::from_str(&ser).expect("hardened parses");
    assert_eq!(
        original, parsed,
        "hardened policy round-trip must be lossless"
    );
}

#[test]
fn standard_roundtrips_through_toml() {
    let original = profiles::standard();
    let ser = original.to_toml().expect("standard serializes");
    let parsed: Policy = toml::from_str(&ser).expect("standard parses");
    assert_eq!(original, parsed);
}

#[test]
fn permissive_roundtrips_through_toml() {
    let original = profiles::permissive();
    let ser = original.to_toml().expect("permissive serializes");
    let parsed: Policy = toml::from_str(&ser).expect("permissive parses");
    assert_eq!(original, parsed);
}

// ---------------------------------------------------------------------------
// Fail-closed loader
// ---------------------------------------------------------------------------

#[test]
fn malformed_toml_falls_back_to_hardened() {
    let (policy, fell_back) = Policy::from_toml_or_hardened("<<< not toml >>>");
    assert!(fell_back, "malformed TOML must signal fall-back");
    assert_eq!(policy, profiles::hardened());
}

#[test]
fn unknown_schema_version_falls_back_to_hardened() {
    // Build a valid-shape TOML but with schema_version = 9999.
    let mut future = profiles::standard();
    future.schema_version = 9_999;
    let raw = future.to_toml().expect("serialize future policy");
    let (policy, fell_back) = Policy::from_toml_or_hardened(&raw);
    assert!(fell_back, "unknown schema_version must fall back");
    assert_eq!(policy, profiles::hardened());
}

#[test]
fn unknown_field_rejected_via_deny_unknown_fields() {
    // `extra_field` is not in the Policy schema; serde(deny_unknown_fields)
    // must reject and the fallback kicks in.
    let toml_src = r#"
schema_version = 1
profile = "Standard"
extra_field = "boom"
[defaults]
unmatched = "warn"
"#;
    let (policy, fell_back) = Policy::from_toml_or_hardened(toml_src);
    assert!(fell_back, "unknown field must trigger fall-back");
    assert_eq!(policy, profiles::hardened());
}

#[test]
fn valid_minimal_toml_parses_cleanly() {
    let toml_src = r#"
schema_version = 1
profile = "Permissive"
[defaults]
unmatched = "execute"
"#;
    let (policy, fell_back) = Policy::from_toml_or_hardened(toml_src);
    assert!(!fell_back, "valid TOML must not fall back");
    assert_eq!(policy.schema_version, SCHEMA_VERSION);
    assert_eq!(policy.profile, Profile::Permissive);
    assert_eq!(policy.defaults.unmatched, Response::Execute);
    assert!(policy.rules.is_empty());
    assert!(policy.rate_limits.is_empty());
}

// ---------------------------------------------------------------------------
// Refinement invariant
// ---------------------------------------------------------------------------

#[test]
fn refinement_unmatched_default_is_monotone() {
    let h = response_rank(profiles::hardened().defaults.unmatched);
    let s = response_rank(profiles::standard().defaults.unmatched);
    let p = response_rank(profiles::permissive().defaults.unmatched);
    assert!(
        h >= s && s >= p,
        "refinement violated: hardened={h} standard={s} permissive={p} \
         (expected hardened >= standard >= permissive over strictness rank)"
    );
}

#[test]
fn hardened_denies_unmatched() {
    assert_eq!(
        profiles::hardened().defaults.unmatched,
        Response::Drop,
        "Hardened MUST drop on unmatched (§4.1 + §7.3)"
    );
}

#[test]
fn permissive_executes_unmatched() {
    assert_eq!(profiles::permissive().defaults.unmatched, Response::Execute);
}

#[test]
fn standard_warns_on_unmatched() {
    assert_eq!(profiles::standard().defaults.unmatched, Response::Warn);
}

// ---------------------------------------------------------------------------
// Hardened-specific invariants from techlead brief
// ---------------------------------------------------------------------------

#[test]
fn hardened_rules_only_admit_host_configfile_or_user() {
    let h = profiles::hardened();
    for rule in &h.rules {
        match rule.origin_min {
            OriginTag::Host | OriginTag::ConfigFile | OriginTag::User => {}
            other => panic!(
                "Hardened rule for {:?} has origin_min = {:?}, expected Host | ConfigFile | User",
                rule.sequence, other
            ),
        }
    }
}

#[test]
fn hardened_requires_shell_integration_nonce() {
    assert!(
        profiles::hardened()
            .defaults
            .shell_integration_require_nonce
    );
}

// ---------------------------------------------------------------------------
// Schema version + profile tag coherence
// ---------------------------------------------------------------------------

#[test]
fn all_builtin_profiles_stamp_current_schema_version() {
    for p in [
        profiles::permissive(),
        profiles::standard(),
        profiles::hardened(),
    ] {
        assert_eq!(p.schema_version, SCHEMA_VERSION);
    }
}

#[test]
fn builtin_profiles_carry_correct_profile_tag() {
    assert_eq!(profiles::permissive().profile, Profile::Permissive);
    assert_eq!(profiles::standard().profile, Profile::Standard);
    assert_eq!(profiles::hardened().profile, Profile::Hardened);
}

// ---------------------------------------------------------------------------
// Rate-limit references resolve
// ---------------------------------------------------------------------------

#[test]
fn every_rate_limit_ref_resolves_in_each_profile() {
    for policy in [
        profiles::permissive(),
        profiles::standard(),
        profiles::hardened(),
    ] {
        let ids: Vec<&str> = policy.rate_limits.iter().map(|rl| rl.id.as_str()).collect();
        for rule in &policy.rules {
            if let Some(ref rl_ref) = rule.rate_limit {
                assert!(
                    ids.contains(&rl_ref.as_str()),
                    "profile {:?}: rule {:?} references unknown rate limit {rl_ref:?}",
                    policy.profile,
                    rule.sequence,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Alias table
// ---------------------------------------------------------------------------

#[test]
fn alias_table_has_expected_entries() {
    // Design §3.4 — 7 canonical aliases.
    assert_eq!(aliases::count(), 7);
    assert!(aliases::lookup("OSC 52 set").is_some());
    assert!(aliases::lookup("OSC 52 query").is_some());
    assert!(aliases::lookup("response any").is_some());
}

#[test]
fn alias_lookup_is_case_sensitive() {
    assert!(aliases::lookup("OSC 52 set").is_some());
    assert!(aliases::lookup("osc 52 set").is_none());
}

// ---------------------------------------------------------------------------
// Hand-built Rule / RateLimit round-trip
// ---------------------------------------------------------------------------

#[test]
fn manually_built_policy_roundtrips() {
    let p = Policy {
        schema_version: SCHEMA_VERSION,
        profile: Profile::Standard,
        defaults: Defaults {
            unmatched: Response::Warn,
            shell_integration_require_nonce: true,
        },
        rules: vec![Rule {
            sequence: "OSC 52 set".to_owned(),
            origin_min: OriginTag::User,
            response: Response::Ask,
            rate_limit: Some("clipboard".to_owned()),
            prompt_id: Some("clipboard-write".to_owned()),
        }],
        rate_limits: vec![RateLimit {
            id: "clipboard".to_owned(),
            capacity_bytes: 16_384,
            refill_per_second: 1_024,
            per_sequence_max: 65_536,
        }],
    };

    let ser = p.to_toml().expect("serialize");
    let back: Policy = toml::from_str(&ser).expect("parse");
    assert_eq!(p, back);
}

// ---------------------------------------------------------------------------
// Mirror-field invariant (§4.5 + §6.1)
// ---------------------------------------------------------------------------
//
// These tests exercise the read (`mirror_snapshot`) and write
// (`set_mirror_bool`) halves of the §6.1 invariant:
//
//     policy_engine.effective_response_pty(field) == Execute
//         iff  modes.allow_<field> == true
//
// Wiring into `Terminal` / `aterm-core` is out of scope for this crate; the
// follow-up issue tests the end-to-end consistency once handlers consult the
// engine. Here we verify the local algebra: snapshot matches probe, and
// `set → snapshot` round-trips in both directions.

/// Helper: assert that `snapshot.get(field) == (effective_response_pty(field)
/// == Execute)` for every mirror field. The body of the §6.1 invariant, run
/// as a check against any engine state.
fn assert_snapshot_matches_engine(eng: &PolicyEngine, label: &str) {
    let snap = eng.mirror_snapshot();
    for field in MirrorField::ALL {
        let rsp = eng.effective_response_pty(field);
        let bool_is_exec = snap.get(field);
        assert_eq!(
            bool_is_exec,
            rsp == Response::Execute,
            "{label}: mirror snapshot out of sync with engine for {field:?} \
             (snapshot = {bool_is_exec}, effective_response_pty = {rsp:?})"
        );
    }
}

#[test]
fn mirror_snapshot_matches_effective_response_on_permissive() {
    let eng = PolicyEngine::new(profiles::permissive());
    assert_snapshot_matches_engine(&eng, "permissive");
    // Permissive's wildcard `response any` catches every Pty-origin probe
    // with Execute, and nonce is not required.
    let snap = eng.mirror_snapshot();
    assert!(snap.allow_osc52_query);
    assert!(snap.allow_osc52_set);
    assert!(snap.allow_window_ops);
    assert!(snap.allow_notifications);
    assert!(snap.allow_palette_reconfigure);
    assert!(!snap.require_shell_integration_nonce);
}

#[test]
fn mirror_snapshot_matches_effective_response_on_standard() {
    let eng = PolicyEngine::standard();
    assert_snapshot_matches_engine(&eng, "standard");
    // Standard requires the nonce; every per-field allow_* bool is observed
    // as `true` at Pty origin because the rules' strict origin_min gates
    // fall through to the `response any` wildcard (see engine::tests
    // `standard_clipboard_set_from_pty_falls_through_to_unmatched`).
    let snap = eng.mirror_snapshot();
    assert!(snap.require_shell_integration_nonce);
}

#[test]
fn mirror_snapshot_matches_effective_response_on_hardened() {
    let eng = PolicyEngine::hardened();
    assert_snapshot_matches_engine(&eng, "hardened");
    // Hardened's unmatched-default is Drop; no rule fires at Pty origin so
    // every allow_* probe returns Drop → bool = false.
    let snap = eng.mirror_snapshot();
    assert!(!snap.allow_osc52_query);
    assert!(!snap.allow_osc52_set);
    assert!(!snap.allow_window_ops);
    assert!(!snap.allow_notifications);
    assert!(!snap.allow_palette_reconfigure);
    assert!(snap.require_shell_integration_nonce);
}

#[test]
fn policy_engine_standard_matches_new_of_standard_profile() {
    // `PolicyEngine::standard` is a convenience constructor; verify it is
    // observationally equivalent to `PolicyEngine::new(profiles::standard())`.
    let a = PolicyEngine::standard();
    let b = PolicyEngine::new(profiles::standard());
    assert_eq!(a.policy(), b.policy());
    assert_eq!(a.mirror_snapshot(), b.mirror_snapshot());
}

#[test]
fn set_mirror_bool_then_effective_response_roundtrips_true() {
    // For every field and every profile, setting the bool to `true` must
    // leave `effective_response_pty == Execute`.
    for starting in [
        profiles::permissive(),
        profiles::standard(),
        profiles::hardened(),
    ] {
        for field in MirrorField::ALL {
            let mut eng = PolicyEngine::new(starting.clone());
            eng.set_mirror_bool(field, true);
            assert_eq!(
                eng.effective_response_pty(field),
                Response::Execute,
                "field {field:?} on profile {:?} did not become Execute after set(true)",
                starting.profile,
            );
            assert!(
                eng.mirror_snapshot().get(field),
                "snapshot.get({field:?}) did not become true after set(true) on profile {:?}",
                starting.profile,
            );
        }
    }
}

#[test]
fn set_mirror_bool_then_effective_response_roundtrips_false() {
    // For every field and every profile, setting the bool to `false` must
    // leave `effective_response_pty != Execute`.
    for starting in [
        profiles::permissive(),
        profiles::standard(),
        profiles::hardened(),
    ] {
        for field in MirrorField::ALL {
            let mut eng = PolicyEngine::new(starting.clone());
            eng.set_mirror_bool(field, false);
            assert_ne!(
                eng.effective_response_pty(field),
                Response::Execute,
                "field {field:?} on profile {:?} stayed Execute after set(false)",
                starting.profile,
            );
            assert!(
                !eng.mirror_snapshot().get(field),
                "snapshot.get({field:?}) stayed true after set(false) on profile {:?}",
                starting.profile,
            );
        }
    }
}

#[test]
fn mirror_invariant_holds_for_every_field_after_arbitrary_sequence() {
    // Drive a field through true → false → true → false and verify the
    // §6.1 invariant at every step. Use Hardened as the starting point so
    // the initial state is all-false.
    let mut eng = PolicyEngine::hardened();
    for field in MirrorField::ALL {
        for value in [true, false, true, false] {
            eng.set_mirror_bool(field, value);
            assert_snapshot_matches_engine(&eng, &format!("after set({field:?}, {value})"));
            assert_eq!(eng.mirror_snapshot().get(field), value);
        }
    }
}

#[test]
fn set_mirror_bool_does_not_grow_rule_list_unboundedly() {
    // Calling set_mirror_bool repeatedly for the same field must not
    // accumulate stale mirror rules: each call removes the previous
    // mirror-tagged rule before inserting the new one.
    let mut eng = PolicyEngine::hardened();
    let starting_len = eng.policy().rules.len();

    // 10 alternating toggles for every field.
    for _ in 0..5 {
        for field in MirrorField::ALL {
            eng.set_mirror_bool(field, true);
            eng.set_mirror_bool(field, false);
        }
    }

    // At most one mirror rule per rule-gated field (5 of them — the nonce
    // field does not create a rule). So the final rule count is the
    // original plus at most five.
    let final_len = eng.policy().rules.len();
    assert!(
        final_len <= starting_len + 5,
        "rule list grew unboundedly: {starting_len} → {final_len}"
    );
}

#[test]
fn set_mirror_bool_require_nonce_updates_defaults_only() {
    // The require-nonce field is not a per-sequence rule (§6.4). Writing it
    // must change defaults.shell_integration_require_nonce and leave the
    // rule list intact.
    let mut eng = PolicyEngine::hardened();
    let rule_count_before = eng.policy().rules.len();

    eng.set_mirror_bool(MirrorField::RequireShellIntegrationNonce, false);
    assert!(!eng.policy().defaults.shell_integration_require_nonce);
    assert_eq!(eng.policy().rules.len(), rule_count_before);
    assert!(!eng.mirror_snapshot().require_shell_integration_nonce);

    eng.set_mirror_bool(MirrorField::RequireShellIntegrationNonce, true);
    assert!(eng.policy().defaults.shell_integration_require_nonce);
    assert_eq!(eng.policy().rules.len(), rule_count_before);
    assert!(eng.mirror_snapshot().require_shell_integration_nonce);
}

#[test]
fn mirror_fields_are_independent() {
    // Setting one field's bool must not flip any other field's bool.
    let mut eng = PolicyEngine::hardened();
    let baseline = eng.mirror_snapshot();

    eng.set_mirror_bool(MirrorField::AllowOsc52Set, true);
    let after = eng.mirror_snapshot();

    assert!(after.allow_osc52_set);
    // Every other field must match the baseline.
    for field in MirrorField::ALL {
        if field == MirrorField::AllowOsc52Set {
            continue;
        }
        assert_eq!(
            baseline.get(field),
            after.get(field),
            "setting AllowOsc52Set disturbed {field:?}"
        );
    }
}

#[test]
fn mirror_snapshot_default_is_all_false() {
    // The `Default` derive on MirrorSnapshot must produce all-false (a
    // fail-closed baseline). The `Terminal` wiring issue depends on this to
    // safely initialize before any policy is loaded.
    let snap = MirrorSnapshot::default();
    for field in MirrorField::ALL {
        assert!(!snap.get(field));
    }
}

#[test]
fn mirror_snapshot_set_then_get_roundtrips() {
    let mut snap = MirrorSnapshot::default();
    for field in MirrorField::ALL {
        snap.set(field, true);
        assert!(snap.get(field));
        snap.set(field, false);
        assert!(!snap.get(field));
    }
}

#[test]
fn mirror_rules_survive_toml_roundtrip() {
    // Mirror rules are stored in the policy's rule vector (tagged via
    // prompt_id). They MUST round-trip through TOML so that a policy
    // serialized after set_mirror_bool deserializes to an equivalent engine.
    let mut eng = PolicyEngine::hardened();
    eng.set_mirror_bool(MirrorField::AllowOsc52Set, true);
    eng.set_mirror_bool(MirrorField::AllowNotifications, true);

    let snap_before = eng.mirror_snapshot();
    let ser = eng.policy().to_toml().expect("serialize");
    let parsed: Policy = toml::from_str(&ser).expect("parse");
    let reloaded = PolicyEngine::new(parsed);
    assert_eq!(reloaded.mirror_snapshot(), snap_before);
}
