// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! The compiler-collectable source↔spec cross-reference (TRUST_NATIVE_TLA, §2).
//!
//! This is the runtime backing for the no-longer-decorative `#[refines]`,
//! `#[spec_unmodeled]`, and `#[spec_invariant]` attribute macros. Each macro now
//! emits an `inventory::submit!` of a record into a distributed slice, so the FULL
//! set of source↔spec bindings is collectable at test/compile time WITHOUT scanning
//! source text — the standard `inventory` pattern.
//!
//! Two halves meet here:
//!   * **Source → spec**: [`RefinementAnchor`] / [`Waiver`] / [`InvariantAnchor`]
//!     records, submitted by the attribute macros, collected via [`refinements`],
//!     [`waivers`], [`invariant_anchors`].
//!   * **Spec → source**: the embedded [`Model`](crate::derive::Model) registry
//!     ([`model_registry`]) + [`Model::anchors`](crate::derive::Model::anchors),
//!     plus external `.tla` parsed by [`TlaSpec::parse`](crate::tla_check::TlaSpec).
//!
//! [`check_closure`] enforces the four bidirectional obligations of §2.2 over both
//! kinds of `SpecModule`, with coverage scoped to *actively-bound* machines (see
//! the doc on that fn). It is the runnable form of `trust-spec-link` (Phase 0),
//! before the IR pass exists (Phase 3).
//!
//! IMPORTANT — collection scope. `inventory` only sees `submit!`s from object code
//! LINKED into the running binary. The `#[refines]`/`#[spec_unmodeled]` usages are
//! all `#[cfg_attr(test, ...)]`-gated, so they expand (and submit) ONLY in the
//! *unit-test* build of the crate that hosts them (aterm-core, aterm-types). A gate
//! that wants to see aterm-core's anchors must therefore run inside aterm-core's
//! own unit-test binary — which is exactly where the `spec_xref_closure` gate lives.

use std::collections::{BTreeMap, BTreeSet};

use crate::derive::Model;
use crate::tla_check::TlaSpec;

/// A source→spec refinement binding, emitted by `#[refines(machine, action, …)]`.
///
/// This is the `inventory`-collectable anchor record. The struct mirrors
/// [`crate::coverage::RefinementEntry`] but uses `&'static str` so it can live in a
/// `const` submitted at link time (the macro span-captures `file!()`/`line!()`).
#[derive(Debug, Clone, Copy)]
pub struct RefinementAnchor {
    /// The TLA+ machine name (e.g. `"terminal_modes"`) — matches a `Model::name`
    /// (lower/CamelCase resolved by [`machine_matches`]) or an external module name.
    pub machine: &'static str,
    /// The TLA+ action name (e.g. `"SetCursorVisible"`) — matches an `Action`.
    pub action: &'static str,
    /// The annotated Rust fn (path/name), e.g. `"TerminalHandler::show_cursor"`.
    pub rust_method: &'static str,
    /// Source location `file:line` (proc-macro `file!()`/`line!()`).
    pub location: &'static str,
    /// Optional projection fn path (the `project=` arg), `""` when absent.
    pub project: &'static str,
}

/// An explicit "this fn is intentionally NOT modeled" waiver, emitted by
/// `#[spec_unmodeled(reason, …)]`. `machine`/`action` are optional (a bare
/// `reason="…"` waiver — the legacy bypass-setter form — leaves them `""`); when
/// present, the waiver discharges that model `Action` for the coverage obligation.
#[derive(Debug, Clone, Copy)]
pub struct Waiver {
    pub machine: &'static str,
    pub action: &'static str,
    pub reason: &'static str,
    pub rust_method: &'static str,
    pub location: &'static str,
}

/// A source→spec invariant binding, emitted by `#[spec_invariant(id, machine, …)]`.
/// `machine` is optional for back-compat (`""` when absent); when present it must
/// resolve to a registered `SpecModule` (obligation 4).
#[derive(Debug, Clone, Copy)]
pub struct InvariantAnchor {
    pub machine: &'static str,
    pub id: &'static str,
    pub rust_method: &'static str,
    pub location: &'static str,
}

/// Which verifier discharges a [`ProofAnchor`]'s obligation (TRUST_NATIVE_TLA §4,
/// Phase 4 — "Unify the verifier ledger"). Today only [`Kani`](ProofKind::Kani) is
/// emitted (bounded-local BMC harnesses); the variant exists so the ledger is open to
/// other bounded/SMT verifiers without churning the record shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofKind {
    /// A `#[kani::proof]` harness (bounded model checking of a local data-structure
    /// property). Dormant under stock `cargo` (the harness is `#[cfg(kani)]`); the
    /// ANCHOR is decoupled from the harness so it registers in normal/test builds.
    Kani,
}

