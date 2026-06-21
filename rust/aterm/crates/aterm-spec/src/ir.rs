// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Lowering the source↔spec cross-reference to a `trust-ir` `SpecModule` artifact
//! (TRUST_NATIVE_TLA, Phase 3).
//!
//! Phases 0-2 enforce the four bidirectional obligations of §2.2 *in Rust*, inside
//! aterm's own [`crate::xref::check_closure`]. Phase 3 makes the **TRUST toolchain
//! itself** the enforcer: aterm lowers the same models + anchors + waivers into a
//! `trust-ir` `SpecModule`-bearing module and hands it to `trust-ir spec-link`,
//! which independently certifies Ob.1/Ob.3/Ob.4. This module is the **producer**
//! half: it emits a byte-conforming `.trust_irtxt` artifact.
//!
//! ## Why text, not a `trust_ir` crate dependency
//!
//! The authoritative artifact format is documented at
//! `trust-ir/crates/trust-ir/docs/spec-module-format.md` (§2, the `.trust_irtxt`
//! grammar). aterm-spec is intentionally NOT in the production release closure and
//! has no `trust_ir` dependency; coupling it to the external Trust repo would be
//! fragile. Instead we emit the documented text format DIRECTLY, byte-for-byte
//! identical to `trust_ir`'s canonical writer (`display.rs::write_spec_module`):
//!
//!   * module header `; TrustIr text format v1` + `module "<name>"`,
//!   * each `spec_module "<name>" { … }` block preceded by ONE blank line,
//!   * every free-form string quoted via Rust `{:?}` escaping (`"`→`\"`, `\`→`\\`,
//!     newline→`\n`, tab→`\t`) — Rust's `Debug for str` IS that escaping, so we use
//!     `format!("{s:?}")` and the bytes match the trust-ir writer exactly.
//!
//! Stability is proven from aterm by running `trust-ir convert --from text --to
//! text` (fmt→parse→fmt fixed point) and `trust-ir spec-link` over the assembled
//! artifact in the gate (`aterm-gui` `spec_xref_gate`).
//!
//! ## Machine-name canonicalization (the one real subtlety)
//!
//! aterm's anchors use the lower_snake convention (`"terminal_modes"`,
//! `"fork_exec"`, `"sandbox"`) while a `Model::name`/MODULE name is CamelCase
//! (`"TerminalModes"`, `"ForkExec"`, `"Sandbox"`). aterm's in-Rust resolver
//! ([`crate::xref::machine_matches`]) matches case-insensitively after stripping
//! `_`. But `trust-ir spec-link` resolves machines by EXACT string match
//! (`by_name.get(machine)`), so the lowered artifact MUST rewrite every anchor's /
//! waiver's `machine` to the canonical `SpecModule.name` it resolves to. We do that
//! here (via `machine_matches`), so the artifact Ob.4-resolves in trust-ir exactly
//! when it resolves in aterm — the two enforcers agree by construction.

use std::collections::BTreeSet;

use crate::derive::Model;
use crate::xref::{machine_matches, ProofAnchor, ProofKind, RefinementAnchor, SpecModule, Waiver};

/// Render a free-form string as a `trust-ir` text-format quoted token: Rust `{:?}`
/// escaping (`"`→`\"`, `\`→`\\`, newline→`\n`, tab→`\t`), which is byte-identical to
/// the `{:?}` the trust-ir canonical writer uses for every free-form field.
fn q(s: &str) -> String {
    format!("{s:?}")
}