/// A proof→spec binding, emitted by the `proof_anchor!` macro (TRUST_NATIVE_TLA §4).
///
/// This is the kani-harness analogue of [`RefinementAnchor`]: it joins a bounded-local
/// proof to the SAME `(machine, action)` namespace the temporal (`ty`) models use, so
/// the gate can emit ONE per-action ledger over both verifiers — `ty` (temporal /
/// conformance) and `kani` (bounded-local). No proof is moved or merged; the anchor
/// just records that the named harness *refers to* that action.
///
/// CRITICAL (the §4 subtlety): the kani harnesses are `#[cfg(kani)]`-gated — dormant
/// under stock `cargo`. An ATTRIBUTE on the harness fn would be stripped (and never
/// register) in normal/test builds. So the `proof_anchor!` macro is a MODULE-LEVEL
/// declarative-macro INVOCATION decoupled from the harness fn (it names the harness by
/// string), gated behind the `spec-anchors` feature exactly like [`RefinementAnchor`].
#[derive(Debug, Clone, Copy)]
pub struct ProofAnchor {
    /// The TLA+ machine name (e.g. `"Ring"`) — matches a `Model::name` (lower/CamelCase
    /// resolved by [`machine_matches`]) or an external module name. Same namespace as
    /// [`RefinementAnchor::machine`].
    pub machine: &'static str,
    /// The model action this proof refers to (e.g. `"Push"`) — must resolve in `machine`
    /// (Ob.1), the SAME obligation refinements satisfy.
    pub action: &'static str,
    /// The `#[kani::proof]` harness fn name (e.g. `"line_count_accurate"`). A diagnostic
    /// label for the ledger; the precise DefId binding is out of scope (mirrors
    /// [`RefinementAnchor::rust_method`]).
    pub proof_name: &'static str,
    /// Which verifier discharges this anchor (always [`ProofKind::Kani`] today).
    pub kind: ProofKind,
    /// Source location `file:line` (proc-macro/`file!()`/`line!()` of the invocation).
    pub location: &'static str,
}

inventory::collect!(RefinementAnchor);
inventory::collect!(Waiver);
inventory::collect!(InvariantAnchor);
inventory::collect!(ProofAnchor);

/// All [`RefinementAnchor`]s linked into the current binary.
pub fn refinements() -> impl Iterator<Item = &'static RefinementAnchor> {
    inventory::iter::<RefinementAnchor>.into_iter()
}

/// All [`Waiver`]s linked into the current binary.
pub fn waivers() -> impl Iterator<Item = &'static Waiver> {
    inventory::iter::<Waiver>.into_iter()
}

/// All [`InvariantAnchor`]s linked into the current binary.
pub fn invariant_anchors() -> impl Iterator<Item = &'static InvariantAnchor> {
    inventory::iter::<InvariantAnchor>.into_iter()
}

/// All [`ProofAnchor`]s linked into the current binary (the kani-harness ledger half).
pub fn proof_anchors() -> impl Iterator<Item = &'static ProofAnchor> {
    inventory::iter::<ProofAnchor>.into_iter()
}

/// Every embedded [`Model`] aterm-spec knows about — the spec→source registry.
///
/// This enumerates ALL `ty_model!`/`derive`-authored models so the closure gate can
/// resolve a `machine` named by an anchor to a registered `SpecModule` (obligation
/// 4) and enumerate a machine's actions (obligation 1 + coverage). Adding a model
/// here makes it a first-class anchor target.
pub fn model_registry() -> Vec<Model> {
    use crate::derive::*;
    vec![
        terminal_modes_model(),
        ring_model(),
        cursor_model(),
        subscribe_model(),
        transact_model(),
        kernel_model(),
        snapshot_model(),
        read_image_seq_model(),
        // A7 (WS-G): the PTY-master fd-lifecycle ownership discipline — drift-free
        // twin of FdLifecycle.tla, anchored to aterm-session/src/sink.rs.
        fd_lifecycle_model(),
        // WS-G: spawn-time locale guarantee — the child always runs under a UTF-8
        // LC_CTYPE. Abstract twin of aterm_pty::resolve_spawn_locale (real-code
        // binding in aterm-pty's spawn_locale_conformance test). Proves-and-catches.
        spawn_locale_model(),
        evict_full_model(),
        tier_residency_model(),
        recording_model(),
        coalesce_model(),
        window_routing_model(),
        // Introspection / recursive-stacking control plane (audit findings M1/M2/S1).
        dispatch_complete_model(),
        relay_teardown_model(),
        proxy_registry_model(),
        // Liveness twin: forward-handshake deadlock-freedom (the drain_buffered class).
        forward_handshake_model(),
        // Generalized error-class models (F1 info-flow, ordering, reply-fidelity).
        capability_secrecy_model(),
        publish_ordering_model(),
        reply_fidelity_model(),
        // Capability-layer audit: the trust core's authorization-soundness predicate.
        authorize_soundness_model(),
        // Deep-nesting safety: forwarding needs Owner scope (no transitive authority).
        no_transitive_authority_model(),
        // GUI native-chrome safety: split-pane tree integrity + session-pool refcount
        // accounting (the Tier-1 conformance + #[refines] anchors live in aterm-gui).
        pane_tree_model(),
        session_pool_model(),
        // Native titlebar tab-strip parity: the NSSegmentedControl mirror discipline
        // (seg_count==count, selected==active). Tier-1 conformance + #[refines] anchors
        // live in aterm-gui (projects the strip lane from WindowState::strip_shadow).
        tab_strip_model(),
    ]
}

/// A registered spec module — an embedded [`Model`] or a parsed external `.tla`.
/// Obligation 4 (machine resolves) is satisfied by EITHER variant.
pub enum SpecModule {
    /// An embedded `ty_model!`/`derive` model (the default, drift-free).
    Embedded(Model),
    /// A parsed external `.tla` (full-TLA+ design specs; ISOLATION family).
    External(TlaSpec),
}

impl SpecModule {
    /// The machine name this module declares.
    pub fn name(&self) -> &str {
        match self {
            SpecModule::Embedded(m) => m.name,
            SpecModule::External(t) => &t.module_name,
        }
    }

    /// The names a REFINEMENT / PROOF / WAIVER anchor may RESOLVE to for obligation 1
    /// ("action exists"). This is the ACTION namespace — for an embedded model its
    /// `Action` names; for an external `.tla` the `Next` disjuncts ONLY (the
    /// [`coverage_actions`](Self::coverage_actions) set), NOT every top-level def.
    ///
    /// TRUST_VACUITY_GATE §2.4 (finding 4): the External arm previously returned
    /// `t.actions` (ALL top-level defs — `Init`/`TypeOK`/named constants/invariants),
    /// so an external `#[refines]`/`proof_anchor!` aimed at a non-`Next` def like
    /// `TypeOK` OVER-resolved and was wrongly accepted. The lowered Trust artifact is
    /// already strict here (it emits only `coverage_actions()` and Ob.1-checks against
    /// that — the L3 lock), so the in-Rust gate was the looser of the two. Narrowing
    /// this to `coverage_actions()` ALIGNS the in-Rust gate with Trust's
    /// already-strict artifact: a `#[refines]`/`proof_anchor!` naming `Init`/`TypeOK`
    /// now fails Ob.1 in BOTH paths. `#[spec_invariant]` keeps the full def set via the
    /// separate [`invariant_names`](Self::invariant_names) (invariants legitimately
    /// name non-`Next` defs).
    pub fn action_names(&self) -> BTreeSet<String> {
        // For an embedded model the action set IS the coverage set; for an external
        // `.tla` this is now the `Next` disjuncts only (the L3-locked behavior).
        self.coverage_actions()
    }

    /// The names a `#[spec_invariant]` `id` may RESOLVE to (obligation 1 for the
    /// INVARIANT arm ONLY). This is the full top-level definition set — for an
    /// embedded model its action names; for an external `.tla` EVERY top-level
    /// definition (`t.actions`), because an invariant legitimately names a non-`Next`
    /// def like `TypeOK`/`Confined`.
    ///
    /// TRUST_VACUITY_GATE §2.4: this is the deliberately-WIDER set used ONLY by the
    /// `#[spec_invariant]` id arm, kept separate from [`action_names`](Self::action_names)
    /// (the Next-only set the refinement/proof/waiver arms use) so narrowing the action
    /// set does not break invariants that name `TypeOK`-style defs.
    pub fn invariant_names(&self) -> BTreeSet<String> {
        match self {
            SpecModule::Embedded(m) => m.anchors().map(|(_, a)| a.to_string()).collect(),
            SpecModule::External(t) => t.actions.clone(),
        }
    }