/// Lower ONE registered [`SpecModule`] to its `spec_module "<name>" { … }` text
/// block (no leading blank line — the assembler inserts the inter-block blank line).
/// `anchors`/`waivers` are the already-collected, already-canonicalized records that
/// belong to THIS module (i.e. their `machine` resolves to `module.name()`), emitted
/// as `anchor`/`waiver` lines inside the block.
///
/// * **Embedded** `Model`: `origin embedded`; `var "<name>" : "<ty>"` for every
///   state variable (scalar → `"Int"`, function-valued → `"[1..<range> -> BOOLEAN]"`,
///   an opaque tag the standalone IR does not interpret); `action "<name>"` for every
///   model action; `invariant "<name>" : "<formula>"` with the formula rendered by
///   the model's own TLA+ generator ([`crate::derive::Expr::to_tla`]).
/// * **External** `.tla` (ISOLATION family): `origin external "<path>"`; `action
///   "<name>"` for every `Next` disjunct (`coverage_actions`); no vars/invariants
///   (the external `.tla` parser does not extract typed vars, and the doc's external
///   example carries actions only — they are pure design intent bound by anchors).
fn lower_module_block(
    module: &SpecModule,
    anchors: &[&RefinementAnchor],
    waivers: &[&Waiver],
    proofs: &[&ProofAnchor],
) -> String {
    let name = module.name();
    let mut s = String::new();
    s.push_str(&format!("spec_module {} {{\n", q(name)));

    match module {
        SpecModule::Embedded(m) => {
            s.push_str("  origin embedded\n");
            emit_embedded_vars(&mut s, m);
            // Actions in declared order (order is preserved per the doc).
            for a in &m.actions {
                s.push_str(&format!("  action {}\n", q(a.name)));
            }
            for inv in &m.invariants {
                s.push_str(&format!(
                    "  invariant {} : {}\n",
                    q(inv.name),
                    q(&inv.expr.to_tla())
                ));
            }
        }
        SpecModule::External(t) => {
            s.push_str(&format!("  origin external {}\n", q(&t.file_path)));
            // Actions = the Next disjuncts (the coverage set), in sorted order.
            for a in module.coverage_actions() {
                s.push_str(&format!("  action {}\n", q(&a)));
            }
        }
    }

    // Anchors (canonical machine name == this module's name).
    for a in anchors {
        s.push_str(&format!(
            "  anchor machine {} action {} rust {} span {}",
            q(name),
            q(a.action),
            q(a.rust_method),
            q(a.location),
        ));
        if !a.project.is_empty() {
            s.push_str(&format!(" project {}", q(a.project)));
        }
        s.push('\n');
    }
    // Waivers.
    for w in waivers {
        s.push_str(&format!(
            "  waiver machine {} action {} reason {}\n",
            q(name),
            q(w.action),
            q(w.reason),
        ));
    }
    // Proofs (TRUST_VACUITY_GATE §2.1 / finding 1b): each `proof_anchor!`'d kani
    // harness lowers to a `proof machine "m" action "a" name "h" kind "kani"` line —
    // the IR analogue of `proof_anchor!`. trust-ir's L1 resolves `name` against the
    // supplied `--harness-manifest`; Ob.1/Ob.4 hold on `machine`/`action` like an
    // anchor. `kind` is the lowercase tag the text grammar expects (`"kani"`).
    for p in proofs {
        let kind = match p.kind {
            ProofKind::Kani => "kani",
        };
        s.push_str(&format!(
            "  proof machine {} action {} name {} kind {}\n",
            q(name),
            q(p.action),
            q(p.proof_name),
            q(kind),
        ));
    }

    s.push_str("}\n");
    s
}

/// Emit the `var "<name>" : "<ty>"` lines for an embedded [`Model`]: scalar vars in
/// declared order (opaque tag `"Int"`), then function-valued vars (opaque tag
/// `"[1..<range> -> BOOLEAN]"`, mirroring the all-FALSE `[1..range -> BOOLEAN]`
/// init the TLA+ generator emits). `ty` is opaque to the standalone IR.
fn emit_embedded_vars(s: &mut String, m: &Model) {
    for v in &m.vars {
        s.push_str(&format!("  var {} : {}\n", q(v.name), q("Int")));
    }
    for f in &m.fn_vars {
        let ty = format!("[1..{} -> BOOLEAN]", f.range);
        s.push_str(&format!("  var {} : {}\n", q(f.name), q(&ty)));
    }
}