    /// The real ACTION set used for the COVERAGE obligation (obligation 3): every
    /// action must be bound-or-waived for an actively-bound machine. For an embedded
    /// model this is its `Action` names (same as [`action_names`](Self::action_names)).
    /// For an external `.tla` it is the disjuncts of `Next == …` ONLY — NOT every
    /// top-level def, so coverage never demands a `#[refines]` for `Init`/`Spec`/
    /// `TypeOK`/an invariant/a named constant (which are not actions). When a spec has
    /// no parseable `Next` disjuncts (defensive), fall back to the full def set so
    /// coverage cannot be vacuously satisfied.
    pub fn coverage_actions(&self) -> BTreeSet<String> {
        match self {
            SpecModule::Embedded(m) => m.anchors().map(|(_, a)| a.to_string()).collect(),
            SpecModule::External(t) => {
                if t.next_actions.is_empty() {
                    t.actions.clone()
                } else {
                    t.next_actions.clone()
                }
            }
        }
    }
}

/// Whether an anchor's `machine` string resolves to a `SpecModule` named `name`.
///
/// Anchors use a lower_snake/lowercase convention (`"terminal_modes"`,
/// `"window_routing"`, `"ring"`) while a `Model::name`/MODULE is CamelCase
/// (`"TerminalModes"`, `"WindowRouting"`, `"Ring"`). We match case-insensitively
/// after stripping `_`, so `"terminal_modes"` ⟺ `"TerminalModes"`.
pub fn machine_matches(anchor_machine: &str, module_name: &str) -> bool {
    fn norm(s: &str) -> String {
        s.chars()
            .filter(|c| *c != '_')
            .flat_map(|c| c.to_lowercase())
            .collect()
    }
    norm(anchor_machine) == norm(module_name)
}

/// A single obligation failure (for a readable aggregate error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureViolation {
    /// Which of the four §2.2 obligations was violated (1, 3, or 4 here; 2 —
    /// "symbol resolves to a live DefId" — needs the Trust IR lowering of Phase 3,
    /// so it is out of scope for the Phase-0 aterm-local gate and not asserted).
    pub obligation: u8,
    pub message: String,
}

/// A per-machine coverage line for the report (printed for every active machine,
/// and surfaced for the non-active embedded models so ratios are visible — the
/// "REPORT the rest" half of obligation 3).
#[derive(Debug, Clone)]
pub struct MachineCoverage {
    pub machine: String,
    pub total_actions: usize,
    pub bound: BTreeSet<String>,
    pub waived: BTreeSet<String>,
    pub uncovered: BTreeSet<String>,
    pub active: bool,
}

impl MachineCoverage {
    pub fn ratio(&self) -> f64 {
        if self.total_actions == 0 {
            return 1.0;
        }
        (self.bound.len() + self.waived.len()) as f64 / self.total_actions as f64
    }
}

/// The outcome of [`check_closure`]: the violations (empty == green) and the
/// per-machine coverage ledger (for reporting).
pub struct ClosureReport {
    pub violations: Vec<ClosureViolation>,
    pub coverage: Vec<MachineCoverage>,
}

impl ClosureReport {
    /// Whether the closure holds (no violations).
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// One row of the UNIFIED VERIFIER LEDGER (TRUST_NATIVE_TLA §4, Phase 4): for a single
/// `(machine, action)`, which verifier(s) discharge it — `ty` (temporal: a `#[refines]`
/// binding drives Tier-0/Tier-1 `ty` over the model action) and/or `kani` (bounded-local:
/// a `proof_anchor!`'d harness refers to it). This is the "single coverage ledger over
/// both verifiers" the design calls for — no proof is merged; the IR just learns they
/// refer to the same action.
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    /// Canonical machine name (the resolved `SpecModule.name`).
    pub machine: String,
    /// The model action.
    pub action: String,
    /// Discharged temporally: ≥1 `#[refines]` (or a `#[spec_unmodeled]` waiver) binds
    /// this action to the `ty`-checked model.
    pub ty: bool,
    /// Discharged bounded-locally: ≥1 `proof_anchor!`'d kani harness refers to it.
    pub kani: bool,
    /// The kani harness names that refer to this action (for the report detail).
    pub proofs: BTreeSet<String>,
}

impl LedgerEntry {
    /// The `ty=✓/–  kani=✓/–  <machine>::<action>` line for the gate report.
    pub fn render(&self) -> String {
        let mark = |b: bool| if b { "✓" } else { "–" };
        format!(
            "ty={}  kani={}  {}::{}",
            mark(self.ty),
            mark(self.kani),
            self.machine,
            self.action
        )
    }
}

/// Build the per-`(machine, action)` UNIFIED VERIFIER LEDGER over the registered
/// `modules`, the collected `refinements`/`waivers` (the `ty`/temporal half) and
/// `proof_anchors` (the `kani`/bounded-local half) (TRUST_NATIVE_TLA §4, Phase 4).
///
/// Every action of every registered machine gets a row; `ty` is set iff a refinement
/// (or a machine+action waiver) binds it, `kani` iff a `proof_anchor!`'d harness names
/// it. Anchors whose `machine` resolves to no module are silently skipped here — they are
/// already flagged as Ob.4 violations by [`check_closure`], so the ledger never invents a
/// row for a dangling machine.
pub fn verifier_ledger(modules: &[SpecModule]) -> Vec<LedgerEntry> {
    // Seed a row for every (canonical machine, action).
    let mut rows: BTreeMap<(String, String), LedgerEntry> = BTreeMap::new();
    for m in modules {
        let canon = m.name().to_string();
        for action in m.action_names() {
            rows.entry((canon.clone(), action.clone()))
                .or_insert_with(|| LedgerEntry {
                    machine: canon.clone(),
                    action: action.clone(),
                    ty: false,
                    kani: false,
                    proofs: BTreeSet::new(),
                });
        }
    }

    let resolve = |anchor_machine: &str| -> Option<String> {
        modules
            .iter()
            .find(|m| machine_matches(anchor_machine, m.name()))
            .map(|m| m.name().to_string())
    };

    // ty half: refinements and machine+action waivers mark `ty` for their action.
    for r in refinements() {
        if let Some(canon) = resolve(r.machine)
            && let Some(e) = rows.get_mut(&(canon, r.action.to_string()))
        {
            e.ty = true;
        }
    }
    for w in waivers() {
        if w.machine.is_empty() || w.action.is_empty() {
            continue;
        }
        if let Some(canon) = resolve(w.machine)
            && let Some(e) = rows.get_mut(&(canon, w.action.to_string()))
        {
            e.ty = true;
        }
    }

    // kani half: each proof anchor marks `kani` and records the harness name.
    for p in proof_anchors() {
        if let Some(canon) = resolve(p.machine)
            && let Some(e) = rows.get_mut(&(canon, p.action.to_string()))
        {
            e.kani = true;
            e.proofs.insert(p.proof_name.to_string());
        }
    }

    rows.into_values().collect()
}

/// Enforce the four bidirectional obligations of TRUST_NATIVE_TLA §2.2 over the
/// given `SpecModule`s, the collected `refinements`, `waivers`, and
/// `invariant_anchors`. This is the runnable `trust-spec-link` (Phase 0).
///
/// Obligations enforced:
///   1. **Action exists** — every `refines`/`spec_invariant` action (and every
///      waiver action, when present) names a real definition in its module.
///   3. **Coverage** — for every machine that has ≥1 refinement (an *active*
///      machine), every model `Action` is bound-or-waived, i.e. `ratio == 1.0`.
///   4. **Machine exists** — every `machine` named by any anchor resolves to a
///      registered `SpecModule` (embedded or external). Catches a dangling machine.
///
/// **Coverage scoping (deliberate).** Requiring `ratio == 1.0` for ALL registered
/// models would paint a sea of red the moment any model (e.g. the kernel-family
/// twins, or the ISOLATION external specs) lacks a `#[refines]` handler — which is
/// expected today (they are bound via Tier-1 conformance / Phase 2, not via the
/// terminal_modes-style per-method `#[refines]`). So the `== 1.0` requirement is
/// scoped to ACTIVELY-BOUND machines (≥1 refinement), and the ratios of the rest
/// are merely REPORTED (their `MachineCoverage.active == false`). This is exactly
/// the §2.2 obligation-3 intent: total coverage where binding is claimed.
///
/// Obligation 2 ("symbol resolves to a live DefId") is NOT enforced here: it needs
/// the `trust-ir` symbol resolution of Phase 3. The Phase-0 aterm-local gate proves
/// 1/3/4 (the linkage/coverage closure); behavioural alignment is the separate
/// Tier-1 conformance layer (already green for window_routing).
/// One module's three indexed namespaces, keyed by its declared machine name:
/// `(machine name, action set, invariant-def set, coverage-action set)`. They
/// coincide for embedded models and diverge for external `.tla` (see [`check_closure`]).
type ModuleActionIndex = (String, BTreeSet<String>, BTreeSet<String>, BTreeSet<String>);