/// Assemble ONE complete `.trust_irtxt` module: the v1 header + `module "<name>"`,
/// then every registered [`SpecModule`] (embedded `Model`s and external ISOLATION
/// `.tla`) as a `spec_module` block, with ALL collected `refinements`/`waivers`
/// rendered as `anchor`/`waiver` lines inside the block whose name they resolve to.
///
/// The output is byte-for-byte the trust-ir canonical text format, so it round-trips
/// through `trust-ir convert` and is accepted by `trust-ir spec-link`. The four
/// obligations the artifact must satisfy under `spec-link` (Ob.1 action-exists, Ob.3
/// coverage, Ob.4 machine-resolves) are EXACTLY the ones aterm's
/// [`crate::xref::check_closure`] checks in-Rust — so a green in-Rust closure and a
/// green `trust-ir spec-link` certify the same contract, from two independent
/// enforcers.
///
/// ## Resolution / canonicalization
///
/// Each anchor/waiver is bucketed into the module its `machine` resolves to via
/// [`machine_matches`] (lower_snake ⟺ CamelCase). Its `machine` field is REWRITTEN to
/// that module's canonical `name()` so trust-ir's exact-match resolver (Ob.4) agrees
/// with aterm's case-insensitive one. An anchor/waiver whose `machine` resolves to no
/// module, or that carries an empty `machine` (the bare `reason="…"` bypass-setter
/// waiver form), is DROPPED from the artifact — those are dangling-machine /
/// not-tied-to-a-machine cases that aterm's own gate reports separately (a dangling
/// machine is an aterm-side Ob.4 violation that fails the in-Rust closure BEFORE we
/// ever assemble; emitting it here would just make trust-ir reject an artifact aterm
/// already rejected). Waivers with an empty `action` likewise cannot discharge
/// coverage and are dropped.
pub fn lower_to_ir(
    module_name: &str,
    modules: &[SpecModule],
    refinements: &[&RefinementAnchor],
    waivers: &[&Waiver],
    proofs: &[&ProofAnchor],
) -> String {
    let mut out = String::new();
    out.push_str("; TrustIr text format v1\n");
    out.push_str(&format!("module {}\n", q(module_name)));

    for module in modules {
        let canon = module.name();
        let my_anchors: Vec<&RefinementAnchor> = refinements
            .iter()
            .copied()
            .filter(|a| machine_matches(a.machine, canon))
            .collect();
        let my_waivers: Vec<&Waiver> = waivers
            .iter()
            .copied()
            .filter(|w| !w.machine.is_empty() && !w.action.is_empty() && machine_matches(w.machine, canon))
            .collect();
        // Proof anchors whose `machine` resolves to THIS module (canonicalized like
        // anchors). A proof naming no registered machine is dropped — aterm's own gate
        // reports the dangling machine as an Ob.4 violation before we ever assemble.
        let my_proofs: Vec<&ProofAnchor> = proofs
            .iter()
            .copied()
            .filter(|p| machine_matches(p.machine, canon))
            .collect();
        out.push('\n');
        out.push_str(&lower_module_block(module, &my_anchors, &my_waivers, &my_proofs));
    }

    out
}

/// The set of canonical module names referenced by `anchors` that resolve to one of
/// `modules` — a small helper for the gate's reporting (e.g. "N machines lowered").
pub fn lowered_machine_names(modules: &[SpecModule], anchors: &[&RefinementAnchor]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for a in anchors {
        for m in modules {
            if machine_matches(a.machine, m.name()) {
                names.insert(m.name().to_string());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::{ring_model, terminal_modes_model};

    /// A hand-built anchor record (mirrors what the `#[refines]` macro submits).
    fn anchor(
        machine: &'static str,
        action: &'static str,
        rust: &'static str,
        loc: &'static str,
        project: &'static str,
    ) -> RefinementAnchor {
        RefinementAnchor { machine, action, rust_method: rust, location: loc, project }
    }

    fn waiver(machine: &'static str, action: &'static str, reason: &'static str) -> Waiver {
        Waiver { machine, action, reason, rust_method: "test", location: "test:1" }
    }

    fn proof(machine: &'static str, action: &'static str, name: &'static str) -> ProofAnchor {
        ProofAnchor {
            machine,
            action,
            proof_name: name,
            kind: ProofKind::Kani,
            location: "test:1",
        }
    }

    #[test]
    fn header_is_v1_canonical() {
        let txt = lower_to_ir("aterm_spec_xref", &[], &[], &[], &[]);
        assert!(txt.starts_with("; TrustIr text format v1\nmodule \"aterm_spec_xref\"\n"), "{txt}");
    }

    #[test]
    fn embedded_block_emits_vars_actions_invariants_and_canonicalizes_machine() {
        let modules = vec![SpecModule::Embedded(ring_model())];
        // Anchor uses the lower convention "ring"; Model::name is "Ring".
        let a = anchor("ring", "Push", "aterm_buffer::Ring::push", "ring.rs:1:1", "Ring::project");
        let w = waiver("ring", "Push", "test waiver");
        let txt = lower_to_ir("m", &modules, &[&a], &[&w], &[]);
        // Canonical name "Ring" used everywhere in the block.
        assert!(txt.contains("spec_module \"Ring\" {\n"), "{txt}");
        assert!(txt.contains("  origin embedded\n"), "{txt}");
        assert!(txt.contains("  var \"seq\" : \"Int\"\n"), "{txt}");
        assert!(txt.contains("  var \"lo\" : \"Int\"\n"), "{txt}");
        assert!(txt.contains("  action \"Push\"\n"), "{txt}");
        assert!(txt.contains("  invariant \"LenBounded\" : \"seq - lo + 1 =< Cap\"\n"), "{txt}");
        // Anchor + waiver machine REWRITTEN to canonical "Ring" (exact-match for trust-ir).
        assert!(
            txt.contains("  anchor machine \"Ring\" action \"Push\" rust \"aterm_buffer::Ring::push\" span \"ring.rs:1:1\" project \"Ring::project\"\n"),
            "{txt}"
        );
        assert!(txt.contains("  waiver machine \"Ring\" action \"Push\" reason \"test waiver\"\n"), "{txt}");
    }

    #[test]
    fn anchor_without_project_omits_project_clause() {
        let modules = vec![SpecModule::Embedded(ring_model())];
        let a = anchor("ring", "Push", "r::push", "r.rs:1:1", ""); // empty project
        let txt = lower_to_ir("m", &modules, &[&a], &[], &[]);
        assert!(
            txt.contains("  anchor machine \"Ring\" action \"Push\" rust \"r::push\" span \"r.rs:1:1\"\n"),
            "no project clause: {txt}"
        );
        assert!(!txt.contains("project"), "{txt}");
    }

    #[test]
    fn unresolved_or_bare_records_are_dropped() {
        let modules = vec![SpecModule::Embedded(ring_model())];
        let dangling = anchor("no_such_machine", "X", "r", "r:1", "");
        let bare = waiver("", "", "bypass-setter"); // empty machine
        let txt = lower_to_ir("m", &modules, &[&dangling], &[&bare], &[]);
        assert!(!txt.contains("no_such_machine"), "dangling anchor dropped: {txt}");
        assert!(!txt.contains("bypass-setter"), "bare waiver dropped: {txt}");
    }

    #[test]
    fn proof_anchor_lowers_to_a_proof_line_canonicalized() {
        let modules = vec![SpecModule::Embedded(ring_model())];
        // Proof uses the lower convention "ring"; Model::name is "Ring".
        let p = proof("ring", "Push", "ring_push_refines");
        let txt = lower_to_ir("m", &modules, &[], &[], &[&p]);
        assert!(
            txt.contains("  proof machine \"Ring\" action \"Push\" name \"ring_push_refines\" kind \"kani\"\n"),
            "proof line (canonicalized machine + lowercase kani kind): {txt}"
        );
    }

    #[test]
    fn proof_for_unresolved_machine_is_dropped() {
        let modules = vec![SpecModule::Embedded(ring_model())];
        let p = proof("no_such_machine", "Push", "ghost_harness");
        let txt = lower_to_ir("m", &modules, &[], &[], &[&p]);
        assert!(!txt.contains("ghost_harness"), "proof for a dangling machine is dropped: {txt}");
    }

    #[test]
    fn camelcase_terminal_modes_resolves_from_lower_anchor() {
        let modules = vec![SpecModule::Embedded(terminal_modes_model())];
        let a = anchor("terminal_modes", "SetOriginMode", "h::set", "h.rs:1:1", "");
        let names = lowered_machine_names(&modules, &[&a]);
        assert!(names.contains("TerminalModes"), "{names:?}");
    }
}