pub fn check_closure(modules: &[SpecModule]) -> ClosureReport {
    let mut violations = Vec::new();

    // Index modules by THREE namespaces (TRUST_VACUITY_GATE §2.4):
    //   * the ACTION set (`action_names`, now Next-only for external) — the
    //     refinement/proof/waiver Ob.1 resolves against this;
    //   * the INVARIANT-def set (`invariant_names`, the full top-level def set for
    //     external) — the `#[spec_invariant]` id arm resolves against this ONLY;
    //   * the coverage-action set (`coverage_actions`) — the Ob.3 coverage check.
    // For embedded models all three coincide; for external `.tla` the action and
    // coverage sets are the `Next` disjuncts while the invariant set is the full def
    // set (so an invariant naming `TypeOK` resolves but a `#[refines]` naming it does
    // NOT — the alignment with Trust's already-strict artifact).
    // (machine name as declared, action set, invariant-def set, coverage-action set)
    let module_actions: Vec<ModuleActionIndex> = modules
        .iter()
        .map(|m| {
            (
                m.name().to_string(),
                m.action_names(),
                m.invariant_names(),
                m.coverage_actions(),
            )
        })
        .collect();

    let resolve = |anchor_machine: &str| -> Option<&ModuleActionIndex> {
        module_actions
            .iter()
            .find(|(name, _, _, _)| machine_matches(anchor_machine, name))
    };

    // ---- Obligation 4 + 1 for refinements ----
    for r in refinements() {
        match resolve(r.machine) {
            None => violations.push(ClosureViolation {
                obligation: 4,
                message: format!(
                    "#[refines] at {} ({}) names machine `{}` which resolves to NO registered \
                     SpecModule (embedded Model or external .tla). Either author the model or \
                     fix the machine name.",
                    r.location, r.rust_method, r.machine
                ),
            }),
            Some((_, actions, _, _)) => {
                if !actions.contains(r.action) {
                    violations.push(ClosureViolation {
                        obligation: 1,
                        message: format!(
                            "#[refines] at {} ({}) names action `{}` which does NOT exist in \
                             machine `{}`. Known actions: {:?}",
                            r.location, r.rust_method, r.action, r.machine, actions
                        ),
                    });
                }
            }
        }
    }

    // ---- Obligation 4 + 1 for invariant anchors (machine optional) ----
    for inv in invariant_anchors() {
        if inv.machine.is_empty() {
            continue;
        }
        match resolve(inv.machine) {
            None => violations.push(ClosureViolation {
                obligation: 4,
                message: format!(
                    "#[spec_invariant] at {} ({}) names machine `{}` which resolves to NO \
                     registered SpecModule.",
                    inv.location, inv.rust_method, inv.machine
                ),
            }),
            Some((_, _, inv_defs, _)) => {
                // An invariant anchor's `id` should name a top-level DEFINITION in the
                // module (invariants legitimately name non-`Next` defs like `TypeOK`),
                // so it resolves against the WIDER `invariant_names()` set — NOT the
                // Next-only action set the refinement/proof arms use (§2.4).
                if !inv_defs.contains(inv.id) {
                    violations.push(ClosureViolation {
                        obligation: 1,
                        message: format!(
                            "#[spec_invariant] at {} ({}) names id `{}` which does NOT exist in \
                             machine `{}`. Known definitions: {:?}",
                            inv.location, inv.rust_method, inv.id, inv.machine, inv_defs
                        ),
                    });
                }
            }
        }
    }

    // ---- Obligation 4 + 1 for waivers that name a machine/action ----
    for w in waivers() {
        if w.machine.is_empty() {
            // Bare `reason="…"` waiver (bypass-setter form): not tied to a machine,
            // nothing to resolve. It cannot discharge coverage either.
            continue;
        }
        match resolve(w.machine) {
            None => violations.push(ClosureViolation {
                obligation: 4,
                message: format!(
                    "#[spec_unmodeled] at {} ({}) names machine `{}` which resolves to NO \
                     registered SpecModule.",
                    w.location, w.rust_method, w.machine
                ),
            }),
            Some((_, actions, _, _)) => {
                if !w.action.is_empty() && !actions.contains(w.action) {
                    violations.push(ClosureViolation {
                        obligation: 1,
                        message: format!(
                            "#[spec_unmodeled] at {} ({}) names action `{}` which does NOT exist \
                             in machine `{}`. Known actions: {:?}",
                            w.location, w.rust_method, w.action, w.machine, actions
                        ),
                    });
                }
            }
        }
    }

    // ---- Obligation 4 + 1 for PROOF anchors (TRUST_NATIVE_TLA §4, Phase 4) ----
    // A `proof_anchor!`'d kani harness joins the SAME (machine, action) namespace as a
    // refinement, so it must satisfy the SAME structural obligations: its `machine`
    // resolves to a registered SpecModule (Ob.4) and its `action` exists in that machine
    // (Ob.1). This is the teeth of the unified ledger — a proof_anchor naming a bogus
    // action fails the gate exactly like a #[refines] would. (A proof anchor does NOT
    // discharge the coverage obligation — kani is bounded-LOCAL, not the temporal binding
    // coverage demands — so it is intentionally NOT folded into `bound` below.)
    for p in proof_anchors() {
        match resolve(p.machine) {
            None => violations.push(ClosureViolation {
                obligation: 4,
                message: format!(
                    "proof_anchor! at {} (proof `{}`) names machine `{}` which resolves to NO \
                     registered SpecModule (embedded Model or external .tla). Either author the \
                     model or fix the machine name.",
                    p.location, p.proof_name, p.machine
                ),
            }),
            Some((_, actions, _, _)) => {
                if !actions.contains(p.action) {
                    violations.push(ClosureViolation {
                        obligation: 1,
                        message: format!(
                            "proof_anchor! at {} (proof `{}`) names action `{}` which does NOT \
                             exist in machine `{}`. Known actions: {:?}",
                            p.location, p.proof_name, p.action, p.machine, actions
                        ),
                    });
                }
            }
        }
    }

    // ---- Obligation 3: coverage over active machines, report over the rest ----
    // Bind/waive sets keyed by the MODULE's declared name (canonical).
    let mut bound: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut waived: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for r in refinements() {
        if let Some((name, _, _, _)) = resolve(r.machine) {
            bound
                .entry(name.clone())
                .or_default()
                .insert(r.action.to_string());
        }
    }
    for w in waivers() {
        if w.machine.is_empty() || w.action.is_empty() {
            continue;
        }
        if let Some((name, _, _, _)) = resolve(w.machine) {
            waived
                .entry(name.clone())
                .or_default()
                .insert(w.action.to_string());
        }
    }

    let mut coverage = Vec::new();
    // Coverage is computed over the real coverage-action set (4th tuple element), NOT
    // the full def set — so an external machine is "fully covered" once every `Next`
    // disjunct is bound-or-waived, without demanding a `#[refines]` for `Init`/`TypeOK`.
    for (name, _actions, _inv_defs, cov_actions) in &module_actions {
        let actions = cov_actions;
        let b = bound.get(name).cloned().unwrap_or_default();
        let wv = waived.get(name).cloned().unwrap_or_default();
        let active = !b.is_empty();
        let covered: BTreeSet<String> = b.union(&wv).cloned().collect();
        let uncovered: BTreeSet<String> = actions.difference(&covered).cloned().collect();
        let mc = MachineCoverage {
            machine: name.clone(),
            total_actions: actions.len(),
            bound: b,
            waived: wv,
            uncovered: uncovered.clone(),
            active,
        };
        // Obligation 3 (scoped): an active machine must be fully bound-or-waived.
        if active && !uncovered.is_empty() {
            violations.push(ClosureViolation {
                obligation: 3,
                message: format!(
                    "machine `{}` is actively bound ({} refinement(s)) but {} action(s) are \
                     neither bound nor waived: {:?}. Add a #[refines] or a \
                     #[spec_unmodeled(machine=…, action=…, reason=…)] for each. (ratio = {:.3})",
                    name,
                    mc.bound.len(),
                    uncovered.len(),
                    uncovered,
                    mc.ratio()
                ),
            });
        }
        coverage.push(mc);
    }

    ClosureReport {
        violations,
        coverage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::{cursor_model, ring_model};
    use crate::tla_check::TlaSpec;

    /// TRUST_VACUITY_GATE §2.4 LOCK (finding 4): an EXTERNAL module's `action_names()`
    /// (the refinement/proof/waiver Ob.1 set) is the `Next` disjuncts ONLY, while
    /// `invariant_names()` (the `#[spec_invariant]` id set) is the full top-level def
    /// set. So an anchor naming `TypeOK` fails Ob.1 but an invariant naming `TypeOK`
    /// resolves — aligning the in-Rust gate with Trust's already-strict artifact (L3).
    #[test]
    fn external_action_names_is_next_only_invariant_names_is_full_set() {
        let tla = "---- MODULE Fix ----\nVARIABLES x\nInit == x = 0\nApply == x' = 1\n\
                   Next == Apply\nTypeOK == x \\in {0, 1}\n====\n";
        let spec = TlaSpec::parse_str(tla, "Fix.tla").expect("parse");
        let m = SpecModule::External(spec);
        // Next-only action set: Apply yes, TypeOK no.
        assert!(
            m.action_names().contains("Apply"),
            "Apply is a Next disjunct"
        );
        assert!(
            !m.action_names().contains("TypeOK"),
            "TypeOK must NOT be a valid action target (Next-only) — the finding-4 narrowing"
        );
        // Wider invariant-def set: TypeOK resolves as an invariant id.
        assert!(
            m.invariant_names().contains("TypeOK"),
            "TypeOK must resolve as an invariant id (the full def set)"
        );
        assert!(
            m.invariant_names().contains("Apply"),
            "the def set is a superset of actions"
        );
    }

    // A real proof_anchor! invocation at module level (the kani-half of the ledger).
    // Submitted into the inventory slice for this crate's own unit-test binary, so the
    // collection / resolution / ledger path is exercised in aterm-spec itself (NOT inside
    // any #[cfg(kani)] block — that is the whole point of the decoupling).
    crate::proof_anchor!(
        machine = "Ring",
        action = "Push",
        proof = "aterm_spec_self_test_ring_push"
    );
    crate::proof_anchor!(
        machine = "Cursor",
        action = "Grow",
        proof = "aterm_spec_self_test_cursor_grow"
    );

    #[test]
    fn proof_anchor_macro_submits_a_collectable_kani_record() {
        let mine: Vec<_> = proof_anchors()
            .filter(|p| p.proof_name.starts_with("aterm_spec_self_test_"))
            .collect();
        assert_eq!(
            mine.len(),
            2,
            "both self-test proof anchors should be collected"
        );
        assert!(mine.iter().all(|p| p.kind == ProofKind::Kani));
        let ring = mine
            .iter()
            .find(|p| p.machine == "Ring")
            .expect("ring anchor");
        assert_eq!(ring.action, "Push");
        assert!(
            ring.location.contains("xref.rs"),
            "location is file:line: {}",
            ring.location
        );
    }

    #[test]
    fn proof_anchor_machine_action_resolves_under_the_closure() {
        // The proof anchors above name (Ring, Push) and (Cursor, Grow) — both REAL model
        // actions, so check_closure must NOT flag them (Ob.1/Ob.4 satisfied). We isolate
        // the proof-anchor obligations by checking no violation mentions a self-test proof.
        let modules = vec![
            SpecModule::Embedded(ring_model()),
            SpecModule::Embedded(cursor_model()),
        ];
        let report = check_closure(&modules);
        for v in &report.violations {
            assert!(
                !v.message.contains("aterm_spec_self_test_"),
                "a VALID self-test proof anchor was wrongly flagged: [Ob.{}] {}",
                v.obligation,
                v.message
            );
        }
    }

    #[test]
    fn verifier_ledger_marks_kani_for_proof_anchored_actions() {
        let modules = vec![
            SpecModule::Embedded(ring_model()),
            SpecModule::Embedded(cursor_model()),
        ];
        let ledger = verifier_ledger(&modules);
        // Ring::Push is proof-anchored (kani=✓) by the self-test above.
        let ring_push = ledger
            .iter()
            .find(|e| e.machine == "Ring" && e.action == "Push")
            .expect("Ring::Push row");
        assert!(
            ring_push.kani,
            "Ring::Push must be kani-discharged in the ledger"
        );
        assert!(ring_push.proofs.contains("aterm_spec_self_test_ring_push"));
        // Cursor::Deliver is NOT proof-anchored here (kani=–).
        let deliver = ledger
            .iter()
            .find(|e| e.machine == "Cursor" && e.action == "Deliver")
            .expect("Cursor::Deliver row");
        assert!(
            !deliver.kani,
            "Cursor::Deliver has no proof anchor — kani must be –"
        );
        // The render is the per-(machine,action) ledger line shape.
        assert!(
            ring_push.render().contains("kani=✓  Ring::Push"),
            "{}",
            ring_push.render()
        );
        assert!(
            deliver.render().contains("kani=–  Cursor::Deliver"),
            "{}",
            deliver.render()
        );
    }
}
