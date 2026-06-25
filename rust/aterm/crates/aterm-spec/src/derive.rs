// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! Derived TLA+: one source of truth for a state machine, two faithful backends.
//!
//! The specs in `aterm-spec-models/specs/` are hand-written `.tla` files — they
//! can drift from the code. This module is the *derivation half* of
//! `docs/RFC-ty-embed-derived-tla.md`: a [`Model`] is a first-class Rust value
//! describing a bounded state machine (constants, variables, guarded actions,
//! invariants) over a small expression language [`Expr`]. From that ONE source we
//! generate BOTH:
//!
//!   - [`Model::to_tla`] / [`Model::to_cfg`] — a complete, `ty`-checkable TLA+
//!     module + config (the embedded spec), and
//!   - [`Model::fire`] / [`Model::successors`] — the executable transition
//!     semantics (a real interpreter, using TLA+ primed semantics: every
//!     right-hand side is evaluated against the pre-state, then applied).
//!     `successors` enumerates the existential fan-out of a nondeterministic
//!     ([`Expr::InRange`]) action; `fire` applies its canonical representative.
//!
//! Because both backends consume the same `Expr` trees, a change to the model
//! changes the spec AND the executable semantics together — they cannot drift.
//! The generated spec is exhaustively checked by `ty` (Tier 0); the same model
//! drives conformance against real code (Tier 1, see
//! `aterm-buffer/tests/conformance_eventlog.rs`).
//!
//! The expression language is intentionally small (what the bounded-ring model
//! needs); it grows as real models demand. The translation is mechanical:
//! `<=` ⇒ `=<`, `if/else` ⇒ `IF/THEN/ELSE`, `&&`/`||` ⇒ `/\`/`\/`.

use std::collections::BTreeMap;

/// A value in the model's (bounded-integer / boolean) semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl Value {
    fn as_int(self) -> i64 {
        match self {
            Value::Int(i) => i,
            Value::Bool(_) => panic!("expected Int, got Bool — model type error"),
        }
    }
    fn as_bool(self) -> bool {
        match self {
            Value::Bool(b) => b,
            Value::Int(_) => panic!("expected Bool, got Int — model type error"),
        }
    }
}

/// A small expression language shared by the TLA+ generator and the interpreter.
/// `Var`/`ConstRef` both resolve from the evaluation environment; arithmetic and
/// comparison map 1:1 to TLA+.
#[derive(Debug, Clone)]
pub enum Expr {
    /// An integer literal.
    Int(i64),
    /// A state variable reference (resolves from the current state).
    Var(&'static str),
    /// A TLA+ CONSTANT reference (resolves from the model's constants).
    ConstRef(&'static str),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    /// `a > b` (boolean).
    Gt(Box<Expr>, Box<Expr>),
    /// `a <= b` (boolean) — emits TLA+ `=<`.
    Le(Box<Expr>, Box<Expr>),
    /// `a = b` (boolean, integer operands) — emits TLA+ `=`.
    Eq(Box<Expr>, Box<Expr>),
    /// `a \/ b` (boolean disjunction) — emits parenthesized TLA+ `(a \/ b)` so it
    /// composes correctly when used as an action guard (which is conjoined with
    /// the update conjuncts, and `/\` binds tighter than `\/`).
    Or(Box<Expr>, Box<Expr>),
    /// `a /\ b` (boolean conjunction) — emits TLA+ `a /\ b` (unparenthesized; `/\`
    /// is associative with the action-level conjunction it joins, so a guard built
    /// from `And` composes correctly without parens).
    And(Box<Expr>, Box<Expr>),
    /// `IF cond THEN a ELSE b` (cond boolean; arms integer).
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `a <=> b` (boolean iff) — emits parenthesized `(a <=> b)`.
    Iff(Box<Expr>, Box<Expr>),
    /// `\A n \in lo..hi : body` (universal quantifier; `body` boolean, may
    /// reference the bound index by name). Scalar bodies are interpreter-evaluable.
    Forall(&'static str, Box<Expr>, Box<Expr>, Box<Expr>),
    // ---- function-valued (TLA+ generation only; see eval note) ----
    /// `fn[index]` — access a function-valued (`[1..N -> BOOLEAN]`) state variable.
    FnAccess(&'static str, Box<Expr>),
    /// `[fn EXCEPT ![index] = value]` — a point update of a function variable.
    Except(&'static str, Box<Expr>, Box<Expr>),
    /// `[n \in lo..hi |-> body]` — a function comprehension (`body` may reference
    /// the bound index by name); used for whole-function updates.
    Comprehension(&'static str, Box<Expr>, Box<Expr>, Box<Expr>),
    /// `TRUE` / `FALSE`.
    Bool(bool),
    /// `a # b` (integer inequality).
    Neq(Box<Expr>, Box<Expr>),
    /// `lo..hi` — a bounded integer SET, the only NONDETERMINISTIC construct. As an
    /// action update RHS it renders `var' \in lo..hi` (not `var' = ...`): the action
    /// admits ANY value in `lo..=hi` for that variable. `ty` checks the full
    /// existential fan-out exhaustively at Tier-0; the executable twin enumerates it
    /// via [`Model::successors`]. It is set-valued, so it has no scalar [`Expr::eval`]
    /// — it is meaningful ONLY as a top-level update RHS. Used where the scalar
    /// projection cannot pin a single next value (e.g. closing the frontmost window
    /// re-points `frontmost` to *some* surviving allocated id, not a computable one).
    InRange(Box<Expr>, Box<Expr>),
}

/// Convenience constructors (keep model definitions terse).
pub fn int(i: i64) -> Expr {
    Expr::Int(i)
}
pub fn var(n: &'static str) -> Expr {
    Expr::Var(n)
}
pub fn cst(n: &'static str) -> Expr {
    Expr::ConstRef(n)
}
pub fn add(a: Expr, b: Expr) -> Expr {
    Expr::Add(Box::new(a), Box::new(b))
}
pub fn sub(a: Expr, b: Expr) -> Expr {
    Expr::Sub(Box::new(a), Box::new(b))
}
pub fn gt(a: Expr, b: Expr) -> Expr {
    Expr::Gt(Box::new(a), Box::new(b))
}
pub fn le(a: Expr, b: Expr) -> Expr {
    Expr::Le(Box::new(a), Box::new(b))
}
pub fn eq(a: Expr, b: Expr) -> Expr {
    Expr::Eq(Box::new(a), Box::new(b))
}
pub fn or_(a: Expr, b: Expr) -> Expr {
    Expr::Or(Box::new(a), Box::new(b))
}
pub fn and_(a: Expr, b: Expr) -> Expr {
    Expr::And(Box::new(a), Box::new(b))
}
pub fn if_(c: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If(Box::new(c), Box::new(a), Box::new(b))
}
pub fn iff(a: Expr, b: Expr) -> Expr {
    Expr::Iff(Box::new(a), Box::new(b))
}
pub fn forall(idx: &'static str, lo: Expr, hi: Expr, body: Expr) -> Expr {
    Expr::Forall(idx, Box::new(lo), Box::new(hi), Box::new(body))
}
pub fn fn_access(f: &'static str, index: Expr) -> Expr {
    Expr::FnAccess(f, Box::new(index))
}
pub fn except(f: &'static str, index: Expr, value: Expr) -> Expr {
    Expr::Except(f, Box::new(index), Box::new(value))
}
pub fn comprehension(idx: &'static str, lo: Expr, hi: Expr, body: Expr) -> Expr {
    Expr::Comprehension(idx, Box::new(lo), Box::new(hi), Box::new(body))
}
pub fn bool_lit(b: bool) -> Expr {
    Expr::Bool(b)
}
pub fn neq(a: Expr, b: Expr) -> Expr {
    Expr::Neq(Box::new(a), Box::new(b))
}
/// `lo..hi` — a bounded integer set, the nondeterministic update RHS. See
/// [`Expr::InRange`]: as an action update it admits any `lo..=hi` for the variable.
pub fn in_range(lo: Expr, hi: Expr) -> Expr {
    Expr::InRange(Box::new(lo), Box::new(hi))
}

impl Expr {
    /// Evaluate against an environment (state variables + constants).
    pub fn eval(&self, env: &BTreeMap<&'static str, i64>) -> Value {
        match self {
            Expr::Int(i) => Value::Int(*i),
            Expr::Var(n) | Expr::ConstRef(n) => Value::Int(
                *env.get(n)
                    .unwrap_or_else(|| panic!("unbound identifier `{n}` in model evaluation")),
            ),
            Expr::Add(a, b) => Value::Int(a.eval(env).as_int() + b.eval(env).as_int()),
            Expr::Sub(a, b) => Value::Int(a.eval(env).as_int() - b.eval(env).as_int()),
            Expr::Gt(a, b) => Value::Bool(a.eval(env).as_int() > b.eval(env).as_int()),
            Expr::Le(a, b) => Value::Bool(a.eval(env).as_int() <= b.eval(env).as_int()),
            Expr::Eq(a, b) => Value::Bool(a.eval(env).as_int() == b.eval(env).as_int()),
            Expr::Neq(a, b) => Value::Bool(a.eval(env).as_int() != b.eval(env).as_int()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Or(a, b) => Value::Bool(a.eval(env).as_bool() || b.eval(env).as_bool()),
            Expr::And(a, b) => Value::Bool(a.eval(env).as_bool() && b.eval(env).as_bool()),
            Expr::If(c, a, b) => {
                if c.eval(env).as_bool() {
                    a.eval(env)
                } else {
                    b.eval(env)
                }
            }
            Expr::Iff(a, b) => Value::Bool(a.eval(env).as_bool() == b.eval(env).as_bool()),
            Expr::Forall(idx, lo, hi, body) => {
                let (l, h) = (lo.eval(env).as_int(), hi.eval(env).as_int());
                let mut e = env.clone();
                for n in l..=h {
                    e.insert(idx, n);
                    if !body.eval(&e).as_bool() {
                        return Value::Bool(false);
                    }
                }
                Value::Bool(true)
            }
            // Function-valued exprs are TLA+-generation only: the faithful models
            // that use them are Tier-0 ty-checked, not run through this scalar
            // interpreter (whose env is integer-valued). A scalar model never
            // contains these, so these arms are unreachable in practice.
            Expr::FnAccess(..) | Expr::Except(..) | Expr::Comprehension(..) => {
                panic!(
                    "function-valued Expr is TLA+-generation only (Tier-0 ty-checked, not interpreter-evaluable)"
                )
            }
            // `lo..hi` is a SET, not a scalar — there is no single value to return.
            // The interpreter consumes it only as an update RHS, where
            // `Model::successors` matches it and enumerates `lo..=hi` directly
            // (evaluating its bounds), so this arm is never reached in practice.
            Expr::InRange(..) => panic!(
                "InRange (`lo..hi`) is set-valued; it is meaningful only as a nondeterministic \
                 update RHS — fire the action via `Model::successors`, not scalar `eval`"
            ),
        }
    }

    /// Render as a TLA+ expression. `prime` marks state-variable references with a
    /// trailing `'` (used on action right-hand sides? no — RHS reads pre-state, so
    /// `prime` is false there; it exists for completeness of variable rendering).
    pub fn to_tla(&self) -> String {
        match self {
            Expr::Int(i) => i.to_string(),
            Expr::Var(n) | Expr::ConstRef(n) => (*n).to_string(),
            Expr::Add(a, b) => format!("{} + {}", a.to_tla(), b.to_tla()),
            Expr::Sub(a, b) => format!("{} - {}", a.to_tla(), b.to_tla()),
            Expr::Gt(a, b) => format!("{} > {}", a.to_tla(), b.to_tla()),
            Expr::Le(a, b) => format!("{} =< {}", a.to_tla(), b.to_tla()),
            Expr::Eq(a, b) => format!("{} = {}", a.to_tla(), b.to_tla()),
            Expr::Neq(a, b) => format!("{} # {}", a.to_tla(), b.to_tla()),
            Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            // Parenthesized: `\/` binds looser than the `/\` it is conjoined with.
            Expr::Or(a, b) => format!("({} \\/ {})", a.to_tla(), b.to_tla()),
            Expr::And(a, b) => format!("{} /\\ {}", a.to_tla(), b.to_tla()),
            // Parenthesized: an `IF`'s `ELSE` extends as far right as possible, so an
            // IF-valued update that is NOT the last action conjunct would otherwise
            // swallow the following `/\ ...`.
            Expr::If(c, a, b) => {
                format!(
                    "(IF {} THEN {} ELSE {})",
                    c.to_tla(),
                    a.to_tla(),
                    b.to_tla()
                )
            }
            Expr::Iff(a, b) => format!("({} <=> {})", a.to_tla(), b.to_tla()),
            Expr::Forall(idx, lo, hi, body) => {
                format!(
                    "\\A {idx} \\in {}..{} : {}",
                    lo.to_tla(),
                    hi.to_tla(),
                    body.to_tla()
                )
            }
            Expr::FnAccess(f, index) => format!("{f}[{}]", index.to_tla()),
            Expr::Except(f, index, value) => {
                format!("[{f} EXCEPT ![{}] = {}]", index.to_tla(), value.to_tla())
            }
            Expr::Comprehension(idx, lo, hi, body) => {
                format!(
                    "[{idx} \\in {}..{} |-> {}]",
                    lo.to_tla(),
                    hi.to_tla(),
                    body.to_tla()
                )
            }
            // A bounded set `lo..hi`. The action renderer special-cases an InRange
            // update RHS to emit `var' \in lo..hi`; this standalone form is the set
            // itself, for completeness of rendering.
            Expr::InRange(lo, hi) => format!("{}..{}", lo.to_tla(), hi.to_tla()),
        }
    }
}

/// `var' = expr`: an action's update to one state variable.
#[derive(Debug, Clone)]
pub struct Update {
    pub var: &'static str,
    pub expr: Expr,
}

/// A guarded action (a disjunct of `Next`). Variables not updated stay UNCHANGED.
#[derive(Debug, Clone)]
pub struct Action {
    pub name: &'static str,
    pub guard: Option<Expr>,
    pub updates: Vec<Update>,
}

/// A named safety invariant.
#[derive(Debug, Clone)]
pub struct Invariant {
    pub name: &'static str,
    pub expr: Expr,
}

/// A state variable with its `Init` value.
#[derive(Debug, Clone)]
pub struct StateVar {
    pub name: &'static str,
    pub init: i64,
}

/// A function-valued state variable `[1..range -> BOOLEAN]`, initialized all-FALSE.
/// Used for per-element models (e.g. a ring's live-set) that the scalar projection
/// cannot express. Such models are Tier-0 `ty`-checked (TLA+ generation), not run
/// through the integer interpreter.
#[derive(Debug, Clone)]
pub struct FnVar {
    pub name: &'static str,
    /// Upper bound of the index domain (a CONSTANT name); the domain is `1..range`.
    pub range: &'static str,
}

/// A bounded state machine — the single source for both the TLA+ spec and the
/// executable semantics.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: &'static str,
    pub consts: Vec<(&'static str, i64)>,
    pub vars: Vec<StateVar>,
    /// Function-valued variables (`[1..range -> BOOLEAN]`). Usually empty (scalar
    /// models); non-empty only for per-element Tier-0 models.
    pub fn_vars: Vec<FnVar>,
    pub actions: Vec<Action>,
    pub invariants: Vec<Invariant>,
}

impl Model {
    /// All state-variable names, scalar then function-valued (the order used in
    /// `VARIABLES`, `vars`, and `UNCHANGED`).
    fn all_var_names(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.vars.iter().map(|x| x.name).collect();
        v.extend(self.fn_vars.iter().map(|f| f.name));
        v
    }

    /// Constants as an evaluation environment base.
    fn const_env(&self) -> BTreeMap<&'static str, i64> {
        self.consts.iter().copied().collect()
    }

    /// Generate the complete TLA+ module text (the embedded, `ty`-checkable spec)
    /// with the model's concrete `Init` (variables start at their declared values).
    pub fn to_tla(&self) -> String {
        let const_names: Vec<String> = self.consts.iter().map(|(n, _)| (*n).to_string()).collect();
        let mut init_parts: Vec<String> = self
            .vars
            .iter()
            .map(|v| format!("{} = {}", v.name, v.init))
            .collect();
        // Function vars initialize to the all-FALSE function over `1..range`.
        for f in &self.fn_vars {
            init_parts.push(format!("{} = [n \\in 1..{} |-> FALSE]", f.name, f.range));
        }
        let init = init_parts.join(" /\\ ");
        self.render(&const_names, &init)
    }

    /// Generate the module with a PARAMETERIZED `Init`: each variable starts at a
    /// fresh CONSTANT `<var>_init`. This lets any predecessor state be the start —
    /// used for strict per-transition conformance against real code (Tier 1):
    /// validating a two-step trace `[prev, next]` with `Init` pinned to `prev`
    /// strictly checks that `Next` admits the real `prev -> next` transition.
    pub fn transition_spec(&self) -> String {
        let mut const_names: Vec<String> =
            self.consts.iter().map(|(n, _)| (*n).to_string()).collect();
        for v in &self.vars {
            const_names.push(format!("{}_init", v.name));
        }
        let init = self
            .vars
            .iter()
            .map(|v| format!("{} = {}_init", v.name, v.name))
            .collect::<Vec<_>>()
            .join(" /\\ ");
        self.render(&const_names, &init)
    }

    /// Shared renderer: emit the module given the CONSTANT names and the `Init`
    /// body. Actions, `Next`, `Spec`, and invariants are identical across the
    /// concrete and parameterized-`Init` forms — one source for both.
    fn render(&self, const_names: &[String], init_line: &str) -> String {
        let vars = self.all_var_names();
        let mut s = String::new();
        s.push_str(&format!("---- MODULE {} ----\n", self.name));
        s.push_str("EXTENDS Naturals\n");
        if !const_names.is_empty() {
            s.push_str(&format!("CONSTANT {}\n", const_names.join(", ")));
        }
        s.push_str(&format!("VARIABLES {}\n", vars.join(", ")));
        s.push_str(&format!("vars == << {} >>\n", vars.join(", ")));
        s.push_str(&format!("Init == {init_line}\n"));

        // Actions
        for a in &self.actions {
            let mut conj: Vec<String> = Vec::new();
            if let Some(g) = &a.guard {
                conj.push(g.to_tla());
            }
            for u in &a.updates {
                match &u.expr {
                    // A nondeterministic update: `var' \in lo..hi` (membership), not
                    // `var' = ...` (equality). ty checks every value in the bounded
                    // range as a separate `Next` successor — the existential fan-out.
                    Expr::InRange(lo, hi) => {
                        conj.push(format!("{}' \\in {}..{}", u.var, lo.to_tla(), hi.to_tla()))
                    }
                    _ => conj.push(format!("{}' = {}", u.var, u.expr.to_tla())),
                }
            }
            // UNCHANGED for variables this action does not update.
            let updated: Vec<&str> = a.updates.iter().map(|u| u.var).collect();
            let unchanged: Vec<&str> = vars
                .iter()
                .copied()
                .filter(|v| !updated.contains(v))
                .collect();
            if !unchanged.is_empty() {
                conj.push(format!("UNCHANGED << {} >>", unchanged.join(", ")));
            }
            s.push_str(&format!("{} == {}\n", a.name, conj.join(" /\\ ")));
        }

        let action_names: Vec<&str> = self.actions.iter().map(|a| a.name).collect();
        s.push_str(&format!("Next == {}\n", action_names.join(" \\/ ")));
        s.push_str("Spec == Init /\\ [][Next]_vars\n");

        for inv in &self.invariants {
            s.push_str(&format!("{} == {}\n", inv.name, inv.expr.to_tla()));
        }
        s.push_str("====\n");
        s
    }

    /// Generate the `.cfg`: constant bindings, the specification, and every
    /// invariant. Bounded constants keep `ty check` exhaustive + terminating.
    pub fn to_cfg(&self) -> String {
        self.to_cfg_with(&[])
    }

    /// Like [`to_cfg`](Self::to_cfg) but with constant `overrides` — e.g. flip a
    /// `Buggy` flag to 1 to check that an invariant is non-trivial (the buggy
    /// variant must yield a counterexample), the in-spec analogue of the
    /// `Buggy`-constant convention used by the hand-written specs.
    pub fn to_cfg_with(&self, overrides: &[(&'static str, i64)]) -> String {
        let mut s = String::new();
        for (n, default) in &self.consts {
            let val = overrides
                .iter()
                .find(|(o, _)| o == n)
                .map(|(_, v)| *v)
                .unwrap_or(*default);
            s.push_str(&format!("CONSTANT {n} = {val}\n"));
        }
        s.push_str("SPECIFICATION Spec\n");
        for inv in &self.invariants {
            s.push_str(&format!("INVARIANT {}\n", inv.name));
        }
        s.push_str("CHECK_DEADLOCK FALSE\n");
        s
    }

    /// Like [`to_cfg_with`](Self::to_cfg_with) but with `ty` DEADLOCK / LIVENESS
    /// detection ON (`CHECK_DEADLOCK TRUE`) — the liveness twin of the safety
    /// configs, for request/reply "wedge" models (e.g. the forward handshake).
    ///
    /// This closes the gap the original audit documented: the safety models
    /// verify reachable-bad-STATE properties, but a blocking-call deadlock (the
    /// `drain_buffered` `fill_buf` hang — server parked reading more input while
    /// the client is parked awaiting the reply) is a two-party all-blocked wedge
    /// that only `CHECK_DEADLOCK` catches. The default `to_cfg`/`to_cfg_with` keep
    /// `CHECK_DEADLOCK FALSE`, so the 14 existing models — which legitimately stop
    /// at bounded `MaxSeq`/`MaxN` terminals — are untouched.
    ///
    /// DISCIPLINE (empirically enforced by `ty`): a model checked this way MUST
    /// give every legitimate work-complete terminal an explicit guarded
    /// zero-update `Done` self-loop, because TLA+ `[Next]_vars` stuttering does
    /// NOT count as an enabled step for deadlock purposes — without it `ty`
    /// flags the clean terminal as a deadlock. With it, the only reported
    /// deadlock is a genuine all-parties-parked wedge.
    #[must_use]
    pub fn to_cfg_deadlock_with(&self, overrides: &[(&'static str, i64)]) -> String {
        self.to_cfg_with(overrides)
            .replace("CHECK_DEADLOCK FALSE\n", "CHECK_DEADLOCK TRUE\n")
    }

    /// Config for [`transition_spec`](Self::transition_spec): binds each model
    /// constant (with optional `overrides`, e.g. the real ring capacity instead of
    /// the small exhaustive-check bound) and pins each `<var>_init` to `init`. Used
    /// for per-transition conformance — `ty trace validate --spec` then strictly
    /// checks the real `prev -> next` step against the derived `Next`.
    pub fn transition_cfg(
        &self,
        init: &BTreeMap<&'static str, i64>,
        overrides: &[(&'static str, i64)],
    ) -> String {
        let mut s = String::new();
        for (n, default) in &self.consts {
            let val = overrides
                .iter()
                .find(|(o, _)| o == n)
                .map(|(_, v)| *v)
                .unwrap_or(*default);
            s.push_str(&format!("CONSTANT {n} = {val}\n"));
        }
        for v in &self.vars {
            let val = init.get(v.name).copied().unwrap_or(v.init);
            s.push_str(&format!("CONSTANT {}_init = {val}\n", v.name));
        }
        s.push_str("SPECIFICATION Spec\nCHECK_DEADLOCK FALSE\n");
        s
    }

    /// The initial concrete state (variable -> value).
    pub fn init_state(&self) -> BTreeMap<&'static str, i64> {
        self.vars.iter().map(|v| (v.name, v.init)).collect()
    }

    /// The `(machine, action)` anchor targets this model declares — the spec→source
    /// half of the cross-reference (TRUST_NATIVE_TLA §2.1). The closure gate
    /// (`crate::xref::check_closure`) resolves each `#[refines(machine, action)]`
    /// against this set: obligation 1 (the action exists in the model) and the
    /// coverage obligation (every action is bound-or-waived). `machine` is the
    /// model's own `name` for every action.
    pub fn anchors(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.actions.iter().map(move |a| (self.name, a.name))
    }

    /// Every next-state reachable by firing `action` from `state` — the executable
    /// twin of the generated TLA+ action's existential fan-out. TLA+ primed
    /// semantics: every right-hand side is evaluated against the PRE-state, then
    /// applied atomically. A purely deterministic action yields exactly ONE
    /// successor; an action with a nondeterministic [`Expr::InRange`] update yields
    /// one successor per value in its bounded `lo..=hi` range (the cartesian product
    /// when several updates are nondeterministic). Returns an EMPTY vector if the
    /// guard is unsatisfied OR any range is empty (`lo > hi`) — exactly the states
    /// where the generated `var' \in {}` makes the TLA+ action disabled.
    pub fn successors(
        &self,
        action: &str,
        state: &BTreeMap<&'static str, i64>,
    ) -> Vec<BTreeMap<&'static str, i64>> {
        let act = self
            .actions
            .iter()
            .find(|a| a.name == action)
            .unwrap_or_else(|| panic!("no action `{action}` in model `{}`", self.name));
        let mut env = self.const_env();
        env.extend(state.iter().map(|(k, v)| (*k, *v)));
        if act.guard.as_ref().is_some_and(|g| !g.eval(&env).as_bool()) {
            return Vec::new();
        }
        // Evaluate each update against the PRE-state into a list of candidate values
        // (one for a deterministic `=`, many for a nondeterministic `\in lo..hi`).
        let choices: Vec<(&'static str, Vec<i64>)> = act
            .updates
            .iter()
            .map(|u| match &u.expr {
                Expr::InRange(lo, hi) => {
                    let (l, h) = (lo.eval(&env).as_int(), hi.eval(&env).as_int());
                    (u.var, (l..=h).collect())
                }
                e => (u.var, vec![e.eval(&env).as_int()]),
            })
            .collect();
        // Cartesian product over the per-update choice lists. Starts from the
        // unchanged pre-state (so variables no action touches stay put), then
        // branches each successor over every choice — ascending, so the FIRST
        // successor takes every range's lower bound (the canonical representative
        // `fire` selects). An empty choice list collapses the product to none.
        let mut acc: Vec<BTreeMap<&'static str, i64>> = vec![state.clone()];
        for (var, vals) in choices {
            let mut next = Vec::with_capacity(acc.len() * vals.len());
            for base in &acc {
                for &v in &vals {
                    let mut s = base.clone();
                    s.insert(var, v);
                    next.push(s);
                }
            }
            acc = next;
        }
        acc
    }

    /// Fire a named action against `state`, applying the CANONICAL successor (TLA+
    /// primed semantics). Returns `false` without mutating if the action is disabled
    /// (guard unsatisfied or an empty nondeterministic range). For a deterministic
    /// action this is the unique successor; for a nondeterministic one it is the
    /// lower-bound representative — drive [`Model::successors`] directly to explore
    /// the full fan-out. This is the executable twin of the generated TLA+ action.
    pub fn fire(&self, action: &str, state: &mut BTreeMap<&'static str, i64>) -> bool {
        match self.successors(action, state).into_iter().next() {
            Some(next) => {
                *state = next;
                true
            }
            None => false,
        }
    }

    /// Whether `action`'s guard is satisfied in `state` (no guard ⇒ always
    /// enabled). This lets real code be checked against the model's ACTUAL guard
    /// expression — e.g. a conformance test can assert the real subscriber gaps
    /// exactly when `PollGap` is enabled — rather than re-stating the predicate.
    pub fn action_enabled(&self, action: &str, state: &BTreeMap<&'static str, i64>) -> bool {
        let act = self
            .actions
            .iter()
            .find(|a| a.name == action)
            .unwrap_or_else(|| panic!("no action `{action}` in model `{}`", self.name));
        let mut env = self.const_env();
        env.extend(state.iter().map(|(k, v)| (*k, *v)));
        act.guard
            .as_ref()
            .map(|g| g.eval(&env).as_bool())
            .unwrap_or(true)
    }

    /// Evaluate a named invariant against a concrete state.
    pub fn check_invariant(&self, name: &str, state: &BTreeMap<&'static str, i64>) -> bool {
        let inv = self
            .invariants
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("no invariant `{name}`"));
        let mut env = self.const_env();
        env.extend(state.iter().map(|(k, v)| (*k, *v)));
        inv.expr.eval(&env).as_bool()
    }
}

/// TERMINAL MODES — the DEC/ANSI/keyboard mode-flag state machine that the VT
/// handler maintains (TRUST_NATIVE_TLA, Phase 0: resolves the dangling
/// `terminal_modes` machine the `#[refines]` anchors in
/// `aterm-core/src/terminal/handler_{dec,esc,state,report}*.rs` point at).
///
/// Each modelled mode is a bounded scalar (booleans as `{0,1}`; the multi-valued
/// `mouse_mode`/`mouse_encoding`/`cursor_style` as small bounded ints). The 26
/// actions are exactly the `#[refines(machine="terminal_modes", action=…)]` set:
/// the `Set*`/`Reset*` toggle pairs, the multi-valued setters
/// (`SetMouseMode`/`SetSgrMouseEncoding`/`SetCursorStyle`), and the two reset
/// actions (`SoftReset` = DECSTR, `FullReset` = RIS) which return the modes to
/// known defaults. (The DEC modes the handler explicitly does NOT model —
/// VT52/132-col/reverse-video/BiDi/… — are `#[spec_unmodeled(reason=…)]` waivers,
/// not actions here; that is the deliberate "modelled vs. waived" split.)
///
/// **Invariant `ModesValid`.** Every mode stays inside its valid domain under ANY
/// interleaving of the 26 actions: the booleans never leave `{0,1}`, and the
/// multi-valued modes never leave their enum range. This is the contract the
/// handler genuinely maintains — `TerminalModes` fields are `bool`/small `enum`,
/// so a mode is never an out-of-range / torn value, and a reset always lands on a
/// valid default. It is non-vacuous: `ty` enumerates the full action fan-out from
/// `Init`, so a setter that pushed a mode out of range (or a reset that left a
/// stale out-of-range value) would be caught.
///
/// SCOPE: this scalar model captures mode *validity* and the reset discipline (the
/// set/reset/RIS/DECSTR contract), not per-mode rendering semantics — those live in
/// the engine and, where bounded, in the other derived models. It exists so the
/// `terminal_modes` anchors RESOLVE (obligation 4) and are fully bound-or-waived
/// (obligation 3): the smallest sound model that makes the cross-reference real.
pub fn terminal_modes_model() -> Model {
    crate::ty_model! {
        TerminalModes {
            // Boolean modes (0/1). DECSTR (SoftReset) / RIS (FullReset) defaults
            // are encoded in the two reset actions below.
            var app_cursor_keys = 0;
            var origin_mode = 0;
            var auto_wrap = 1;          // DECAWM defaults ON
            var cursor_visible = 1;     // DECTCEM defaults ON
            var focus_reporting = 0;
            var sync_output = 0;
            var insert_mode = 0;
            var new_line_mode = 0;
            var alt_screen = 0;
            var bracketed_paste = 0;
            // Multi-valued modes (small bounded enums). mouse_mode: 0=off..4; the
            // mouse coordinate encoding sgr flag: 0/1; cursor_style: 0..6 (DECSCUSR).
            var mouse_mode = 0;
            var sgr_mouse = 0;
            var cursor_style = 0;

            action SetApplicationCursorKeys { app_cursor_keys = 1; }
            action ResetApplicationCursorKeys { app_cursor_keys = 0; }
            action SetOriginMode { origin_mode = 1; }
            action ResetOriginMode { origin_mode = 0; }
            action SetAutoWrap { auto_wrap = 1; }
            action ResetAutoWrap { auto_wrap = 0; }
            action SetCursorVisible { cursor_visible = 1; }
            action ResetCursorVisible { cursor_visible = 0; }
            action SetFocusReporting { focus_reporting = 1; }
            action ResetFocusReporting { focus_reporting = 0; }
            action SetSynchronizedOutput { sync_output = 1; }
            action ResetSynchronizedOutput { sync_output = 0; }
            action SetInsertMode { insert_mode = 1; }
            action ResetInsertMode { insert_mode = 0; }
            action SetNewLineMode { new_line_mode = 1; }
            action ResetNewLineMode { new_line_mode = 0; }
            action SetAlternateScreen { alt_screen = 1; }
            action ResetAlternateScreen { alt_screen = 0; }
            action SetBracketedPaste { bracketed_paste = 1; }
            action ResetBracketedPaste { bracketed_paste = 0; }
            // Multi-valued setters: enter a representative valid value in range.
            // (The real handler picks among X10/Normal/ButtonEvent/AnyEvent etc;
            // the bounded abstraction is "any in-range mode", here a fixed witness.)
            action SetMouseMode { mouse_mode = 1; }
            action SetSgrMouseEncoding { sgr_mouse = 1; }
            action ResetSgrMouseEncoding { sgr_mouse = 0; }
            action SetCursorStyle { cursor_style = 6; }

            // SoftReset (DECSTR): return modes to their soft defaults — cursor
            // visible, autowrap on, everything else off; mouse/encoding/style cleared.
            action SoftReset {
                app_cursor_keys = 0;
                origin_mode = 0;
                auto_wrap = 1;
                cursor_visible = 1;
                focus_reporting = 0;
                sync_output = 0;
                insert_mode = 0;
                new_line_mode = 0;
                bracketed_paste = 0;
                mouse_mode = 0;
                sgr_mouse = 0;
                cursor_style = 0;
            }
            // FullReset (RIS): hard reset — everything to power-on defaults
            // (including leaving the alternate screen).
            action FullReset {
                app_cursor_keys = 0;
                origin_mode = 0;
                auto_wrap = 1;
                cursor_visible = 1;
                focus_reporting = 0;
                sync_output = 0;
                insert_mode = 0;
                new_line_mode = 0;
                alt_screen = 0;
                bracketed_paste = 0;
                mouse_mode = 0;
                sgr_mouse = 0;
                cursor_style = 0;
            }

            // Every mode stays in its valid domain under any action interleaving.
            invariant ModesValid:
                app_cursor_keys <= 1 && origin_mode <= 1 && auto_wrap <= 1
                && cursor_visible <= 1 && focus_reporting <= 1 && sync_output <= 1
                && insert_mode <= 1 && new_line_mode <= 1 && alt_screen <= 1
                && bracketed_paste <= 1 && mouse_mode <= 4 && sgr_mouse <= 1
                && cursor_style <= 6;
        }
    }
}

/// The bounded event-log ring as a derived model — the single source the spec in
/// `Evict.tla` hand-encodes, scalar-projected to `<<seq, lo>>`. `Push` advances
/// `seq` and evicts the oldest live event (`lo`) exactly when the live window
/// would exceed `Cap`. `MaxSeq` bounds the state space so `ty check` is
/// exhaustive + terminating; `Cap` is the ring capacity. Action name is `Push`
/// (not `Append`, which clashes with ty's Sequences builtin).
pub fn ring_model() -> Model {
    Model {
        name: "Ring",
        consts: vec![("MaxSeq", 6), ("Cap", 3)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "lo",
                init: 1,
            },
        ],
        fn_vars: vec![],
        actions: vec![Action {
            name: "Push",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))), // seq <= MaxSeq - 1
            updates: vec![
                Update {
                    var: "seq",
                    expr: add(var("seq"), int(1)),
                },
                Update {
                    var: "lo",
                    // IF (seq + 1) - lo + 1 > Cap THEN lo + 1 ELSE lo
                    expr: if_(
                        gt(
                            add(sub(add(var("seq"), int(1)), var("lo")), int(1)),
                            cst("Cap"),
                        ),
                        add(var("lo"), int(1)),
                        var("lo"),
                    ),
                },
            ],
        }],
        invariants: vec![Invariant {
            name: "LenBounded",
            // seq - lo + 1 <= Cap
            expr: le(add(sub(var("seq"), var("lo")), int(1)), cst("Cap")),
        }],
    }
}

/// A second derived model — a writer/subscriber cursor — chosen because it
/// exercises derivation paths the ring does not: TWO actions (so `Next` is a
/// disjunction) and PARTIAL updates (so each action emits an `UNCHANGED` clause
/// for the variable it leaves alone). `Grow` advances the writer `seq`; `Deliver`
/// catches the reader `cursor` up to `seq`. Invariant: the reader never passes the
/// writer (`cursor <= seq`). This is the Subscribe/Kernel family in miniature, and
/// it proves the derivation engine generalizes beyond the single-action ring.
pub fn cursor_model() -> Model {
    Model {
        name: "Cursor",
        consts: vec![("MaxSeq", 4)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "cursor",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Grow", // writer appends; cursor is UNCHANGED
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![Update {
                    var: "seq",
                    expr: add(var("seq"), int(1)),
                }],
            },
            Action {
                name: "Deliver", // reader catches up; seq is UNCHANGED
                guard: Some(gt(var("seq"), var("cursor"))),
                updates: vec![Update {
                    var: "cursor",
                    expr: var("seq"),
                }],
            },
        ],
        invariants: vec![Invariant {
            name: "CursorBounded",
            expr: le(var("cursor"), var("seq")), // cursor <= seq
        }],
    }
}

/// A third derived model — the subscriber's NO-SILENT-LOSS / gap discipline, the
/// kernel family's most important correctness property: a reader that has fallen
/// behind the live ring window MUST receive a Gap (resync) and must NEVER be
/// silently delivered events as if nothing was lost. Scalar projection over
/// `<<seq, lo, cursor, lost>>`: `Grow` advances the writer and evicts the oldest
/// when over `Cap`; `PollGap` resyncs a fallen-behind reader (`lo > cursor + 1`);
/// `PollDeliver` delivers while the reader is still within the live window.
///
/// The `Buggy` constant flips `PollDeliver`'s guard: with `Buggy = 0` (committed)
/// it is correctly guarded and `lost` stays 0; with `Buggy = 1` it fires even when
/// the reader is behind, silently skipping evicted events — so `lost` becomes 1
/// and `NoSilentLoss` is violated. Thus `ty` both PROVES the property (Buggy=0)
/// and, via a `Buggy=1` cfg, shows it genuinely CATCHES the silent-loss bug.
/// Exercises the `Expr` disjunction (`\/`) and equality (`=`) operators.
pub fn subscribe_model() -> Model {
    Model {
        name: "Subscribe",
        consts: vec![("MaxSeq", 4), ("Cap", 2), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "lo",
                init: 1,
            },
            StateVar {
                name: "cursor",
                init: 0,
            },
            StateVar {
                name: "lost",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Grow", // writer appends + evicts oldest when over Cap
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![
                    Update {
                        var: "seq",
                        expr: add(var("seq"), int(1)),
                    },
                    Update {
                        var: "lo",
                        expr: if_(
                            gt(
                                add(sub(add(var("seq"), int(1)), var("lo")), int(1)),
                                cst("Cap"),
                            ),
                            add(var("lo"), int(1)),
                            var("lo"),
                        ),
                    },
                ], // cursor, lost UNCHANGED
            },
            Action {
                name: "PollGap", // reader fell behind (lo > cursor + 1): resync, no loss
                guard: Some(gt(var("lo"), add(var("cursor"), int(1)))),
                updates: vec![Update {
                    var: "cursor",
                    expr: var("seq"),
                }], // seq, lo, lost UNCHANGED
            },
            Action {
                name: "PollDeliver", // deliver; correct iff the reader is still in window
                // Buggy = 1 \/ lo =< cursor + 1  (Buggy removes the in-window guard)
                guard: Some(or_(
                    eq(cst("Buggy"), int(1)),
                    le(var("lo"), add(var("cursor"), int(1))),
                )),
                updates: vec![
                    Update {
                        var: "cursor",
                        expr: var("seq"),
                    },
                    // lost' = IF lo > cursor + 1 THEN 1 ELSE lost  (records a silent skip)
                    Update {
                        var: "lost",
                        expr: if_(
                            gt(var("lo"), add(var("cursor"), int(1))),
                            int(1),
                            var("lost"),
                        ),
                    },
                ], // seq, lo UNCHANGED
            },
        ],
        invariants: vec![Invariant {
            name: "NoSilentLoss",
            expr: eq(var("lost"), int(0)), // lost = 0
        }],
    }
}

/// OBSERVATION KERNEL — NO-SILENT-LOSS LATCH (RFC "The Reactive Surface", L0).
/// The abstract twin of `aterm-core`'s [`WatcherSet`](../../aterm_core/terminal/observe)
/// no-silent-loss invariant, bound to the real engine by
/// `aterm-core/tests/conformance_observe.rs`.
///
/// A surface predicate can be **transiently** true — a row matched then
/// overwritten, a block completed then superseded — across two coalesced
/// consumer wakes. The kernel must latch the predicate AT THE SEAM where it
/// became true (`post_process`), not on the later, coalescing wake that sees
/// only the LATEST state. Scalar projection `<<truth, latched, lost>>`: `Rise`
/// makes the predicate true (the CORRECT kernel latches immediately; the buggy
/// one defers to a wake), `Fall` clears the transient (recording a silent loss
/// if it was never latched), `Wake` is the coalescing consumer that can latch
/// only while `truth` still holds.
///
/// `Buggy = 0` (committed): `Rise` latches at the seam, so `Fall` never loses and
/// `NoSilentLoss` holds. `Buggy = 1`: `Rise` defers, so a `Rise`→`Fall` with no
/// intervening `Wake` silently drops the event → `lost = 1`. Thus `ty` PROVES the
/// latch (Buggy=0) and CATCHES the coalescing-loss bug (Buggy=1 → counterexample).
pub fn watcher_latch_model() -> Model {
    Model {
        name: "WatcherLatch",
        consts: vec![("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "truth",
                init: 0,
            },
            StateVar {
                name: "latched",
                init: 0,
            },
            StateVar {
                name: "lost",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Rise", // predicate becomes true at a processed batch
                guard: Some(eq(var("truth"), int(0))),
                updates: vec![
                    Update {
                        var: "truth",
                        expr: int(1),
                    },
                    Update {
                        var: "latched",
                        // CORRECT (Buggy=0): latch AT THE SEAM. Buggy=1: defer.
                        expr: if_(eq(cst("Buggy"), int(0)), int(1), var("latched")),
                    },
                ], // lost UNCHANGED
            },
            Action {
                name: "Fall", // the transient clears
                guard: Some(eq(var("truth"), int(1))),
                updates: vec![
                    Update {
                        var: "truth",
                        expr: int(0),
                    },
                    Update {
                        var: "lost",
                        // never latched + buggy deferral => a true was silently lost
                        expr: if_(
                            and_(eq(cst("Buggy"), int(1)), eq(var("latched"), int(0))),
                            int(1),
                            var("lost"),
                        ),
                    },
                ], // latched UNCHANGED
            },
            Action {
                name: "Wake", // coalescing consumer: latches only while truth holds
                guard: Some(eq(var("truth"), int(1))),
                updates: vec![Update {
                    var: "latched",
                    expr: int(1),
                }], // truth, lost UNCHANGED
            },
        ],
        invariants: vec![Invariant {
            name: "NoSilentLoss",
            expr: eq(var("lost"), int(0)), // lost = 0
        }],
    }
}

/// OBSERVATION KERNEL — EARLIEST-ARMED IDLE DEADLINE (RFC L0). The abstract twin
/// of [`WatcherSet::next_deadline`](../../aterm_core/terminal/observe): the host
/// arms ONE `ControlFlow::WaitUntil`, and it must equal the MINIMUM of all
/// pending `IdleFor` deadlines so an earlier deadline is never missed (the
/// `BellFlash::deadline` discipline). Scalar projection `<<armed, minp>>` over
/// two deadline values (near = 1, far = 2; `4` is the unset sentinel): `ArmNear`
/// / `ArmFar` register a deadline; `armed` must track `minp = min` of everything
/// registered.
///
/// `Buggy = 0` (committed): `armed' = min(armed, v)`, so `armed = minp` always.
/// `Buggy = 1`: keep-first (`armed` set only while unset), so arming the FAR
/// deadline then the NEAR one leaves `armed = 2` while `minp = 1` — an earlier
/// wake is missed. `ty` PROVES `armed = minp` (Buggy=0) and CATCHES the
/// keep-first bug (Buggy=1 → counterexample). Two-action disjunctive `Next` with
/// nested `if` updates (the `cursor_model` family, plus a min computation).
pub fn idle_deadline_model() -> Model {
    // min(a, b) == IF a > b THEN b ELSE a ; keep-first(a, v) == IF a = Unset THEN v ELSE a.
    let arm = |v: i64| -> Expr {
        if_(
            eq(cst("Buggy"), int(0)),
            if_(gt(var("armed"), int(v)), int(v), var("armed")), // min(armed, v)
            if_(eq(var("armed"), int(4)), int(v), var("armed")), // keep-first
        )
    };
    Model {
        name: "IdleDeadline",
        consts: vec![("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "armed",
                init: 4,
            }, // 4 == unset sentinel (no pending deadline)
            StateVar {
                name: "minp",
                init: 4,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "ArmNear", // register the nearer deadline (value 1)
                guard: Some(gt(var("minp"), int(1))),
                updates: vec![
                    Update {
                        var: "minp",
                        expr: if_(gt(var("minp"), int(1)), int(1), var("minp")),
                    },
                    Update {
                        var: "armed",
                        expr: arm(1),
                    },
                ],
            },
            Action {
                name: "ArmFar", // register the farther deadline (value 2)
                guard: Some(gt(var("minp"), int(2))),
                updates: vec![
                    Update {
                        var: "minp",
                        expr: if_(gt(var("minp"), int(2)), int(2), var("minp")),
                    },
                    Update {
                        var: "armed",
                        expr: arm(2),
                    },
                ],
            },
        ],
        invariants: vec![Invariant {
            name: "EarliestArmed",
            expr: eq(var("armed"), var("minp")), // armed == min of pending deadlines
        }],
    }
}

/// SELF-REFLECTION FEEDBACK GOVERNOR — FAIL-CLOSED (RFC "The Reactive Surface",
/// R4 / L2). The abstract twin of `aterm-agent`'s
/// [`SelfGovernor`](../../aterm_agent/struct.SelfGovernor.html): once the
/// circuit-breaker trips on sustained self-induced churn, NO self-write may
/// proceed — the storm backstop that `await-idle` alone cannot provide (a
/// self-write that produces output keeps `content_seq` advancing, so quiescence
/// never settles). This models the BREAKER condition of the real
/// `SelfGovernor::allow_self_write` gate — the latching, hardest-to-reason-about
/// one; the gate's other two fail-closed conditions (self-write disabled, or the
/// token bucket empty) are non-latching and covered by `SelfGovernor`'s unit
/// tests. Scalar projection `<<tripped, wrote_while_tripped>>`: `Trip` latches the
/// breaker; `Write` proceeds only while NOT tripped (the correct gate) and records
/// a violation if it ever fires while tripped.
///
/// `Buggy = 0` (committed): `Write` is guarded on `tripped = 0`, so a write never
/// happens after a trip and `FailClosed` holds. `Buggy = 1`: the guard drops, so
/// a `Trip`→`Write` lets a self-write through the tripped breaker →
/// `wrote_while_tripped = 1`. Thus `ty` PROVES the backstop (Buggy=0) and CATCHES
/// the breaker-bypass bug (Buggy=1 → counterexample). This is the `edge_gate`
/// FailClosed shape (`decision <= granted`).
pub fn self_governor_model() -> Model {
    Model {
        name: "SelfGovernor",
        consts: vec![("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "tripped",
                init: 0,
            },
            StateVar {
                name: "wrote_while_tripped",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Trip", // sustained self-churn trips the breaker (latching)
                guard: Some(eq(var("tripped"), int(0))),
                updates: vec![Update {
                    var: "tripped",
                    expr: int(1),
                }], // wrote_while_tripped UNCHANGED
            },
            Action {
                name: "Write", // a self-write attempt
                // CORRECT (Buggy=0): only when NOT tripped. Buggy=1: drop the gate.
                guard: Some(or_(eq(cst("Buggy"), int(1)), eq(var("tripped"), int(0)))),
                updates: vec![Update {
                    var: "wrote_while_tripped",
                    // a write that fired while tripped is a fail-OPEN violation
                    expr: if_(
                        eq(var("tripped"), int(1)),
                        int(1),
                        var("wrote_while_tripped"),
                    ),
                }], // tripped UNCHANGED
            },
        ],
        invariants: vec![Invariant {
            name: "FailClosed",
            expr: eq(var("wrote_while_tripped"), int(0)), // no write survived a trip
        }],
    }
}

/// SELF-FEED FLOOR — NO-OVERDRAFT (RFC D3). The abstract twin of `aterm-gui`'s
/// [`inject_floor`](../../aterm_gui/inject_floor) token bucket: the un-bypassable
/// control-layer backstop that bounds self-targeted input injection so a raw
/// client cannot drive a feedback storm. Scalar projection `<<tokens, over>>`
/// over a bucket of capacity `Cap`: `Refill` adds a token (capped); `Write`
/// admits an injection only with a spare token (the correct gate) and records an
/// overdraft if it ever admits at zero.
///
/// `Buggy = 0` (committed): `Write` is guarded on `tokens > 0`, so it never
/// overdraws and `NoOverdraft` holds (and `tokens <= Cap` from the capped
/// refill). `Buggy = 1`: the guard drops, so a `Write` at `tokens = 0` injects
/// past the floor → `over = 1`. `ty` PROVES the bound (Buggy=0) and CATCHES the
/// overdraft bug (Buggy=1 → counterexample). The bounded-ring / token-bucket
/// shape (`ring_model` family).
pub fn inject_floor_model() -> Model {
    Model {
        name: "InjectFloor",
        consts: vec![("Cap", 2), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "tokens",
                init: 2,
            }, // starts full (= Cap)
            StateVar {
                name: "over",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Refill", // continuous refill, capped at Cap
                guard: Some(le(var("tokens"), sub(cst("Cap"), int(1)))),
                updates: vec![Update {
                    var: "tokens",
                    expr: add(var("tokens"), int(1)),
                }], // over UNCHANGED
            },
            Action {
                name: "Write", // a self-targeted injection attempt
                // CORRECT (Buggy=0): only with a spare token. Buggy=1: drop the gate.
                guard: Some(or_(eq(cst("Buggy"), int(1)), gt(var("tokens"), int(0)))),
                updates: vec![
                    Update {
                        var: "tokens",
                        expr: if_(
                            gt(var("tokens"), int(0)),
                            sub(var("tokens"), int(1)),
                            var("tokens"),
                        ),
                    },
                    Update {
                        var: "over",
                        // admitted at zero tokens => overdraft (floor bypassed)
                        expr: if_(eq(var("tokens"), int(0)), int(1), var("over")),
                    },
                ],
            },
        ],
        invariants: vec![
            Invariant {
                name: "NoOverdraft",
                expr: eq(var("over"), int(0)), // never admitted past an empty bucket
            },
            Invariant {
                name: "BoundedTokens",
                expr: le(var("tokens"), cst("Cap")), // the bucket never exceeds Cap
            },
        ],
    }
}

/// NETWORK CAPABILITY — CHANNEL-BOUND, NO REPLAY (RFC D4 / L3). The abstract twin
/// of `aterm-net`'s [`channel_bind`](../../aterm_net/fn.channel_bind.html): an
/// edge token captured on one connection must NOT authorize on another. The local
/// fabric's same-uid `SO_PEERCRED` check has no network analog, so the token is
/// bound to the connection's exporter (`presented = H(token, exporter)`); a
/// replay on a different channel presents a value computed over the WRONG
/// exporter. Scalar projection `<<captured, accepted_replay>>`: `Capture` records
/// the channel-A presented value an adversary observed; `ReplayOnB` presents it on
/// channel B.
///
/// `Buggy = 0` (committed): the verifier checks the binding against the CURRENT
/// channel, so the cross-channel replay is rejected and `accepted_replay` stays 0.
/// `Buggy = 1`: the verifier ignores the channel (accepts a bare token), so the
/// replay succeeds → `accepted_replay = 1`. `ty` PROVES no-replay (Buggy=0) and
/// CATCHES the channel-unbound bug (Buggy=1 → counterexample).
pub fn channel_bind_model() -> Model {
    Model {
        name: "ChannelBind",
        consts: vec![("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "captured",
                init: 0,
            },
            StateVar {
                name: "accepted_replay",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Capture", // adversary records channel-A's presented value
                guard: Some(eq(var("captured"), int(0))),
                updates: vec![Update {
                    var: "captured",
                    expr: int(1),
                }], // accepted_replay UNCHANGED
            },
            Action {
                name: "ReplayOnB", // present the captured A-value on channel B
                guard: Some(eq(var("captured"), int(1))),
                updates: vec![Update {
                    var: "accepted_replay",
                    // channel-bound verifier (Buggy=0) rejects; unbound (Buggy=1) accepts
                    expr: if_(eq(cst("Buggy"), int(1)), int(1), var("accepted_replay")),
                }], // captured UNCHANGED
            },
        ],
        invariants: vec![Invariant {
            name: "NoReplay",
            expr: eq(var("accepted_replay"), int(0)), // a cross-channel replay never authorizes
        }],
    }
}

/// COLOUR-PRESENTATION GATE — a code point that defaults to TEXT presentation is
/// never resolved to the colour-emoji face. The abstract twin of aterm-render's
/// `select_face` (the real-code binding is aterm-render's
/// `select_face_never_colors_text_presentation` exhaustive test).
///
/// This is the model of the ⏺ (U+23FA) fix: `select_face` used to choose the
/// colour-emoji face for ANY code point the monochrome faces missed but Apple
/// Color Emoji covered — ignoring the Unicode `Emoji_Presentation` property.
/// U+23FA is `Emoji=Yes` but `Emoji_Presentation=No`, so it defaults to text; the
/// reference terminals gate the colour face on that property (iTerm2:
/// `emojiWithDefaultEmojiPresentation` membership; Ghostty:
/// `uucode.get(.is_emoji_presentation, cp)`), never on raw font coverage.
///
/// Scalar projection `<<wants_emoji, color>>`: `wants_emoji` = 1 iff the code
/// point has default emoji presentation (or an explicit VS16) — the only
/// legitimate trigger for colour; `color` = the gate's output (1 = resolved to
/// the colour face), RECOMPUTED from `wants_emoji` in the same step (face
/// selection is stateless / per-call, so the decision is never stale). The two
/// `Want*` actions spread the nondeterministic input — a default-emoji code point
/// vs a default-text one — over the reachable space.
///
/// `Buggy` gates the SHIPPED defect: with `Buggy = 0` (committed) `color` is set
/// ONLY when `wants_emoji`; with `Buggy = 1` it is set regardless (the old
/// coverage-only gate), so a default-TEXT code point gets `color = 1` and
/// `NoColorForText` is violated. Thus `ty` PROVES the gate (Buggy=0) and CATCHES
/// the real regression (Buggy=1 → counterexample). Exercises a constant-guarded
/// `if` update and a two-action disjunctive `Next`.
pub fn presentation_gate_model() -> Model {
    // color' = if (wants_emoji' OR Buggy) then 1 else 0. TLA primed semantics use
    // unprimed vars on the RHS, so substitute the LITERAL value each action is
    // about to assign to `wants_emoji` (1 for WantEmoji, 0 for WantText).
    let color_for = |wants_emoji_next: i64| {
        if_(
            gt(add(int(wants_emoji_next), cst("Buggy")), int(0)),
            int(1),
            int(0),
        )
    };
    Model {
        name: "PresentationGate",
        consts: vec![("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "wants_emoji",
                init: 0,
            },
            StateVar {
                name: "color",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                // The code point has default-emoji presentation (or VS16): colour allowed.
                name: "WantEmoji",
                guard: Some(le(var("wants_emoji"), int(0))),
                updates: vec![
                    Update {
                        var: "wants_emoji",
                        expr: int(1),
                    },
                    Update {
                        var: "color",
                        expr: color_for(1),
                    },
                ],
            },
            Action {
                // A default-text code point: colour must be withheld (the fix).
                name: "WantText",
                guard: Some(gt(var("wants_emoji"), int(0))),
                updates: vec![
                    Update {
                        var: "wants_emoji",
                        expr: int(0),
                    },
                    Update {
                        var: "color",
                        expr: color_for(0),
                    },
                ],
            },
        ],
        invariants: vec![Invariant {
            // A colour resolution implies the code point wanted emoji presentation.
            name: "NoColorForText",
            expr: le(var("color"), var("wants_emoji")),
        }],
    }
}

/// SPAWN-TIME LOCALE GUARANTEE — the child process aterm launches must run under a
/// UTF-8 `LC_CTYPE` whatever locale aterm inherited. `LC_CTYPE` is the POSIX
/// character-encoding category; under a non-UTF-8 one, locale-aware programs (emacs,
/// vim, python) re-encode multibyte terminal output to the ASCII codeset and emit a
/// literal `?` per character — the box-drawing-`?` bug. The real decision is
/// `aterm_pty::resolve_spawn_locale` (POSIX precedence `LC_ALL > LC_CTYPE > LANG`,
/// empty == unset); this is its abstract twin, with the real-code binding in
/// aterm-pty's `spawn_locale_conformance_*` test.
///
/// Scalar projection `<<present, eff_utf8, ctype, resolved>>`: `present` = any locale
/// var is set non-empty; `eff_utf8` = the effective inherited encoding is already
/// UTF-8; `ctype` = the child's resulting `LC_CTYPE` (1 = UTF-8) once `resolved`. The
/// two `Observe*` actions spread the nondeterministic input shape — nothing set; a
/// non-UTF-8 locale present; a UTF-8 locale present — and `Resolve` runs the fix once.
///
/// `Buggy` gates the SHIPPED defect: with `Buggy = 0` (committed) `Resolve` always
/// yields `ctype = 1`; with `Buggy = 1` it forces UTF-8 ONLY when nothing is present
/// (the old all-unset guard), so a present-but-non-UTF-8 locale leaves `ctype = 0`
/// and `ChildHasUtf8Ctype` is violated. Thus `ty` PROVES the guarantee (Buggy=0) and
/// CATCHES the real regression (Buggy=1 → counterexample). Exercises a nested `if`,
/// a constant-guarded update, and disjunction.
pub fn spawn_locale_model() -> Model {
    Model {
        name: "SpawnLocale",
        consts: vec![("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "present",
                init: 0,
            },
            StateVar {
                name: "eff_utf8",
                init: 0,
            },
            StateVar {
                name: "ctype",
                init: 0,
            },
            StateVar {
                name: "resolved",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "ObserveNonUtf8", // a non-UTF-8 locale is present (e.g. LANG=C)
                guard: Some(and_(
                    eq(var("resolved"), int(0)),
                    eq(var("present"), int(0)),
                )),
                updates: vec![Update {
                    var: "present",
                    expr: int(1),
                }], // eff_utf8 stays 0
            },
            Action {
                name: "ObserveUtf8", // a UTF-8 locale is present
                guard: Some(and_(
                    eq(var("resolved"), int(0)),
                    eq(var("present"), int(0)),
                )),
                updates: vec![
                    Update {
                        var: "present",
                        expr: int(1),
                    },
                    Update {
                        var: "eff_utf8",
                        expr: int(1),
                    },
                ],
            },
            Action {
                name: "Resolve", // run resolve_spawn_locale once; sets the child's LC_CTYPE
                guard: Some(eq(var("resolved"), int(0))),
                updates: vec![
                    Update {
                        var: "resolved",
                        expr: int(1),
                    },
                    // Buggy=1: IF present=0 THEN 1 ELSE eff_utf8 (old all-unset guard
                    // leaves a present non-UTF-8 locale unfixed). Buggy=0: always 1.
                    Update {
                        var: "ctype",
                        expr: if_(
                            eq(cst("Buggy"), int(1)),
                            if_(eq(var("present"), int(0)), int(1), var("eff_utf8")),
                            int(1),
                        ),
                    },
                ],
            },
        ],
        invariants: vec![Invariant {
            name: "ChildHasUtf8Ctype",
            // resolved => ctype = 1
            expr: or_(eq(var("resolved"), int(0)), eq(var("ctype"), int(1))),
        }],
    }
}

/// A fifth derived model — TRANSACTION ATOMICITY / no-lost-update under
/// optimistic concurrency (the `Transact` kernel-family property). A transaction
/// reads the head at `tbase` (`Begin`); concurrent `Write`s may advance `seq`. At
/// commit, the correct discipline is: commit only if no write intervened
/// (`seq = tbase`), otherwise ABORT — committing against a stale `tbase` would
/// clobber the concurrent write (a lost update). Scalar projection over
/// `<<seq, tbase, active, lost>>`, edits-per-txn `K`.
///
/// `Buggy` gates the bad path: with `Buggy = 0` (committed) a conflict can only
/// `Abort`, so `lost` stays 0; with `Buggy = 1` the txn may commit despite a
/// conflict (`seq' = tbase + K`, overwriting the intervening write) and sets
/// `lost = 1`. So `ty` proves `NoLostUpdate` (Buggy=0) and catches it (Buggy=1).
/// Exercises the `Expr` conjunction (`/\`) operator in guards.
pub fn transact_model() -> Model {
    Model {
        name: "Transact",
        consts: vec![("MaxSeq", 4), ("K", 2), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "tbase",
                init: 0,
            },
            StateVar {
                name: "active",
                init: 0,
            },
            StateVar {
                name: "lost",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Write", // a concurrent writer advances the committed head
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![Update {
                    var: "seq",
                    expr: add(var("seq"), int(1)),
                }],
            },
            Action {
                name: "Begin", // a txn reads the current head as its base version
                guard: Some(eq(var("active"), int(0))),
                updates: vec![
                    Update {
                        var: "active",
                        expr: int(1),
                    },
                    Update {
                        var: "tbase",
                        expr: var("seq"),
                    },
                ],
            },
            Action {
                name: "CommitClean", // no write intervened: commit K edits atomically
                guard: Some(and_(
                    and_(eq(var("active"), int(1)), eq(var("seq"), var("tbase"))),
                    le(var("seq"), sub(cst("MaxSeq"), cst("K"))),
                )),
                updates: vec![
                    Update {
                        var: "seq",
                        expr: add(var("seq"), cst("K")),
                    },
                    Update {
                        var: "active",
                        expr: int(0),
                    },
                ],
            },
            Action {
                name: "Abort", // a write intervened (seq > tbase): correct path aborts
                guard: Some(and_(
                    eq(var("active"), int(1)),
                    gt(var("seq"), var("tbase")),
                )),
                updates: vec![Update {
                    var: "active",
                    expr: int(0),
                }],
            },
            Action {
                name: "BuggyCommit", // conflict committed anyway -> clobbers, lost update
                guard: Some(and_(
                    and_(
                        and_(eq(var("active"), int(1)), gt(var("seq"), var("tbase"))),
                        eq(cst("Buggy"), int(1)),
                    ),
                    le(var("tbase"), sub(cst("MaxSeq"), cst("K"))),
                )),
                updates: vec![
                    Update {
                        var: "seq",
                        expr: add(var("tbase"), cst("K")),
                    },
                    Update {
                        var: "active",
                        expr: int(0),
                    },
                    Update {
                        var: "lost",
                        expr: int(1),
                    },
                ],
            },
        ],
        invariants: vec![Invariant {
            name: "NoLostUpdate",
            expr: eq(var("lost"), int(0)), // lost = 0
        }],
    }
}

/// A fifth derived model — the event-log SPINE: gap-free, monotone, `seq == count`
/// (the `Kernel` family property). Each `Append` assigns the next contiguous seq
/// and bumps the count, so the head seq always equals the number of events — no
/// gaps, no duplicates. `Buggy` makes an append jump seq by 2 (a gap), so
/// `seq != count` and `SeqIsCount` is violated. ty proves it (Buggy=0) and catches
/// the gap (Buggy=1).
pub fn kernel_model() -> Model {
    Model {
        name: "Kernel",
        consts: vec![("MaxSeq", 5), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "count",
                init: 0,
            },
        ],
        // Action `Emit` (not `Append`, which clashes with ty's Sequences builtin in
        // a single-action spec — see the ring's `Push`).
        fn_vars: vec![],
        actions: vec![Action {
            name: "Emit",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
            updates: vec![
                Update {
                    var: "count",
                    expr: add(var("count"), int(1)),
                },
                // seq' = IF Buggy = 1 THEN seq + 2 ELSE seq + 1   (Buggy opens a gap)
                Update {
                    var: "seq",
                    expr: if_(
                        eq(cst("Buggy"), int(1)),
                        add(var("seq"), int(2)),
                        add(var("seq"), int(1)),
                    ),
                },
            ],
        }],
        invariants: vec![Invariant {
            name: "SeqIsCount",
            expr: eq(var("seq"), var("count")), // seq = count (gap-free, monotone spine)
        }],
    }
}

/// A sixth derived model — SNAPSHOT isolation (the `Snapshot` family property): a
/// snapshot, once taken, is isolated from later writes; a write must NOT leak into
/// an active snapshot's view. Scalar projection over `<<seq, snapped, leaked>>`:
/// `Snap` activates a snapshot; `Write` advances the head and, in the BUGGY case
/// (`Buggy = 1 /\ snapped = 1`), leaks into the snapshot (`leaked = 1`). ty proves
/// `SnapshotIsolated` (Buggy=0) and catches the leak (Buggy=1).
pub fn snapshot_model() -> Model {
    Model {
        name: "Snapshot",
        consts: vec![("MaxSeq", 4), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "snapped",
                init: 0,
            },
            StateVar {
                name: "leaked",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Snap", // take a single snapshot of the current head
                guard: Some(eq(var("snapped"), int(0))),
                updates: vec![Update {
                    var: "snapped",
                    expr: int(1),
                }], // seq, leaked UNCHANGED
            },
            Action {
                name: "Write", // advance the head; must not leak into an active snapshot
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![
                    Update {
                        var: "seq",
                        expr: add(var("seq"), int(1)),
                    },
                    // leaked' = IF Buggy = 1 /\ snapped = 1 THEN 1 ELSE leaked
                    Update {
                        var: "leaked",
                        expr: if_(
                            and_(eq(cst("Buggy"), int(1)), eq(var("snapped"), int(1))),
                            int(1),
                            var("leaked"),
                        ),
                    },
                ], // snapped UNCHANGED
            },
        ],
        invariants: vec![Invariant {
            name: "SnapshotIsolated",
            expr: eq(var("leaked"), int(0)), // leaked = 0
        }],
    }
}

/// The READ-IMAGE snapshot-SEQ protocol (REARCH A-3), authored via [`ty_model!`].
///
/// The engine's render snapshot ([`crate`]-external `Terminal::cell_frame_into`)
/// is stamped with the monotone `damage_epoch` as its `snapshot_seq`. This models
/// the temporal contract that stamp must obey, scalar-projected over
/// `<<epoch, snapped, snap_seq, torn>>`:
///
///   * `Damage` advances the live `epoch` (the engine bumps `damage_epoch` on
///     net-new grid damage) — the MONOTONE-SEQ driver.
///   * `ReadImage` captures `snap_seq := epoch` ATOMICALLY and activates the
///     snapshot (`snapped := 1`) — the "value-of-seq at snapshot time", filled
///     under the one lock (no torn read).
///   * `Write` advances `epoch` (more damage after the snapshot). In the BUGGY
///     case (`Buggy == 1 && snapped == 1`) the later write leaks into the active
///     snapshot, setting `torn := 1` — a retro-mutation / torn read.
///
/// Two invariants, both proven at the committed `Buggy = 0`:
///   1. `NoTornRead: torn = 0` — SNAPSHOT INTERNAL-CONSISTENCY: a write after the
///      capture never mutates the already-emitted snapshot. This is what the
///      `Buggy` flip violates (so `ty` catches it at `Buggy = 1`), exactly the
///      `snapshot_model` isolation discipline applied to the seq stamp.
///   2. `SeqIsStaleOrCurrent: snap_seq <= epoch` — the captured seq never exceeds
///      the live epoch, so it is MONOTONE (the epoch only grows) and STALENESS is
///      always DETECTABLE: a consumer observing `epoch > snap_seq` knows its
///      snapshot is behind. Holds for both `Buggy` values (it pins the capture to
///      `= epoch`; a wrong capture would violate it), so it is non-vacuous.
///
/// Conformance-bound to the REAL `Terminal::cell_frame` + `damage_epoch`/
/// `take_damage` path in `aterm-core/tests/conformance_read_image_seq.rs` (Tier-1),
/// with a negative control so the pass is never vacuous.
#[must_use]
pub fn read_image_seq_model() -> Model {
    crate::ty_model! {
        ReadImageSeq {
            const MaxSeq = 4;
            const Buggy = 0;
            // The live damage epoch (monotone). The snapshot-active latch. The
            // seq captured by the last ReadImage. The torn-read leak flag.
            var epoch = 0;
            var snapped = 0;
            var snap_seq = 0;
            var torn = 0;

            // More grid damage bumps the engine's damage_epoch.
            action Damage when (epoch <= MaxSeq - 1) {
                epoch = epoch + 1;
            }

            // read_image: capture snap_seq = damage_epoch atomically, activate.
            action ReadImage when (snapped == 0) {
                snap_seq = epoch;
                snapped = 1;
            }

            // A write after the snapshot advances the epoch; in the buggy case it
            // leaks into the active snapshot (a torn read).
            action Write when (epoch <= MaxSeq - 1) {
                epoch = epoch + 1;
                torn = if Buggy == 1 && snapped == 1 { 1 } else { torn };
            }

            // No torn read / snapshot internal-consistency (the catch at Buggy=1).
            invariant NoTornRead: torn == 0;
            // Monotone + staleness-detectable: the captured seq never exceeds the
            // live epoch, so epoch > snap_seq is an observable "snapshot is stale".
            invariant SeqIsStaleOrCurrent: snap_seq <= epoch;
        }
    }
}

/// The aterm session PTY-master FD-LIFECYCLE discipline as a DERIVED model
/// (initiative A7, WS-G/concurrency) — the drift-free, code-bound twin that
/// SUPERSEDES the hand-written `FdLifecycle.tla` (now quarantined to `specs/legacy/`,
/// exactly as the kernel-family specs were when their derived twins took over). It
/// is the SINGLE source of truth for the `SinkWriter` ownership state machine in
/// `aterm-session/src/sink.rs`:
///
///   * `SinkWriter` owns the PTY master fd via `Option<OwnedFd>` (sink.rs:53); the
///     fd closes EXACTLY when the last `Arc<SinkWriter>` clone drops, never
///     out-of-band (sink.rs:32-39).
///   * Raw-fd use is via [`master`](../../../aterm_session/sink/struct.SinkWriter.html#method.master)
///     (sink.rs:84) and [`write_frame`](../../../aterm_session/sink/struct.SinkWriter.html#method.write_frame)
///     (sink.rs:97).
///
/// Scalar projection over `<<clones, fdOpen, usedAfterClose>>`:
///
///   * `Clone` — `Arc::clone` adds a holder while a live clone exists and the bound
///     `MaxClones` is not reached (`clones > 0 /\ clones < MaxClones`); `clones += 1`.
///     Bound in source as a `#[spec_unmodeled]` waiver on `new_owned` (std
///     `Arc::clone` is pure RAII — no aterm method to anchor).
///   * `UseFd` — a holder uses the RAW master fd (`master()` / `write_frame`). Sound
///     while open; latches `usedAfterClose` if the fd is already closed
///     (`usedAfterClose' = usedAfterClose \/ ~fdOpen`). Bound to the REAL fd-use code
///     via `#[refines]` on `write_frame`/`master`.
///   * `DropClone` — dropping one clone. THE FIX closes the fd (via `OwnedFd::drop`)
///     EXACTLY when the last clone drops (`fdOpen' = (clones - 1 > 0)`); the modeled
///     DEFECT (`Buggy = 1`) is the pre-fix bare-`i32` `close()` that fires on EVERY
///     drop — an out-of-band close while siblings still hold + use the raw fd
///     (`fdOpen' = 0` regardless of the remaining count). Bound as a
///     `#[spec_unmodeled]` waiver (the `OwnedFd` `Drop` is RAII — no aterm method).
///
/// `Buggy` convention (single-source prove-AND-catch, like `subscribe_model` /
/// `kernel_model` / etc.): the defect rides INSIDE the always-live `DropClone` action
/// (its `fdOpen` update), NOT a separate Buggy-only action — so at `Buggy = 0` no
/// action is dead (the `--strict-vacuity` gate stays green). At the committed
/// `Buggy = 0` the close-on-last-drop discipline holds, so `ty` PROVES both
/// invariants over the whole bounded space; at `Buggy = 1` a drop closes the fd while
/// clones remain and a subsequent `UseFd` latches the use-after-close, making both
/// invariants reachable-false — `ty` finds the COUNTEREXAMPLE. So the derived model
/// hosts BOTH halves; the hand `.tla` is no longer a second registered source.
///
/// Invariants, both proven over the whole bounded space (`MaxClones = 3`, `Buggy=0`):
///   1. `NoUseAfterClose: ~usedAfterClose` — no holder ever uses the raw fd after it
///      closed (the use-after-close race the fix eliminates).
///   2. `ClosedImpliesNoClones: (~fdOpen) => (clones = 0)` — the fd is closed only
///      when no live holder remains (the `OwnedFd`-last-drop guarantee). Written
///      `fdOpen \/ clones = 0` because the builder DSL has no `=>`/`~`.
#[must_use]
pub fn fd_lifecycle_model() -> Model {
    Model {
        name: "FdLifecycle",
        consts: vec![("MaxClones", 3), ("Buggy", 0)],
        vars: vec![
            // Live Arc<SinkWriter> clone count (the original owner starts holding it).
            StateVar {
                name: "clones",
                init: 1,
            },
            // Is the PTY master fd still open? (1 = open, 0 = closed.)
            StateVar {
                name: "fdOpen",
                init: 1,
            },
            // Latched: did a holder use the raw fd after it was closed? (the race.)
            StateVar {
                name: "usedAfterClose",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Clone", // Arc::clone: another party takes a holder.
                // clones > 0 /\ clones < MaxClones
                guard: Some(and_(
                    gt(var("clones"), int(0)),
                    le(var("clones"), sub(cst("MaxClones"), int(1))),
                )),
                updates: vec![Update {
                    var: "clones",
                    expr: add(var("clones"), int(1)),
                }],
            },
            Action {
                name: "UseFd", // a holder uses the RAW master fd (master()/write_frame).
                guard: Some(gt(var("clones"), int(0))),
                // usedAfterClose' = IF fdOpen = 0 THEN 1 ELSE usedAfterClose
                // (latches the use-after-close; fdOpen, clones UNCHANGED).
                updates: vec![Update {
                    var: "usedAfterClose",
                    expr: if_(eq(var("fdOpen"), int(0)), int(1), var("usedAfterClose")),
                }],
            },
            Action {
                name: "DropClone", // drop one clone; THE FIX closes the fd only on the last drop.
                guard: Some(gt(var("clones"), int(0))),
                updates: vec![
                    Update {
                        var: "clones",
                        expr: sub(var("clones"), int(1)),
                    },
                    // fdOpen' = IF Buggy = 1 THEN 0                       (DEFECT: bare close on
                    //                                                      EVERY drop, out-of-band)
                    //           ELSE IF clones - 1 > 0 THEN 1 ELSE 0      (FIX: close iff last holder)
                    // The defect rides this always-live action (no dead Buggy-only action), so
                    // --strict-vacuity stays green at Buggy=0.
                    Update {
                        var: "fdOpen",
                        expr: if_(
                            eq(cst("Buggy"), int(1)),
                            int(0),
                            if_(gt(sub(var("clones"), int(1)), int(0)), int(1), int(0)),
                        ),
                    },
                ],
            },
        ],
        invariants: vec![
            // No party ever uses the raw master fd after it has been closed.
            Invariant {
                name: "NoUseAfterClose",
                expr: eq(var("usedAfterClose"), int(0)),
            },
            // The fd is closed only when no live holder remains (fdOpen \/ clones = 0).
            Invariant {
                name: "ClosedImpliesNoClones",
                expr: or_(eq(var("fdOpen"), int(1)), eq(var("clones"), int(0))),
            },
        ],
    }
}

/// A FAITHFUL per-element ring model with a function-valued live-set
/// `live: [1..MaxSeq -> BOOLEAN]` — the property the scalar `ring_model` cannot
/// express. It proves `EvictOldestContiguous`: the live region is EXACTLY the
/// contiguous window `[lo, seq]`, so eviction removes precisely the oldest event,
/// never a hole and never two. This is the function-valued twin of the
/// hand-written `Evict.tla`'s operational `live` discipline. Because it is
/// function-valued, it is Tier-0 `ty`-checked (TLA+ generation), not run through
/// the scalar interpreter.
pub fn evict_full_model() -> Model {
    // (seq + 1) - lo + 1 > Cap : the eviction condition (over the pre-state seq).
    let evicting = || {
        gt(
            add(sub(add(var("seq"), int(1)), var("lo")), int(1)),
            cst("Cap"),
        )
    };
    Model {
        name: "EvictFull",
        consts: vec![("MaxSeq", 5), ("Cap", 3)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "lo",
                init: 1,
            },
        ],
        fn_vars: vec![FnVar {
            name: "live",
            range: "MaxSeq",
        }],
        actions: vec![Action {
            name: "Push",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
            updates: vec![
                Update {
                    var: "seq",
                    expr: add(var("seq"), int(1)),
                },
                Update {
                    var: "lo",
                    expr: if_(evicting(), add(var("lo"), int(1)), var("lo")),
                },
                Update {
                    var: "live",
                    // Evicting: rebuild the live-set as (old minus the evicted `lo`)
                    // plus the new event `seq+1`. Non-evicting: just mark `seq+1`.
                    expr: if_(
                        evicting(),
                        comprehension(
                            "n",
                            int(1),
                            cst("MaxSeq"),
                            if_(
                                eq(var("n"), add(var("seq"), int(1))),
                                bool_lit(true),
                                and_(fn_access("live", var("n")), neq(var("n"), var("lo"))),
                            ),
                        ),
                        except("live", add(var("seq"), int(1)), bool_lit(true)),
                    ),
                },
            ],
        }],
        invariants: vec![Invariant {
            name: "EvictOldestContiguous",
            // \A n \in 1..MaxSeq : live[n] <=> (lo =< n /\ n =< seq)
            expr: forall(
                "n",
                int(1),
                cst("MaxSeq"),
                iff(
                    fn_access("live", var("n")),
                    and_(le(var("lo"), var("n")), le(var("n"), var("seq"))),
                ),
            ),
        }],
    }
}

/// NEW machine (HIERARCHICAL_SESSIONS.md Addendum B, B.8.2): TIER-RESIDENCY /
/// **spill-not-forget**.
///
/// This is deliberately **not** an extension of [`evict_full_model`]. That model
/// proves `EvictOldestContiguous` — an evicted seq is *definitively not live*.
/// The hydratable temporal buffer asserts the **opposite**: an evicted seq is
/// still *recoverable*. So this is a different state machine over a new
/// `resident_warm`/`resident_cold` projection layered on the same eviction spine.
///
/// `Push` is the eviction spine of `evict_full_model` with one behavioral change:
/// when it evicts the oldest seq `lo`, it **atomically spills** it to the warm
/// tier (`resident_warm[lo] := TRUE`) on the same step — modeling the recorder's
/// spill hook firing synchronously on the `pop_front` path, so there is never an
/// intermediate state where the evicted event is neither live nor resident.
/// `Demote` moves the whole warm tier to cold (both count as resident, so the
/// tier transition preserves recoverability; the DSL has no "pick one index", so
/// a whole-tier demotion is the faithful bounded abstraction).
///
/// Invariant **`NoSilentLoss`**: every recorded seq up to the head is live or
/// resident in some tier —
/// `\A n \in 1..MaxSeq : (n =< seq) => (live[n] \/ resident_warm[n] \/ resident_cold[n])`.
/// The implication is written `(n > seq) \/ R` because the builder DSL has no
/// `=>`/`~`.
///
/// Negative control: at `Buggy = 1`, `Push` **drops on evict without spilling**,
/// so the evicted seq becomes neither live nor resident and `NoSilentLoss` fails —
/// `ty` finds the counterexample. Thus the proof at `Buggy = 0` is non-vacuous.
///
/// Function-valued ⇒ Tier-0 `ty`-checked (TLA+ generation), not run through the
/// scalar interpreter. The keyframe-recoverability clause from the design
/// (`\E k : k =< n /\ keyframe_at[k] /\ resident(k)`) is intentionally **out of
/// scope here**: it needs an existential the derive DSL lacks and belongs to the
/// B.8.3 hydration-faithfulness model, where the keyframe→replay fold lives.
/// Residency (live ∨ warm ∨ cold) is the complete spill-not-forget property for
/// this machine.
pub fn tier_residency_model() -> Model {
    // (seq + 1) - lo + 1 > Cap : the eviction condition over the pre-state seq
    // (identical to `evict_full_model`'s spine).
    let evicting = || {
        gt(
            add(sub(add(var("seq"), int(1)), var("lo")), int(1)),
            cst("Cap"),
        )
    };
    Model {
        name: "TierResidency",
        consts: vec![("MaxSeq", 4), ("Cap", 2), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            },
            StateVar {
                name: "lo",
                init: 1,
            },
        ],
        fn_vars: vec![
            FnVar {
                name: "live",
                range: "MaxSeq",
            },
            FnVar {
                name: "resident_warm",
                range: "MaxSeq",
            },
            FnVar {
                name: "resident_cold",
                range: "MaxSeq",
            },
        ],
        actions: vec![
            Action {
                name: "Push",
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![
                    Update {
                        var: "seq",
                        expr: add(var("seq"), int(1)),
                    },
                    Update {
                        var: "lo",
                        expr: if_(evicting(), add(var("lo"), int(1)), var("lo")),
                    },
                    Update {
                        var: "live",
                        // Same live-set discipline as evict_full: evicting rebuilds
                        // (old minus the evicted `lo`) plus the new event; otherwise
                        // just mark `seq+1`.
                        expr: if_(
                            evicting(),
                            comprehension(
                                "n",
                                int(1),
                                cst("MaxSeq"),
                                if_(
                                    eq(var("n"), add(var("seq"), int(1))),
                                    bool_lit(true),
                                    and_(fn_access("live", var("n")), neq(var("n"), var("lo"))),
                                ),
                            ),
                            except("live", add(var("seq"), int(1)), bool_lit(true)),
                        ),
                    },
                    Update {
                        var: "resident_warm",
                        // Spill-not-forget: on eviction the evicted `lo` lands in warm
                        // ATOMICALLY. The bug (Buggy=1) drops it — no spill — so the
                        // evicted seq is left in no tier and NoSilentLoss fails.
                        // Non-evicting Push leaves warm untouched.
                        expr: if_(
                            and_(evicting(), eq(cst("Buggy"), int(0))),
                            except("resident_warm", var("lo"), bool_lit(true)),
                            var("resident_warm"),
                        ),
                    },
                    // resident_cold UNCHANGED in Push (rendered automatically).
                ],
            },
            Action {
                name: "Demote",
                // warm -> cold for the whole tier: every warm seq becomes cold, and
                // both are "resident", so residency is preserved across the demotion.
                // Always enabled (a no-op self-loop when warm is empty — harmless for
                // invariant checking).
                guard: None,
                updates: vec![
                    Update {
                        var: "resident_cold",
                        expr: comprehension(
                            "n",
                            int(1),
                            cst("MaxSeq"),
                            or_(
                                fn_access("resident_cold", var("n")),
                                fn_access("resident_warm", var("n")),
                            ),
                        ),
                    },
                    Update {
                        var: "resident_warm",
                        expr: comprehension("n", int(1), cst("MaxSeq"), bool_lit(false)),
                    },
                    // seq, lo, live UNCHANGED.
                ],
            },
        ],
        invariants: vec![Invariant {
            name: "NoSilentLoss",
            // \A n \in 1..MaxSeq : (n =< seq) => (live[n] \/ warm[n] \/ cold[n])
            // implication encoded as (n > seq) \/ R (DSL has no => / ~).
            expr: forall(
                "n",
                int(1),
                cst("MaxSeq"),
                or_(
                    gt(var("n"), var("seq")),
                    or_(
                        fn_access("live", var("n")),
                        or_(
                            fn_access("resident_warm", var("n")),
                            fn_access("resident_cold", var("n")),
                        ),
                    ),
                ),
            ),
        }],
    }
}

/// NEW machine (HIERARCHICAL_SESSIONS.md Addendum B, B.8.3): HYDRATION-FAITHFULNESS.
///
/// The centerpiece replay property: hydrating a recording at an instant and
/// folding events forward from a keyframe reproduces the LIVE engine state —
/// `P(replay@t) = P(live@t)`. This is deliberately **not** the rejected
/// "bookkeeping tautology" (`hydrated_seq = seq`, a counter copied to a counter);
/// the invariant compares two parallel FOLDS, so a dropped/omitted event makes
/// them diverge and `ty` finds the counterexample.
///
/// **Abstract projection `P`.** `ty` function-vars are boolean-valued (`to_tla`
/// seeds them all-FALSE), so `P` is a one-bit checksum and the fold is a running
/// XOR (parity) over the recorded events — `a # b` in TLA+. Parity is
/// history-dependent: dropping or omitting any event flips the tail, exactly the
/// faithfulness hazard. (A richer multi-bit `P` needs int-valued fn-vars, a DSL
/// extension tracked for B.8.4 Tier-1 binding; one-bit parity already makes the
/// drop/omit controls bite.)
///
/// **Why a cursor, not a one-shot Hydrate.** A TLA+ comprehension
/// `[n |-> f(replay[n-1], payload[n])]` reads the OLD `replay`, so it is NOT a
/// real left-fold. Instead `Hydrate` SEEDS `replay` from the keyframe
/// (`replay[n] = live[n]` for `n =< KF`) and `ReplayStep` folds replay forward
/// ONE index per step via a cursor `rt` (each step legitimately reads the prior
/// new `replay[rt]`) — a genuine keyframe-seed-then-forward-replay.
///
/// **Invariant `ReplayFaithful`:** `\A n : (n =< rt) => (replay[n] = live[n])`
/// (encoded `(n > rt) \/ (replay[n] <=> live[n])`; the DSL has no `=>`/`~`).
///
/// **Negative control.** At `Buggy = 1`, `ReplayStep` at `rt+1 == DROPAT` skips
/// applying the payload (parity stalls), so `replay[DROPAT..]` diverges from
/// `live` and `ReplayFaithful` is violated — `ty` proves faithfulness at
/// `Buggy = 0` and catches the silent drop at `Buggy = 1`. Because the fold
/// would be a no-op if the clock/payload were not an explicit recorded input,
/// this model is only authorable AFTER the B.4.2 Clock seam landed.
pub fn recording_model() -> Model {
    Model {
        name: "Recording",
        // KF = keyframe seq (fixed); DROPAT = the replay index the bug drops.
        consts: vec![("MaxSeq", 4), ("KF", 2), ("DROPAT", 3), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "seq",
                init: 0,
            }, // live head
            StateVar {
                name: "rt",
                init: 0,
            }, // replay cursor (how far replay has folded)
        ],
        fn_vars: vec![
            FnVar {
                name: "payload",
                range: "MaxSeq",
            }, // recorded events (TRUE once recorded)
            FnVar {
                name: "live",
                range: "MaxSeq",
            }, // live parity fold
            FnVar {
                name: "replay",
                range: "MaxSeq",
            }, // replay parity fold (from keyframe)
        ],
        actions: vec![
            // Record one event: payload[seq+1]=TRUE; live[seq+1] = live[seq] XOR TRUE
            // (base: live[1] = TRUE since live[0] does not exist).
            Action {
                name: "Record",
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![
                    Update {
                        var: "seq",
                        expr: add(var("seq"), int(1)),
                    },
                    Update {
                        var: "payload",
                        expr: except("payload", add(var("seq"), int(1)), bool_lit(true)),
                    },
                    Update {
                        var: "live",
                        expr: except(
                            "live",
                            add(var("seq"), int(1)),
                            if_(
                                eq(var("seq"), int(0)),
                                bool_lit(true),
                                neq(fn_access("live", var("seq")), bool_lit(true)),
                            ),
                        ),
                    },
                    // replay UNCHANGED
                ],
            },
            // Hydrate: seed replay from the keyframe — replay[n] = live[n] for n =< KF,
            // FALSE above; set the replay cursor to KF. Guarded so KF is recorded.
            Action {
                name: "Hydrate",
                guard: Some(le(cst("KF"), var("seq"))),
                updates: vec![
                    Update {
                        var: "rt",
                        expr: cst("KF"),
                    },
                    Update {
                        var: "replay",
                        expr: comprehension(
                            "n",
                            int(1),
                            cst("MaxSeq"),
                            if_(
                                le(var("n"), cst("KF")),
                                fn_access("live", var("n")), // keyframe seed
                                bool_lit(false),
                            ),
                        ),
                    },
                    // seq, payload, live UNCHANGED
                ],
            },
            // ReplayStep: fold replay forward one index using the prior replay[rt].
            // replay[rt+1] = replay[rt] XOR payload[rt+1], EXCEPT the DROP bug skips it.
            Action {
                name: "ReplayStep",
                // KF =< rt: only after Hydrate has seeded the cursor at the keyframe
                // (rt is 0 pre-Hydrate; folding before a seed would read replay[0],
                // out of the 1..MaxSeq domain). rt+1 =< seq: stay within recorded events.
                guard: Some(and_(
                    le(cst("KF"), var("rt")),
                    le(add(var("rt"), int(1)), var("seq")),
                )),
                updates: vec![
                    Update {
                        var: "rt",
                        expr: add(var("rt"), int(1)),
                    },
                    Update {
                        var: "replay",
                        expr: except(
                            "replay",
                            add(var("rt"), int(1)),
                            if_(
                                and_(
                                    eq(cst("Buggy"), int(1)),
                                    eq(add(var("rt"), int(1)), cst("DROPAT")),
                                ),
                                fn_access("replay", var("rt")), // DROP: skip payload (parity stalls)
                                neq(
                                    fn_access("replay", var("rt")),
                                    fn_access("payload", add(var("rt"), int(1))),
                                ),
                            ),
                        ),
                    },
                    // seq, payload, live UNCHANGED
                ],
            },
        ],
        invariants: vec![Invariant {
            name: "ReplayFaithful",
            // \A n in 1..MaxSeq : (n =< rt) => (replay[n] = live[n])
            // encoded (n > rt) \/ (replay[n] <=> live[n]); booleans use Iff.
            expr: forall(
                "n",
                int(1),
                cst("MaxSeq"),
                or_(
                    gt(var("n"), var("rt")),
                    iff(fn_access("replay", var("n")), fn_access("live", var("n"))),
                ),
            ),
        }],
    }
}

/// An ELEVENTH derived model — IN-PROCESS MULTI-WINDOW ROUTING (the GUI window
/// lifecycle the multi-window work builds: `App` holds `BTreeMap<WindowId,
/// WindowState>` with a `frontmost_window`; Cmd-N creates a window, closing the
/// last one exits the app). Scalar projection over `<<win_count, frontmost,
/// next_id, exited>>`: the number of live windows, the id of the frontmost window
/// (`0` == none), a MONOTONIC never-reused id source (the multi-window analogue
/// of `next_session_id`), and whether the app has exited. `MaxWin` bounds
/// concurrent windows and `MaxId` bounds total creations, keeping `ty`'s search
/// exhaustive + terminating.
///
/// `Buggy` gates the close-last-window path: with `Buggy = 0` (committed) closing
/// the LAST window sets `exited`, so exit and an empty window set stay in
/// lockstep; with `Buggy = 1` the last close fails to exit, reproducing the
/// "no windows left but the app is still running" defect. So `ty` both PROVES the
/// routing invariants (Buggy=0) and CATCHES the missed exit (Buggy=1 ->
/// counterexample on `ExitIffEmpty`).
///
/// Invariants:
///   ExitIffEmpty       — `exited = 1  <=>  win_count = 0` (close-last exits, and
///                        the app never exits while a window remains).
///   FrontmostLive      — `frontmost = 0  <=>  win_count = 0` (a non-empty set
///                        always has a real frontmost; an empty set has none).
///   FrontmostAllocated — `frontmost = 0  \/  frontmost < next_id` (the frontmost
///                        is never a future / unallocated / reused id — the
///                        never-reused property that makes a stale `Wake` for a
///                        closed window unable to address a live one).
///
/// SCOPE: this scalar projection tracks the frontmost's NULL-ness and ALLOCATION,
/// not which specific ids are live (that needs a per-element refinement / the
/// Tier-1 conformance bind to the real `App`). It is exactly the close→exit +
/// never-reuse safety core.
/// COALESCE: the streaming write fold must be a pure function of the byte log
/// regardless of how it is split across `process_at` calls — i.e. the fast
/// "bulk" lane and the reference "single-char" lane must agree on every cell.
///
/// This is a 2-SAFETY property (a relation between two runs over the SAME input),
/// which a plain single-execution invariant cannot state — which is exactly why
/// model-checking missed the wide-char-wrap-tail and ZWJ-join divergences that
/// shipped. It is encoded here by SELF-COMPOSITION (the same trick
/// `recording_model` uses for live-vs-replay parity): one machine folds the same
/// event stream down BOTH lanes and asserts they never diverge, lifting the
/// 2-safety to a 1-safety invariant `ty` can discharge. The `Buggy` convention
/// reproduces the real class: at `SKIPAT` the bulk lane drops the per-element
/// fixup the single lane applies (the wrap-tail blank / the ZWJ continuation),
/// so the lanes diverge and the invariant is violated.
///
/// Tier-1 binds this to the SHIPPING engine: `aterm-core/tests/replay_corpus_probe.rs`
/// drives the real `process_at` across every chunking of adversarial corpora and
/// asserts an identical `checkpoint()` — the concrete witness this model abstracts.
pub fn coalesce_model() -> Model {
    Model {
        name: "Coalesce",
        // SKIPAT = the fold index at which the buggy bulk lane drops the fixup.
        consts: vec![("MaxSeq", 4), ("SKIPAT", 2), ("Buggy", 0)],
        vars: vec![StateVar {
            name: "seq",
            init: 0,
        }],
        fn_vars: vec![
            FnVar {
                name: "single",
                range: "MaxSeq",
            }, // reference (per-char) fold
            FnVar {
                name: "bulk",
                range: "MaxSeq",
            }, // fast (coalesced) fold
        ],
        actions: vec![Action {
            name: "Emit",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
            updates: vec![
                Update {
                    var: "seq",
                    expr: add(var("seq"), int(1)),
                },
                // Reference lane: each element flips parity (the per-element fixup).
                Update {
                    var: "single",
                    expr: except(
                        "single",
                        add(var("seq"), int(1)),
                        if_(
                            eq(var("seq"), int(0)),
                            bool_lit(true),
                            neq(fn_access("single", var("seq")), bool_lit(true)),
                        ),
                    ),
                },
                // Bulk lane: identical fold, EXCEPT the Buggy variant skips the
                // fixup at SKIPAT (copies the previous cell), diverging — exactly
                // the wrap-tail / ZWJ class. The skip branch only reads bulk[seq]
                // when seq+1 = SKIPAT (so seq >= 1); seq = 0 takes the else.
                Update {
                    var: "bulk",
                    expr: except(
                        "bulk",
                        add(var("seq"), int(1)),
                        if_(
                            and_(
                                eq(cst("Buggy"), int(1)),
                                eq(add(var("seq"), int(1)), cst("SKIPAT")),
                            ),
                            fn_access("bulk", var("seq")), // BUG: drop the fixup
                            if_(
                                eq(var("seq"), int(0)),
                                bool_lit(true),
                                neq(fn_access("bulk", var("seq")), bool_lit(true)),
                            ),
                        ),
                    ),
                },
            ],
        }],
        invariants: vec![Invariant {
            name: "LanesAgree",
            // \A n in 1..MaxSeq : (n > seq) \/ (bulk[n] <=> single[n])
            expr: forall(
                "n",
                int(1),
                cst("MaxSeq"),
                or_(
                    gt(var("n"), var("seq")),
                    iff(fn_access("bulk", var("n")), fn_access("single", var("n"))),
                ),
            ),
        }],
    }
}

pub fn window_routing_model() -> Model {
    Model {
        name: "WindowRouting",
        consts: vec![("MaxWin", 2), ("MaxId", 4), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "win_count",
                init: 1,
            },
            StateVar {
                name: "frontmost",
                init: 1,
            },
            StateVar {
                name: "next_id",
                init: 2,
            },
            StateVar {
                name: "exited",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // Cmd-N / Wake::CreateWindow: a new window takes the next monotonic id
            // and becomes frontmost. Bounded by MaxWin (concurrent) + MaxId (total).
            Action {
                name: "CreateWindow",
                guard: Some(and_(
                    and_(
                        le(var("win_count"), sub(cst("MaxWin"), int(1))),
                        le(var("next_id"), sub(cst("MaxId"), int(1))),
                    ),
                    eq(var("exited"), int(0)),
                )),
                updates: vec![
                    Update {
                        var: "win_count",
                        expr: add(var("win_count"), int(1)),
                    },
                    Update {
                        var: "frontmost",
                        expr: var("next_id"),
                    },
                    Update {
                        var: "next_id",
                        expr: add(var("next_id"), int(1)),
                    },
                ],
            },
            // CloseRequested / Cmd-W last tab: close a window. Closing the LAST one
            // exits the app (unless Buggy) and clears frontmost to none (0); a
            // surviving window keeps a valid, already-allocated frontmost id.
            Action {
                name: "CloseWindow",
                guard: Some(and_(
                    gt(var("win_count"), int(0)),
                    eq(var("exited"), int(0)),
                )),
                updates: vec![
                    Update {
                        var: "win_count",
                        expr: sub(var("win_count"), int(1)),
                    },
                    // exited' = IF this was the last window THEN (Buggy ? 0 : 1) ELSE exited
                    Update {
                        var: "exited",
                        expr: if_(
                            eq(sub(var("win_count"), int(1)), int(0)),
                            if_(eq(cst("Buggy"), int(1)), int(0), int(1)),
                            var("exited"),
                        ),
                    },
                    // frontmost' \in (IF empty THEN {0} ELSE the surviving allocated ids).
                    //
                    // Closing the FRONTMOST window must RE-POINT frontmost to a
                    // survivor — but WHICH survivor is NOT a function of the scalar
                    // projection (`win_count`, `frontmost`, `next_id`): it doesn't
                    // track which specific ids are live. The real app picks the
                    // lowest live `WindowId` (BTreeMap order); a different policy
                    // would pick another. The faithful abstraction is therefore
                    // NONDETERMINISTIC: `frontmost'` may be ANY already-allocated id
                    // `1..(next_id - 1)`. A survivor remaining means a CreateWindow
                    // has run (`next_id >= 3`), so that range is non-empty; when the
                    // LAST window closes the range collapses to `0..0 = {0}` (no
                    // frontmost). `ty` checks the whole `\in` fan-out exhaustively, so
                    // EVERY admissible re-point preserves FrontmostLive (frontmost > 0
                    // iff a window remains) and FrontmostAllocated (frontmost <
                    // next_id, never future/reused) — and the real app's lowest-id
                    // choice is one such admissible value, so Tier-1 conformance
                    // accepts it. This ADMITS the frontmost-with-a-survivor re-point
                    // the old `frontmost' = frontmost` over-pinned away WITHOUT
                    // over-committing to an unprojectable policy. ExitIffEmpty (the
                    // Buggy=1 catch) is independent of this update, so the proof at
                    // Buggy=0 and the counterexample at Buggy=1 both still hold.
                    Update {
                        var: "frontmost",
                        expr: in_range(
                            if_(eq(sub(var("win_count"), int(1)), int(0)), int(0), int(1)),
                            if_(
                                eq(sub(var("win_count"), int(1)), int(0)),
                                int(0),
                                sub(var("next_id"), int(1)),
                            ),
                        ),
                    },
                ],
            },
        ],
        invariants: vec![
            Invariant {
                name: "ExitIffEmpty",
                // (exited=1 /\ win_count=0) \/ (exited=0 /\ win_count>0)
                expr: or_(
                    and_(eq(var("exited"), int(1)), eq(var("win_count"), int(0))),
                    and_(eq(var("exited"), int(0)), gt(var("win_count"), int(0))),
                ),
            },
            Invariant {
                name: "FrontmostLive",
                // (frontmost=0 /\ win_count=0) \/ (frontmost>0 /\ win_count>0)
                expr: or_(
                    and_(eq(var("frontmost"), int(0)), eq(var("win_count"), int(0))),
                    and_(gt(var("frontmost"), int(0)), gt(var("win_count"), int(0))),
                ),
            },
            Invariant {
                name: "FrontmostAllocated",
                // frontmost = 0 \/ frontmost < next_id (never a future/reused id)
                expr: or_(
                    eq(var("frontmost"), int(0)),
                    gt(var("next_id"), var("frontmost")),
                ),
            },
        ],
    }
}

// ===========================================================================
// Introspection / recursive-stacking models (aterm-gui control plane).
// These derive the SAFETY properties the lossless-introspection + cross-process
// proxy feature must hold, so `ty` proves them exhaustively over the bounded
// state space and CATCHES the audit's real defect classes (M1 dispatch gap,
// M2 relay teardown leak, S1 registry leak) under the `Buggy=1` convention —
// model+verify, not example-test. See docs/TRUST-introspection-audit-detection.md.
// ===========================================================================

/// DISPATCH COMPLETENESS (audit finding M1). The control router must route EVERY
/// forwardable verb class aimed at a REMOTE child to the cross-process forward —
/// never silently to the local path (where it answers `ERR no such session`).
///
/// `vc` enumerates the verb classes `0..MaxVc`; `decided` is the router verdict
/// (`0` undecided, `2` forward, `3` deny/local-miss). `Pick` chooses any class
/// nondeterministically (`ty` fans out the whole domain); `Route` applies the
/// routing table. Forwardable = `vc =< 3` (read/write/subscribe/feed-bin);
/// `vc = 4` is a non-forwardable owner verb. `Buggy = 1` drops the SUBSCRIBE
/// class (`vc = 2`) from the forward table — exactly M1, where the verb-first
/// `subscribe` grammar was missed by the selector-first planner.
///
/// Invariant `ForwardableRemoteAlwaysForwarded`: once decided, a forwardable
/// class is routed to forward (`decided = 2`). `ty` proves it at `Buggy = 0`
/// and returns the counterexample `vc = 2, decided = 3` at `Buggy = 1`.
pub fn dispatch_complete_model() -> Model {
    // verb classes 0..MaxVc; forwardable = vc =< 3 (read/write/subscribe/feed-bin),
    // vc=4 is a non-forwardable owner verb. Buggy drops the subscribe class (vc=2)
    // — the exact M1 grammar miss. forward=2, deny/local-miss=3.
    props::gated_completeness(props::Gated {
        name: "DispatchComplete",
        item: "vc",
        decided: "decided",
        domain_max: "MaxVc",
        domain_val: 4,
        fwd_hi: 3,
        drop: 2,
        good: 2,
        bad: 3,
        pick: "Pick",
        route: "Route",
        inv: "ForwardableRemoteAlwaysForwarded",
    })
}

/// RELAY TEARDOWN LIVENESS (audit finding M2). When the cross-process relay tears
/// down, BOTH read halves of BOTH sockets must be shut so a pump parked on a
/// CLONE of a local socket gets EOF and the worker thread joins (no thread/fd
/// leak). `child_read_open`/`client_read_open` model the two read halves a pump
/// blocks on; `done` gates the post-teardown check. `Teardown` shuts the halves:
/// the correct discipline (`shutdown(Both)`) closes the read halves; `Buggy = 1`
/// models the original `shutdown(Write)`-only, which leaves the read halves open.
///
/// Invariant `ReadersUnblockAfterTeardown`: after teardown, both read halves are
/// closed (so both pumps unblock). `ty` proves it at `Buggy = 0` and returns the
/// parked-reader counterexample (`done = 1` with a read half still open) at
/// `Buggy = 1`.
pub fn relay_teardown_model() -> Model {
    // shutdown(Both) closes BOTH read halves (Buggy shutdown(Write) leaves them
    // open → the parked-reader leak). The flags init OPEN (1); Teardown gates on
    // `done`. Invariant: not-yet-torn-down OR both read halves closed.
    props::teardown_clears(props::Teardown {
        name: "RelayTeardown",
        flags: vec!["child_read_open", "client_read_open"],
        gate: "done",
        act: "Teardown",
        inv: "ReadersUnblockAfterTeardown",
    })
}

/// PROXY REGISTRY LIFECYCLE (audit finding S1). A spawned child's `ProxyEntry`
/// must be deregistered on session close, so the process-wide table never grows
/// past the live-session count (no unbounded leak as tabs open/close). `live`
/// counts live sessions, `registered` counts retained entries, bounded by `MaxN`.
/// `Spawn` registers + adds a live session; `Close` removes a live session and —
/// correctly — its entry, but `Buggy = 1` models the original `Drop` that forgot
/// to deregister (the entry survives a closed session).
///
/// Invariant `NoRegistryLeak`: `registered =< live`. `ty` proves it at
/// `Buggy = 0` and catches the leak (`registered = live + 1`) at `Buggy = 1`.
pub fn proxy_registry_model() -> Model {
    props::lifecycle_no_leak(props::Lifecycle {
        name: "ProxyRegistry",
        live: "live",
        reg: "registered",
        max: "MaxN",
        max_val: 3,
        acquire: "Spawn",
        release: "Close",
        inv: "NoRegistryLeak",
    })
}

/// FORWARD-HANDSHAKE LIVENESS / DEADLOCK-FREEDOM — the real `drain_buffered` bug
/// (proxy.rs `drain_buffered`/`connect_and_relay`). This is the LIVENESS twin of
/// the safety models: it closes the gap the audit documented, where a blocking
/// I/O call deadlocks in a way no reachable-bad-STATE invariant can see.
///
/// The cross-process forward is a two-party request → relay → reply: the reply
/// bytes are ALREADY buffered past the request line, and the client is parked
/// awaiting the reply. Correct (`Buggy = 0`): the server relays the BUFFERED
/// bytes (`reader.buffer()`) and the client is always served — a work-complete
/// terminal that stutters via the `Done` self-loop, never a deadlock. `Buggy = 1`
/// models the shipped `fill_buf()` defect: the server insists on reading MORE
/// (`relayed > 0`) before the FIRST relay, but the client — blocked awaiting the
/// reply — sends nothing, so every action is disabled in a non-`Done` state: a
/// two-party all-parked WEDGE that `ty` reports as a DEADLOCK.
///
/// Checked with `CHECK_DEADLOCK TRUE` ([`Model::to_cfg_deadlock_with`]). The
/// `Done` self-loop is MANDATORY — without it `ty` flags the clean
/// `client_waiting = 0` terminal itself as a deadlock (stuttering does not count).
pub fn forward_handshake_model() -> Model {
    props::no_wedge(props::Wedge {
        name: "ForwardHandshake",
        buffered: "buffered",
        buffered_init: 1,
        relayed: "relayed",
        waiting: "client_waiting",
        relay: "Relay",
        recv: "ClientRecv",
        done: "Done",
        inv: "WaitingIsBool",
    })
}

/// AUTHORIZATION SOUNDNESS — the trust core's central predicate (`decide_edge` /
/// `EdgeTable::authorize`): a presented token is PERMITTED only when ALL four
/// conjuncts hold — the token is in the table, its `dst` equals the resolved
/// target, its `op` equals the verb's required op, and its nonce equals the
/// target's current launch nonce. The capability-layer audit found every one of
/// these checked on every request; this model GUARDS that against regression. The
/// `Buggy` variant drops the `dst` conjunct — the confused-deputy escalation (a
/// token valid for one session authorizing a different target); dropping `op`,
/// `nonce`, or `token` instead would model op-confusion, replay, or forgery.
///
/// Invariant `PermitImpliesAllGuards`: a permit implies every guard truly held.
/// `ty` proves it and catches the dropped-conjunct disclosure.
pub fn authorize_soundness_model() -> Model {
    props::conjunctive_authz(props::ConjunctiveAuthz {
        name: "AuthorizeSoundness",
        guards: vec!["token", "dst", "op", "nonce"],
        decided: "decided",
        pick: "Present",
        decide: "Authorize",
        drop: "dst",
        inv: "PermitImpliesAllGuards",
    })
}

/// NO TRANSITIVE AUTHORITY — the property that makes deep nesting SAFE and is the
/// reason `proxy_forward_plan` refuses to forward a chained `@a @b verb`: forwarding
/// requires OWNER scope (`if !matches!(scope, Scope::Owner) { return None }`), so a
/// connection that itself ARRIVED over a forward (and therefore carries only an
/// EDGE scope) cannot initiate a further forward. Authority does not COMPOSE: a
/// grandparent that owns a parent cannot borrow the parent's authority to reach a
/// grandchild — it would need a DIRECT edge to the grandchild (a delegation/grant).
/// This is the confused-deputy boundary the capability audit verified is closed.
///
/// Modeled as a single-guard instance of [`props::conjunctive_authz`] (reusing the
/// authorization-soundness class): a forward is permitted only when the connection
/// is Owner-scoped; `Buggy` waives that guard (the transitive escalation). Invariant
/// `ForwardImpliesOwner`: a permitted forward implies the connection was Owner.
pub fn no_transitive_authority_model() -> Model {
    props::conjunctive_authz(props::ConjunctiveAuthz {
        name: "NoTransitiveAuthority",
        guards: vec!["owner"],
        decided: "forwarded",
        pick: "Arrive",
        decide: "Forward",
        drop: "owner",
        inv: "ForwardImpliesOwner",
    })
}

/// TAB NAVIGATION — the GUI per-window tab-strip index machine (`TabIndex` in
/// `aterm-gui`: `{ active, count }`). One window always holds at least one tab and
/// the renderer reads `active` as an index into the live tab list, so the contract
/// is exactly: `count >= 1` AND `active <= count - 1` (with `usize`, `active >= 0`
/// is trivial). This is the smallest faithful abstraction of the four shipping
/// mutators, bounded by `Cap` tabs so `ty` explores a finite space:
///
///   * `NewTab`    ⟵ `TabIndex::add()`: append a tab and switch to it. `count' =
///     count + 1` and the new tab is the new LAST index, `active' =
///     count` (== new `count - 1`). Guarded `count <= Cap - 1`.
///   * `SelectTab` ⟵ `TabIndex::switch_to(i)` for an in-range `i` (Cmd-1..9 /
///     `switch_tab_in`): jump to ANY valid index. The specific `i` is
///     user input, not a function of the scalar projection, so the
///     faithful update is NONDETERMINISTIC: `active' \in 0..count-1`.
///     `ty` checks the whole fan-out, and the real in-range
///     `switch_to` lands on one such admissible value.
///   * `Cycle`     ⟵ `TabIndex::cycle(true)` (Cmd-Shift-]): forward with WRAP.
///     `(active + 1) % count` has no `%` in the macro algebra, but the
///     invariant `active <= count - 1` makes it exactly `active' = IF
///     active + 1 > count - 1 THEN 0 ELSE active + 1`. Guarded
///     `count > 1` (one tab is a no-op).
///   * `Close`     ⟵ `TabIndex::close(i)` for a non-exit close (`count > 1`, so a
///     window keeps >= 1 tab): `count' = count - 1`, then RE-CLAMP the
///     active index into the shrunk range. The worst case for the
///     range invariant is closing the LAST (active) tab, where active
///     must drop to the new last index `count - 2`; the faithful
///     re-clamp is `active' = IF active > count - 2 THEN count - 2 ELSE
///     active` (= `min(active, new_count - 1)`, matching `close`'s
///     `else if active >= count { active = count - 1 }` arm). Guarded
///     `count > 1`.
///
/// **`Buggy` non-vacuity control.** At `Buggy = 0` `ty` PROVES `CountPositive` +
/// `ActiveInRange` over the whole bounded space. The `Buggy` branch in `Close`
/// FORGETS the re-clamp (`active' = active`), so closing the last/active tab leaves
/// `active = count - 1` while the range shrank to `count - 2` — `active > count - 1`
/// after the step — and `ty` at `Buggy = 1` MUST yield a counterexample to
/// `ActiveInRange`. That is the exact "renderer indexes a tab that no longer
/// exists" defect the clamp prevents.
///
/// Hand-built (not via `ty_model!`) because `SelectTab` needs a NONDETERMINISTIC
/// in-range update (`in_range`), which the light-annotation macro does not surface
/// — same reason as [`window_routing_model`].
pub fn tab_nav_model() -> Model {
    Model {
        name: "TabNav",
        // Bound the tab count so `ty` explores a finite space (a window with up to
        // Cap tabs). `Buggy` flips the Close re-clamp off.
        consts: vec![("Cap", 4), ("Buggy", 0)],
        // A fresh window: one tab (count=1), it is active (active=0).
        vars: vec![
            StateVar {
                name: "count",
                init: 1,
            },
            StateVar {
                name: "active",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // add(): append a tab and switch to it — the new tab is the new LAST
            // index. All RHS evaluate against the pre-state, so `active' = count`
            // (old count == new `count' - 1`) and `count' = count + 1`.
            Action {
                name: "NewTab",
                guard: Some(le(var("count"), sub(cst("Cap"), int(1)))),
                updates: vec![
                    Update {
                        var: "active",
                        expr: var("count"),
                    },
                    Update {
                        var: "count",
                        expr: add(var("count"), int(1)),
                    },
                ],
            },
            // switch_to(i) for an in-range i (Cmd-1..9 / switch_tab_in): jump to ANY
            // valid index. The specific `i` is user input, not a function of the
            // scalar projection, so the faithful update is NONDETERMINISTIC:
            // `active' \in 0..(count - 1)`. `ty` checks the whole fan-out; the real
            // in-range `switch_to` lands on one such admissible value.
            Action {
                name: "SelectTab",
                guard: Some(gt(var("count"), int(1))),
                updates: vec![Update {
                    var: "active",
                    expr: in_range(int(0), sub(var("count"), int(1))),
                }],
            },
            // cycle(true) (Cmd-Shift-]): forward with WRAP. `(active + 1) % count`
            // has no `%` in this algebra, but the invariant `active <= count - 1`
            // makes it exactly `active' = IF active + 1 > count - 1 THEN 0 ELSE
            // active + 1`. Guarded `count > 1` (one tab is a no-op).
            Action {
                name: "Cycle",
                guard: Some(gt(var("count"), int(1))),
                updates: vec![Update {
                    var: "active",
                    expr: if_(
                        gt(add(var("active"), int(1)), sub(var("count"), int(1))),
                        int(0),
                        add(var("active"), int(1)),
                    ),
                }],
            },
            // close(i) for a non-exit close (count > 1, so the window keeps >= 1 tab):
            // `count' = count - 1`, then RE-CLAMP active into the shrunk range. The
            // worst case for the range invariant is closing the LAST (active) tab,
            // where active must drop to the new last index `count - 2`; the faithful
            // re-clamp is `active' = IF active > count - 2 THEN count - 2 ELSE active`
            // (= min(active, new_count - 1), matching `close`'s `else if active >=
            // count { active = count - 1 }` arm). The `Buggy` branch FORGETS the
            // clamp (`active' = active`), so closing the last/active tab leaves
            // `active = count - 1` while the range shrank to `count - 2`.
            Action {
                name: "Close",
                guard: Some(gt(var("count"), int(1))),
                updates: vec![
                    Update {
                        var: "active",
                        expr: if_(
                            eq(cst("Buggy"), int(1)),
                            var("active"),
                            if_(
                                gt(var("active"), sub(var("count"), int(2))),
                                sub(var("count"), int(2)),
                                var("active"),
                            ),
                        ),
                    },
                    Update {
                        var: "count",
                        expr: sub(var("count"), int(1)),
                    },
                ],
            },
        ],
        invariants: vec![
            // A window always has at least one tab.
            Invariant {
                name: "CountPositive",
                expr: gt(var("count"), int(0)),
            },
            // The active index is always in range for the renderer (active <= count-1).
            Invariant {
                name: "ActiveInRange",
                expr: le(var("active"), sub(var("count"), int(1))),
            },
        ],
    }
}

/// SPLIT-PANE TREE INTEGRITY — a tab's `PaneTree` (aterm-gui `pane.rs`) always keeps
/// at least one leaf while the tab is open, and the FOCUSED leaf index never leaves
/// the renderer's `0..leaf_count-1` range. This holds the split-pane feature
/// (Cmd-D / Cmd-Shift-D split, Cmd-W / EOF close) to the same Trust bar as tabs:
/// input + the solid cursor never route to a pane that no longer exists.
///
/// `Buggy` gates the Close re-point: at `Buggy = 0` a Close that removes the focused
/// last leaf drops `focused` to the new last index; at `Buggy = 1` it FORGETS the
/// re-point, leaving `focused = leaf_count` one past the shrunk end (the dangling-
/// focus defect). So `ty` PROVES `FocusInRange` (Buggy=0) and CATCHES it (Buggy=1 →
/// counterexample). SCOPE: only the `CloseOutcome::Collapsed` arm (the tab survives);
/// a `LastPane` close is a tab-machine transition (`tab_nav` / `window_routing`).
pub fn pane_tree_model() -> Model {
    Model {
        name: "PaneTree",
        // Bound the leaf count so `ty` explores a finite space (a tab with up to
        // Cap split panes). `Buggy` flips the Close re-point off.
        consts: vec![("Cap", 4), ("Buggy", 0)],
        // A fresh tab: one leaf (leaf_count=1), it is focused (focused=0).
        vars: vec![
            StateVar {
                name: "leaf_count",
                init: 1,
            },
            StateVar {
                name: "focused",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // split_focused(dir, new): the focused leaf becomes a Split of (original,
            // new); the new pane is the SECOND child -> the new LAST leaf in tree
            // order, and focus moves to it. RHS read the pre-state, so focused' =
            // leaf_count and leaf_count' = leaf_count + 1. Guarded leaf_count <= Cap-1.
            Action {
                name: "Split",
                guard: Some(le(var("leaf_count"), sub(cst("Cap"), int(1)))),
                updates: vec![
                    Update {
                        var: "focused",
                        expr: var("leaf_count"),
                    },
                    Update {
                        var: "leaf_count",
                        expr: add(var("leaf_count"), int(1)),
                    },
                ],
            },
            // close_pane on a SPLIT tab (CloseOutcome::Collapsed, leaf_count > 1):
            // leaf_count' = leaf_count - 1, then RE-POINT focused to a surviving leaf.
            // The real first_leaf/keep-focus index is not a function of the scalar
            // projection, so the faithful update is NONDETERMINISTIC: focused' \in
            // 0..(leaf_count - 2). The in_range MUST be the top-level RHS (the renderer
            // emits focused' \in lo..hi only there; nesting in an IF renders a SET as
            // an = RHS, a type error), so the Buggy flag is folded into the UPPER
            // BOUND: Buggy=0 caps at the new last index leaf_count-2; Buggy=1 stretches
            // the cap to leaf_count-1, one past the end (the forgot-to-re-point defect,
            // where closing the focused last leaf leaves focused = leaf_count - 1).
            Action {
                name: "Close",
                guard: Some(gt(var("leaf_count"), int(1))),
                updates: vec![
                    Update {
                        var: "focused",
                        expr: in_range(
                            int(0),
                            if_(
                                eq(cst("Buggy"), int(1)),
                                sub(var("leaf_count"), int(1)),
                                sub(var("leaf_count"), int(2)),
                            ),
                        ),
                    },
                    Update {
                        var: "leaf_count",
                        expr: sub(var("leaf_count"), int(1)),
                    },
                ],
            },
        ],
        invariants: vec![
            // The tab's tree is never empty while the tab is open (>= 1 leaf).
            Invariant {
                name: "TreeNonEmpty",
                expr: gt(var("leaf_count"), int(0)),
            },
            // Exactly-one in-range focused leaf: focused <= leaf_count - 1.
            Invariant {
                name: "FocusInRange",
                expr: le(var("focused"), sub(var("leaf_count"), int(1))),
            },
        ],
    }
}

/// SESSION-POOL REFCOUNT ACCOUNTING — a pooled session's bookkeeping entry exists
/// exactly while ≥1 window view references it. `refcount` is the live view count
/// (`SessionPool::views`); `closed` is whether the entry has been retired. The
/// invariant `ClosedIffEmpty` (`closed = 1  <=>  refcount = 0`) is the pool's
/// allocation discipline: a session is retired the instant — and only the instant —
/// its last viewer detaches, so the Cmd-Shift-O two-windows-one-session path
/// (refcount 2) never retires early and a fully-detached session never leaks an entry.
///
/// `Buggy = 1` retires on EVERY Release (closes while a co-viewer remains) → `ty`
/// catches the premature-retire counterexample; `Buggy = 0` retires only at
/// refcount 0. The Tier-1 conformance makes the iff NON-VACUOUS by projecting the
/// two variables from TWO INDEPENDENT real signals: `refcount` from the actual count
/// of windows displaying the session (`windows_displaying`, recomputed from the live
/// window/tab structures) and `closed` from pool membership (`views(sid).is_none()`).
/// So a pool that RETIRES a session — dropping its `Session`, closing the PTY — while
/// a window still displays it projects to `[refcount>0, closed=1]`, which `ty`
/// rejects (the use-after-free-on-the-pooled-session hazard). (PTY-fd liveness past
/// the pool entry is a further `SinkWriter`-Arc concern, def2bac — out of scope here.)
pub fn session_pool_model() -> Model {
    Model {
        name: "SessionPool",
        consts: vec![("Cap", 4), ("Buggy", 0)],
        vars: vec![
            StateVar {
                name: "refcount",
                init: 1,
            },
            StateVar {
                name: "closed",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // attach (a 2nd+ window views the same session, Cmd-Shift-O): refcount+1,
            // guarded not-yet-closed and below the model bound.
            Action {
                name: "Acquire",
                guard: Some(and_(
                    eq(var("closed"), int(0)),
                    le(var("refcount"), sub(cst("Cap"), int(1))),
                )),
                updates: vec![Update {
                    var: "refcount",
                    expr: add(var("refcount"), int(1)),
                }],
            },
            // detach (a window stops viewing): refcount-1; retire (closed=1) IFF that
            // was the last viewer. Buggy retires on every detach.
            Action {
                name: "Release",
                guard: Some(gt(var("refcount"), int(0))),
                updates: vec![
                    Update {
                        var: "refcount",
                        expr: sub(var("refcount"), int(1)),
                    },
                    Update {
                        var: "closed",
                        expr: if_(
                            eq(cst("Buggy"), int(1)),
                            int(1),
                            if_(
                                eq(sub(var("refcount"), int(1)), int(0)),
                                int(1),
                                var("closed"),
                            ),
                        ),
                    },
                ],
            },
        ],
        invariants: vec![Invariant {
            name: "ClosedIffEmpty",
            expr: and_(
                or_(eq(var("closed"), int(0)), eq(var("refcount"), int(0))),
                or_(eq(var("closed"), int(1)), gt(var("refcount"), int(0))),
            ),
        }],
    }
}

/// TAB-STRIP PARITY — the NATIVE macOS titlebar tab strip (`toolbar.rs`'s
/// `NSSegmentedControl`) can never DESYNC from the proven tab model. The strip is a
/// pure MIRROR of the tab set: its `segmentCount` must always equal the tab `count`
/// and its `selectedSegment` the `active` tab, and — since AppKit will index it — the
/// selection must stay in range (`selected <= seg_count - 1`). The sink maintaining
/// the mirror is `set_window_tabs(handle, titles, active)`, driven from
/// `App::refresh_window_tabs` after EVERY tab mutation.
///
/// Two-lane self-composition (cf. [`coalesce_model`]): one event stream drives BOTH a
/// TRUTH lane `(count, active)` (the `TabIndex` machine of [`tab_nav_model`]) and a
/// STRIP lane `(seg_count, selected)` (the control), re-synced to mirror the truth
/// after each action. `ty` PROVES `StripMirrorsTruth` @Buggy=0; the Buggy branch in
/// Close DROPS the strip re-sync (a missed `refresh_window_tabs`), freezing BOTH strip
/// vars stale — so the strip shows an extra segment with an out-of-range selection,
/// caught @Buggy=1.
///
/// Tier-1: bound by `tab_strip_conformance` (aterm-gui), which projects the TRUTH lane
/// `(count, active)` from `ws.tabs` AND the STRIP lane `(seg_count, selected)` from
/// `WindowState::strip_shadow` — a faithful record of what `refresh_window_tabs` last
/// pushed to the native `NSSegmentedControl`. The two signals are INDEPENDENT (a tab
/// mutation that forgets to re-sync a window's strip leaves the shadow stale), so the
/// load-bearing case — closing a tab in a NON-FRONT window — is a real, ty-rejected
/// desync unless `close_tab_at` re-syncs THAT window's strip (the fix this drove).
pub fn tab_strip_model() -> Model {
    // Re-clamp of the active/selected index after a Close shrinks the count by one:
    // min(idx, new_count - 1) = IF idx > count - 2 THEN count - 2 ELSE idx (RHS reads
    // the PRE-state count, so new_count - 1 = count - 2). Used for the TRUTH lane's
    // active' and, in the correct path, the STRIP lane's selected'.
    let reclamp = || {
        if_(
            gt(var("active"), sub(var("count"), int(2))),
            sub(var("count"), int(2)),
            var("active"),
        )
    };
    Model {
        name: "TabStrip",
        // Bound the tab count so `ty` explores a finite space. `Buggy` flips the Close
        // strip re-sync off (the forgot-to-refresh defect).
        consts: vec![("Cap", 4), ("Buggy", 0)],
        vars: vec![
            // TRUTH lane (the TabIndex machine).
            StateVar {
                name: "count",
                init: 1,
            },
            StateVar {
                name: "active",
                init: 0,
            },
            // STRIP lane (the NSSegmentedControl mirror).
            StateVar {
                name: "seg_count",
                init: 1,
            },
            StateVar {
                name: "selected",
                init: 0,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // open_tab_in -> TabIndex::add(): append + switch, then refresh re-syncs the
            // strip verbatim. active' = count (the new last index); strip mirrors.
            Action {
                name: "NewTab",
                guard: Some(le(var("count"), sub(cst("Cap"), int(1)))),
                updates: vec![
                    Update {
                        var: "active",
                        expr: var("count"),
                    },
                    Update {
                        var: "count",
                        expr: add(var("count"), int(1)),
                    },
                    Update {
                        var: "selected",
                        expr: var("count"),
                    },
                    Update {
                        var: "seg_count",
                        expr: add(var("count"), int(1)),
                    },
                ],
            },
            // switch_tab_in / cycle_tab: move active, then refresh re-syncs the strip to
            // the NEW active — both lanes move in LOCKSTEP. Modelled as the deterministic
            // cycle(true) wrap (a genuine shipping transition); selected' mirrors the
            // SAME pre-state expression so the lanes never diverge spuriously.
            Action {
                name: "SelectTab",
                guard: Some(gt(var("count"), int(1))),
                updates: vec![
                    Update {
                        var: "active",
                        expr: if_(
                            gt(add(var("active"), int(1)), sub(var("count"), int(1))),
                            int(0),
                            add(var("active"), int(1)),
                        ),
                    },
                    Update {
                        var: "selected",
                        expr: if_(
                            gt(add(var("active"), int(1)), sub(var("count"), int(1))),
                            int(0),
                            add(var("active"), int(1)),
                        ),
                    },
                ],
            },
            // close_tab_at -> TabIndex::close(i), non-exit (count > 1): count' = count-1,
            // active re-clamped; the strip is then re-synced (seg_count' = count',
            // selected' = active'). The Buggy branch DROPS that re-sync (the close forgot
            // refresh_window_tabs on a non-front window), freezing BOTH strip vars at
            // their stale pre-close values — so seg_count outlives the tab it counted and
            // selected points past the new end when the last/active tab was closed.
            Action {
                name: "Close",
                guard: Some(gt(var("count"), int(1))),
                updates: vec![
                    Update {
                        var: "active",
                        expr: reclamp(),
                    },
                    Update {
                        var: "count",
                        expr: sub(var("count"), int(1)),
                    },
                    Update {
                        var: "seg_count",
                        expr: if_(
                            eq(cst("Buggy"), int(1)),
                            var("seg_count"),
                            sub(var("count"), int(1)),
                        ),
                    },
                    Update {
                        var: "selected",
                        expr: if_(eq(cst("Buggy"), int(1)), var("selected"), reclamp()),
                    },
                ],
            },
        ],
        invariants: vec![
            // The native strip can never desync from the proven tab model: its segment
            // count mirrors the tab count, its selection mirrors the active tab, and the
            // selection is a valid segment index AppKit can highlight.
            Invariant {
                name: "StripMirrorsTruth",
                expr: and_(
                    and_(
                        eq(var("seg_count"), var("count")),
                        eq(var("selected"), var("active")),
                    ),
                    le(var("selected"), sub(var("seg_count"), int(1))),
                ),
            },
        ],
    }
}

/// The GLOBAL control-socket `ActiveHandle` mirror (the `active_handle` in aterm-gui's
/// `App`). The control socket has ONE global handle that introspection/drive verbs
/// (`text`/`feed`/`signal`) resolve through (`resolve_active`); it MUST always name the
/// session the user is actually looking at — the FRONTMOST window's active tab's
/// focused pane (the TRUTH lane). The window analog of [`tab_strip_model`]'s
/// per-window strip mirror: same two-lane (truth/mirror) parity discipline, but for the
/// PROCESS-WIDE control target rather than the native chrome.
///
/// `ty` PROVES `HandleMirrorsFront` at `Buggy=0` — every path that moves the front
/// window's active session ALSO re-points the global handle (the
/// `resync_active_or_window` -> `sync_active_session` discipline), so the two lanes
/// never diverge under ANY interleaving of front-active changes — and CATCHES the
/// "swallow class" at `Buggy=1` (a close-collapse / new-window path that re-mirrors only
/// the PER-WINDOW state via `sync_window` and forgets the global re-point) ->
/// counterexample on `HandleMirrorsFront`. That is exactly the defect class fixed by
/// routing `apply_close_outcome` / `create_window_internal` / `push_stub_tab` through
/// `resync_active_or_window`: without it the control socket keeps driving a stale, or
/// just-closed, session — and `Owner`/aterm-ctl verbs bypass the per-request edge gate,
/// so they hit whatever the stale handle points at.
pub fn active_handle_model() -> Model {
    Model {
        name: "ActiveHandle",
        // Bound the fresh-session id space so `ty` explores a finite, terminating space.
        // `Buggy` flips the global re-sync OFF on the close/new-window lane (the swallow).
        consts: vec![("MaxId", 4), ("Buggy", 0)],
        vars: vec![
            // TRUTH lane: the frontmost window's CURRENT active-tab focused-pane session.
            StateVar {
                name: "truth",
                init: 1,
            },
            // MIRROR lane: the global control `ActiveHandle`'s target session.
            StateVar {
                name: "handle",
                init: 1,
            },
            // A strictly-increasing fresh-session allocator, so each change moves the
            // front active session to a DISTINCT id (a stale handle is then observable).
            StateVar {
                name: "next",
                init: 2,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // The ALWAYS-CORRECT lockstep path (open_tab_in / switch_tab / move_tab):
            // the front active session moves to a fresh session and the global handle is
            // re-pointed in lockstep (`if frontmost { sync_active_session }`). Both lanes
            // move together — this path was never buggy.
            Action {
                name: "SwitchActive",
                guard: Some(le(var("next"), sub(cst("MaxId"), int(1)))),
                updates: vec![
                    Update {
                        var: "truth",
                        expr: var("next"),
                    },
                    Update {
                        var: "handle",
                        expr: var("next"),
                    },
                    Update {
                        var: "next",
                        expr: add(var("next"), int(1)),
                    },
                ],
            },
            // The SWALLOW-PRONE path (apply_close_outcome's pane-collapse / tab-close and
            // create_window_internal's new front window): the front active session moves
            // to a fresh session. The FIX re-points the global handle too
            // (`resync_active_or_window` -> `sync_active_session`); the Buggy branch
            // re-mirrors only the per-window state (`sync_window`) and LEAVES THE GLOBAL
            // HANDLE STALE on the just-closed / previous session.
            Action {
                name: "CloseOrNewFront",
                guard: Some(le(var("next"), sub(cst("MaxId"), int(1)))),
                updates: vec![
                    Update {
                        var: "truth",
                        expr: var("next"),
                    },
                    Update {
                        var: "handle",
                        expr: if_(eq(cst("Buggy"), int(1)), var("handle"), var("next")),
                    },
                    Update {
                        var: "next",
                        expr: add(var("next"), int(1)),
                    },
                ],
            },
        ],
        invariants: vec![
            // The global control handle always names the session the user is actually
            // looking at in the frontmost window — so a control verb never drives a
            // stale or just-closed session.
            Invariant {
                name: "HandleMirrorsFront",
                expr: eq(var("handle"), var("truth")),
            },
        ],
    }
}

/// The cross-process `@<child>` proxy FORWARD is acyclic and TERMINATES (control.rs
/// `proxy_forward_plan` / `try_proxy_forward`). The recursion-topology refactor REMOVED
/// the explicit hop-counter cap, leaving ONE structural invariant as the sole guard
/// against a forward loop: the parent rewrites the child's own selector to `@.` (run on
/// self) before relaying, so the child resolves the verb LOCALLY and never re-enters
/// `try_proxy_forward`. A forward chain is therefore at most ONE cross-process hop — a
/// child is never in its own proxy table, and the `@.` rewrite means it can't forward
/// onward — so no A→B→A ping-pong (or unbounded relay-thread/fd growth) can form.
///
/// `ty` PROVES `OneHopNoCycle` at Buggy=0 — the rewrite-to-`@.` discipline caps the
/// chain at depth 1 under any interleaving over the bounded space — and CATCHES the
/// loop class at Buggy=1 (a forward that relays the ORIGINAL cross-selector instead of
/// `@.`, so the child re-forwards and the chain grows past one hop) -> counterexample on
/// `OneHopNoCycle`. This locks in the safety the removed hop-cap used to provide: if the
/// `@.` rewrite ever regresses, the exhaustive check fails.
pub fn proxy_forward_model() -> Model {
    Model {
        name: "ProxyForward",
        // MaxDepth bounds `ty`'s exploration; the SAFETY bound the invariant asserts is 1.
        // `Buggy` flips the `@.` rewrite off (relay the original cross-selector → re-forward).
        consts: vec![("MaxDepth", 2), ("Buggy", 0)],
        vars: vec![
            // Cross-process hops taken by the in-flight forward chain so far.
            StateVar {
                name: "depth",
                init: 0,
            },
            // Is a request still in flight and eligible to forward onward?
            StateVar {
                name: "active",
                init: 1,
            },
        ],
        fn_vars: vec![],
        actions: vec![
            // One forward hop: the parent dials the child and relays. The FIX rewrites
            // the child's selector to `@.`, so the child runs the verb on ITSELF and the
            // chain ENDS (active' = 0). The Buggy branch relays the original cross
            // selector, so the child re-forwards and the chain CONTINUES (active' = 1).
            Action {
                name: "Forward",
                guard: Some(and_(
                    eq(var("active"), int(1)),
                    le(var("depth"), sub(cst("MaxDepth"), int(1))),
                )),
                updates: vec![
                    Update {
                        var: "depth",
                        expr: add(var("depth"), int(1)),
                    },
                    Update {
                        var: "active",
                        expr: if_(eq(cst("Buggy"), int(1)), int(1), int(0)),
                    },
                ],
            },
        ],
        invariants: vec![
            // A forward chain is at most one cross-process hop — never a cycle or
            // unbounded recursion (which would exhaust relay threads / fds).
            Invariant {
                name: "OneHopNoCycle",
                expr: le(var("depth"), int(1)),
            },
        ],
    }
}

// ===========================================================================
// Generalized error-CLASS models (audit findings F1, ordering, reply-fidelity).
// These teach Trust to catch the *classes* the second bug-hunt surfaced — not
// just the specific bugs — so a future regression of the same shape fails the
// exhaustive `ty` check. Same Buggy convention as the safety models above.
// ===========================================================================

/// CAPABILITY SECRECY / information-flow (audit finding F1). A bearer-token SECRET
/// must reach a SANDBOXED same-uid peer (one that cannot read 0600 files) only if
/// it is placed in an INHERITABLE env sink; routed through a 0600 file it must not.
/// `published` is the channel (0 none, 1 file, 2 env); `peer_has` is whether the
/// sandboxed peer obtained the secret. `Buggy = 1` chooses the env channel (the
/// original design); `Buggy = 0` the 0600-file channel (the F1 fix).
///
/// Invariant `NoSecretToSandboxedPeer`: the sandboxed peer never holds the token.
/// `ty` proves it for the file channel and catches the env-channel disclosure —
/// a genuinely new property CLASS: explicit information flow of a secret to an
/// untrusted sink.
pub fn capability_secrecy_model() -> Model {
    props::two_stage_leak(props::TwoStage {
        name: "CapabilitySecrecy",
        // Provision over the 0600-file channel (1) or, when Buggy, the inheritable
        // env channel (2).
        stage: "published",
        stage_act: "Provision",
        stage_rhs: if_(eq(cst("Buggy"), int(1)), int(2), int(1)),
        // A sandboxed peer obtains the secret ONLY from the env channel (2).
        leak: "peer_has",
        leak_act: "SandboxedRead",
        leak_guard: gt(var("published"), int(0)),
        leak_rhs: if_(eq(var("published"), int(2)), int(1), int(0)),
        inv: "NoSecretToSandboxedPeer",
        inv_expr: eq(var("peer_has"), int(0)),
    })
}

/// PUBLISH ORDERING (the graph-entry-before-bind race). A discovery entry must be
/// published only AFTER the socket is bound, so a concurrent stale-sweep can never
/// see an entry pointing at a not-yet-bound socket and delete it. `Buggy = 1`
/// publishes before binding (the original main-thread write); `Buggy = 0` requires
/// `bound` first (publish from inside `spawn` after `bind`).
///
/// Invariant `PublishImpliesBound`: `published ⟹ bound`. `ty` proves the ordered
/// discipline and catches the pre-bind publish.
pub fn publish_ordering_model() -> Model {
    props::happens_before(props::Ordering {
        name: "PublishOrdering",
        a: "bound",
        a_act: "Bind",
        b: "published",
        b_act: "Publish",
        inv: "PublishImpliesBound",
    })
}

/// REPLY FIDELITY (the ERR-after-delivery defect). Once a forwarded verb has been
/// DELIVERED to the child, a later relay-stage failure must NOT report `ERR` to
/// the client (a false "didn't happen" for an op that did). `delivered` and
/// `reported_err` are booleans; `Buggy = 1` reports the error after delivery (the
/// original `connect_and_relay` returning the relay error), `Buggy = 0` swallows it
/// (return Ok once delivered — the fix).
///
/// Invariant `NoErrorAfterDelivery`: never both delivered AND error-reported.
pub fn reply_fidelity_model() -> Model {
    props::two_stage_leak(props::TwoStage {
        name: "ReplyFidelity",
        stage: "delivered",
        stage_act: "Deliver",
        stage_rhs: int(1),
        leak: "reported_err",
        leak_act: "RelayFail",
        leak_guard: and_(
            eq(var("delivered"), int(1)),
            eq(var("reported_err"), int(0)),
        ),
        leak_rhs: if_(eq(cst("Buggy"), int(1)), int(1), int(0)),
        inv: "NoErrorAfterDelivery",
        inv_expr: or_(
            eq(var("delivered"), int(0)),
            eq(var("reported_err"), int(0)),
        ),
    })
}

/// Property-combinator generators: each returns a fully-formed, `Buggy`-gated
/// [`Model`] (prove@Buggy=0, counterexample@Buggy=1) for a recurring property
/// CLASS. A new property is a struct literal + one registry/harness line, not 50
/// lines of `Expr` constructors. Every name (model / action / var / invariant) is
/// threaded through, so the emitted TLA+ is whatever the author wants — the 7
/// introspection models below are byte-identical instances of these generators.
///
/// The classes (the recurring shapes of the hand-built models): lifecycle/no-leak,
/// gated-completeness, happens-before, teardown-clears, two-stage-leak (info-flow /
/// reply-fidelity), and liveness/no-wedge.
pub mod props {
    use super::*;

    /// LOOSEN the buggy case: `(Buggy = 1 \/ cond)` — Buggy admits MORE (ordering).
    fn or_buggy(cond: Expr) -> Expr {
        or_(eq(cst("Buggy"), int(1)), cond)
    }
    /// RESTRICT to `cond` when buggy; correct admits freely: `(Buggy = 0 \/ cond)`.
    fn or_correct(cond: Expr) -> Expr {
        or_(eq(cst("Buggy"), int(0)), cond)
    }

    // ---- CLASS 1: lifecycle / no-leak ----
    pub struct Lifecycle {
        pub name: &'static str,
        pub live: &'static str,
        pub reg: &'static str,
        pub max: &'static str,
        pub max_val: i64,
        pub acquire: &'static str,
        pub release: &'static str,
        pub inv: &'static str,
    }
    /// `acquire` bumps both counters (bounded by `max`); `release` decrements `live`
    /// always and `reg` UNLESS Buggy (forgot to deregister). Invariant `reg =< live`.
    pub fn lifecycle_no_leak(p: Lifecycle) -> Model {
        Model {
            name: p.name,
            consts: vec![(p.max, p.max_val), ("Buggy", 0)],
            vars: vec![
                StateVar {
                    name: p.live,
                    init: 0,
                },
                StateVar {
                    name: p.reg,
                    init: 0,
                },
            ],
            fn_vars: vec![],
            actions: vec![
                Action {
                    name: p.acquire,
                    guard: Some(le(add(var(p.live), int(1)), cst(p.max))),
                    updates: vec![
                        Update {
                            var: p.live,
                            expr: add(var(p.live), int(1)),
                        },
                        Update {
                            var: p.reg,
                            expr: add(var(p.reg), int(1)),
                        },
                    ],
                },
                Action {
                    name: p.release,
                    guard: Some(gt(var(p.live), int(0))),
                    updates: vec![
                        Update {
                            var: p.live,
                            expr: sub(var(p.live), int(1)),
                        },
                        Update {
                            var: p.reg,
                            expr: if_(
                                eq(cst("Buggy"), int(1)),
                                var(p.reg),
                                sub(var(p.reg), int(1)),
                            ),
                        },
                    ],
                },
            ],
            invariants: vec![Invariant {
                name: p.inv,
                expr: le(var(p.reg), var(p.live)),
            }],
        }
    }

    // ---- CLASS 2: gated completeness ----
    pub struct Gated {
        pub name: &'static str,
        pub item: &'static str,
        pub decided: &'static str,
        pub domain_max: &'static str,
        pub domain_val: i64,
        pub fwd_hi: i64,
        pub drop: i64,
        pub good: i64,
        pub bad: i64,
        pub pick: &'static str,
        pub route: &'static str,
        pub inv: &'static str,
    }
    /// `pick` fans `item' \in 0..domain_max`; `route` sets `decided := good` iff
    /// `item =< fwd_hi` AND (correct OR `item # drop`), else `bad`. Buggy drops the
    /// `drop` element. Invariant: `decided=0 \/ item>fwd_hi \/ decided=good`.
    pub fn gated_completeness(p: Gated) -> Model {
        Model {
            name: p.name,
            consts: vec![(p.domain_max, p.domain_val), ("Buggy", 0)],
            vars: vec![
                StateVar {
                    name: p.item,
                    init: 0,
                },
                StateVar {
                    name: p.decided,
                    init: 0,
                },
            ],
            fn_vars: vec![],
            actions: vec![
                Action {
                    name: p.pick,
                    guard: Some(eq(var(p.decided), int(0))),
                    updates: vec![Update {
                        var: p.item,
                        expr: in_range(int(0), cst(p.domain_max)),
                    }],
                },
                Action {
                    name: p.route,
                    guard: Some(eq(var(p.decided), int(0))),
                    updates: vec![Update {
                        var: p.decided,
                        expr: if_(
                            and_(
                                le(var(p.item), int(p.fwd_hi)),
                                or_correct(neq(var(p.item), int(p.drop))),
                            ),
                            int(p.good),
                            int(p.bad),
                        ),
                    }],
                },
            ],
            invariants: vec![Invariant {
                name: p.inv,
                expr: or_(
                    eq(var(p.decided), int(0)),
                    or_(
                        gt(var(p.item), int(p.fwd_hi)),
                        eq(var(p.decided), int(p.good)),
                    ),
                ),
            }],
        }
    }

    // ---- CLASS 3: happens-before (latch-pair ordering) ----
    pub struct Ordering {
        pub name: &'static str,
        pub a: &'static str,
        pub a_act: &'static str,
        pub b: &'static str,
        pub b_act: &'static str,
        pub inv: &'static str,
    }
    /// `a_act` sets `a:=1`; `b_act` (guard `b=0 /\ (Buggy=1 \/ a=1)`) sets `b:=1`.
    /// Buggy lets `b` race ahead of `a`. Invariant: `b=0 \/ a=1`.
    pub fn happens_before(p: Ordering) -> Model {
        Model {
            name: p.name,
            consts: vec![("Buggy", 0)],
            vars: vec![
                StateVar { name: p.a, init: 0 },
                StateVar { name: p.b, init: 0 },
            ],
            fn_vars: vec![],
            actions: vec![
                Action {
                    name: p.a_act,
                    guard: Some(eq(var(p.a), int(0))),
                    updates: vec![Update {
                        var: p.a,
                        expr: int(1),
                    }],
                },
                Action {
                    name: p.b_act,
                    guard: Some(and_(eq(var(p.b), int(0)), or_buggy(eq(var(p.a), int(1))))),
                    updates: vec![Update {
                        var: p.b,
                        expr: int(1),
                    }],
                },
            ],
            invariants: vec![Invariant {
                name: p.inv,
                expr: or_(eq(var(p.b), int(0)), eq(var(p.a), int(1))),
            }],
        }
    }

    // ---- CLASS 4: teardown clears N flags ----
    pub struct Teardown {
        pub name: &'static str,
        pub flags: Vec<&'static str>,
        pub gate: &'static str,
        pub act: &'static str,
        pub inv: &'static str,
    }
    /// `act` (guard `gate=0`) drops every flag to 0 (Buggy leaves them 1) and sets
    /// `gate:=1`. Invariant: `gate=0 \/ AND(flag=0)`. Flags init 1.
    pub fn teardown_clears(p: Teardown) -> Model {
        let mut vars: Vec<StateVar> = p
            .flags
            .iter()
            .map(|f| StateVar { name: f, init: 1 })
            .collect();
        vars.push(StateVar {
            name: p.gate,
            init: 0,
        });
        let mut updates: Vec<Update> = p
            .flags
            .iter()
            .map(|f| Update {
                var: f,
                expr: if_(eq(cst("Buggy"), int(1)), int(1), int(0)),
            })
            .collect();
        updates.push(Update {
            var: p.gate,
            expr: int(1),
        });
        let closed = p
            .flags
            .iter()
            .map(|f| eq(var(f), int(0)))
            .reduce(and_)
            .expect("teardown_clears needs at least one flag");
        Model {
            name: p.name,
            consts: vec![("Buggy", 0)],
            vars,
            fn_vars: vec![],
            actions: vec![Action {
                name: p.act,
                guard: Some(eq(var(p.gate), int(0))),
                updates,
            }],
            invariants: vec![Invariant {
                name: p.inv,
                expr: or_(eq(var(p.gate), int(0)), closed),
            }],
        }
    }

    // ---- CLASS 5: two-stage leak (info-flow / reply-fidelity) ----
    pub struct TwoStage {
        pub name: &'static str,
        pub stage: &'static str,
        pub stage_act: &'static str,
        pub stage_rhs: Expr,
        pub leak: &'static str,
        pub leak_act: &'static str,
        pub leak_guard: Expr,
        pub leak_rhs: Expr,
        pub inv: &'static str,
        pub inv_expr: Expr,
    }
    /// `stage_act` (guard `stage=0`) sets `stage := stage_rhs`; `leak_act` (guard
    /// caller-supplied) sets `leak := leak_rhs`. The RHS exprs let the leak fire on a
    /// stage VALUE (secrecy) OR on `Buggy` directly (reply-fidelity).
    pub fn two_stage_leak(p: TwoStage) -> Model {
        Model {
            name: p.name,
            consts: vec![("Buggy", 0)],
            vars: vec![
                StateVar {
                    name: p.stage,
                    init: 0,
                },
                StateVar {
                    name: p.leak,
                    init: 0,
                },
            ],
            fn_vars: vec![],
            actions: vec![
                Action {
                    name: p.stage_act,
                    guard: Some(eq(var(p.stage), int(0))),
                    updates: vec![Update {
                        var: p.stage,
                        expr: p.stage_rhs,
                    }],
                },
                Action {
                    name: p.leak_act,
                    guard: Some(p.leak_guard),
                    updates: vec![Update {
                        var: p.leak,
                        expr: p.leak_rhs,
                    }],
                },
            ],
            invariants: vec![Invariant {
                name: p.inv,
                expr: p.inv_expr,
            }],
        }
    }

    // ---- CLASS 6: liveness / no-wedge ----
    pub struct Wedge {
        pub name: &'static str,
        pub buffered: &'static str,
        pub buffered_init: i64,
        pub relayed: &'static str,
        pub waiting: &'static str,
        pub relay: &'static str,
        pub recv: &'static str,
        pub done: &'static str,
        pub inv: &'static str,
    }
    /// Two-party request→relay→reply. `relay` guard `(Buggy=0 \/ relayed>0) /\
    /// buffered>0` (Buggy demands a fresh read before the first relay → wedge);
    /// `recv` serves the client; `done` is the MANDATORY guarded zero-update
    /// self-loop on the served terminal (else `ty` flags it as a false deadlock).
    /// Pair with [`Model::to_cfg_deadlock_with`].
    pub fn no_wedge(p: Wedge) -> Model {
        Model {
            name: p.name,
            consts: vec![("Buggy", 0)],
            vars: vec![
                StateVar {
                    name: p.buffered,
                    init: p.buffered_init,
                },
                StateVar {
                    name: p.relayed,
                    init: 0,
                },
                StateVar {
                    name: p.waiting,
                    init: 1,
                },
            ],
            fn_vars: vec![],
            actions: vec![
                Action {
                    name: p.relay,
                    guard: Some(and_(
                        or_correct(gt(var(p.relayed), int(0))),
                        gt(var(p.buffered), int(0)),
                    )),
                    updates: vec![
                        Update {
                            var: p.relayed,
                            expr: add(var(p.relayed), int(1)),
                        },
                        Update {
                            var: p.buffered,
                            expr: sub(var(p.buffered), int(1)),
                        },
                    ],
                },
                Action {
                    name: p.recv,
                    guard: Some(and_(gt(var(p.relayed), int(0)), eq(var(p.waiting), int(1)))),
                    updates: vec![Update {
                        var: p.waiting,
                        expr: int(0),
                    }],
                },
                Action {
                    name: p.done,
                    guard: Some(eq(var(p.waiting), int(0))),
                    updates: vec![],
                },
            ],
            invariants: vec![Invariant {
                name: p.inv,
                expr: le(var(p.waiting), int(1)),
            }],
        }
    }

    // ---- CLASS 7: conjunctive authorization soundness ----
    pub struct ConjunctiveAuthz {
        pub name: &'static str,
        pub guards: Vec<&'static str>,
        pub decided: &'static str,
        pub pick: &'static str,
        pub decide: &'static str,
        pub drop: &'static str,
        pub inv: &'static str,
    }
    /// `pick` sets each guard nondeterministically in `{0,1}`; `decide` PERMITS (2)
    /// iff EVERY guard holds, else DENIES (1) — except Buggy IGNORES the `drop` guard.
    /// Which conjunct is dropped is the bug class: dst → confused-deputy, op → op
    /// confusion, nonce → replay, token → forgery. Invariant: a permit implies EVERY
    /// guard truly held (authorization SOUNDNESS — the `decide_edge` predicate).
    pub fn conjunctive_authz(p: ConjunctiveAuthz) -> Model {
        let mut vars: Vec<StateVar> = p
            .guards
            .iter()
            .map(|g| StateVar { name: g, init: 0 })
            .collect();
        vars.push(StateVar {
            name: p.decided,
            init: 0,
        });
        let pick_updates: Vec<Update> = p
            .guards
            .iter()
            .map(|g| Update {
                var: g,
                expr: in_range(int(0), int(1)),
            })
            .collect();
        // Permit condition: AND of all guards; the `drop` conjunct is WAIVED when Buggy.
        let permit_when = p
            .guards
            .iter()
            .map(|g| {
                if *g == p.drop {
                    or_buggy(eq(var(g), int(1)))
                } else {
                    eq(var(g), int(1))
                }
            })
            .reduce(and_)
            .expect("conjunctive_authz needs at least one guard");
        // Soundness: a permit must imply EVERY guard actually held (no Buggy waiver).
        let all_hold = p
            .guards
            .iter()
            .map(|g| eq(var(g), int(1)))
            .reduce(and_)
            .expect("conjunctive_authz needs at least one guard");
        Model {
            name: p.name,
            consts: vec![("Buggy", 0)],
            vars,
            fn_vars: vec![],
            actions: vec![
                Action {
                    name: p.pick,
                    guard: Some(eq(var(p.decided), int(0))),
                    updates: pick_updates,
                },
                Action {
                    name: p.decide,
                    guard: Some(eq(var(p.decided), int(0))),
                    updates: vec![Update {
                        var: p.decided,
                        expr: if_(permit_when, int(2), int(1)),
                    }],
                },
            ],
            invariants: vec![Invariant {
                name: p.inv,
                expr: or_(neq(var(p.decided), int(2)), all_hold),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tla_check::TlaSpec;

    /// Guard the brittle `.replace()` in `to_cfg_deadlock_with`: the deadlock cfg
    /// flips CHECK_DEADLOCK to TRUE, and the DEFAULT cfg path stays FALSE (so the
    /// 14 existing models that rely on FALSE are untouched). If `to_cfg_with`'s
    /// literal ever changes, this fails loudly rather than silently no-op'ing.
    #[test]
    fn deadlock_cfg_flips_the_line() {
        let m = forward_handshake_model();
        let dl = m.to_cfg_deadlock_with(&[]);
        assert!(
            dl.contains("CHECK_DEADLOCK TRUE\n"),
            "deadlock cfg must enable the check:\n{dl}"
        );
        assert!(
            !dl.contains("CHECK_DEADLOCK FALSE"),
            "deadlock cfg must not also disable it:\n{dl}"
        );
        assert!(
            m.to_cfg().contains("CHECK_DEADLOCK FALSE\n"),
            "default cfg path unchanged"
        );
    }

    #[test]
    fn ring_generates_expected_tla() {
        let tla = ring_model().to_tla();
        // Spot-check the mechanical translation (<= => =<, if => IF/THEN/ELSE).
        assert!(tla.contains("---- MODULE Ring ----"), "{tla}");
        assert!(tla.contains("CONSTANT MaxSeq, Cap"), "{tla}");
        assert!(tla.contains("VARIABLES seq, lo"), "{tla}");
        assert!(tla.contains("Init == seq = 0 /\\ lo = 1"), "{tla}");
        assert!(
            tla.contains("Push == seq =< MaxSeq - 1 /\\ seq' = seq + 1 /\\ lo' = (IF seq + 1 - lo + 1 > Cap THEN lo + 1 ELSE lo)"),
            "{tla}"
        );
        assert!(tla.contains("Next == Push"), "{tla}");
        assert!(tla.contains("Spec == Init /\\ [][Next]_vars"), "{tla}");
        assert!(tla.contains("LenBounded == seq - lo + 1 =< Cap"), "{tla}");
    }

    #[test]
    fn generated_tla_parses_and_exposes_its_defs() {
        // The generated module round-trips through the TLA+ parser, and the
        // action + invariant names are visible (cross-check generator <-> parser).
        let tla = ring_model().to_tla();
        let spec = TlaSpec::parse_str(&tla, "Ring.tla").expect("generated TLA+ must parse");
        assert_eq!(spec.module_name, "Ring");
        assert!(spec.actions.contains("Push"), "defs: {:?}", spec.actions);
        assert!(
            spec.actions.contains("LenBounded"),
            "defs: {:?}",
            spec.actions
        );
    }

    #[test]
    fn transition_spec_parameterizes_init_but_shares_the_action() {
        let m = ring_model();
        let tla = m.transition_spec();
        assert!(
            tla.contains("CONSTANT MaxSeq, Cap, seq_init, lo_init"),
            "{tla}"
        );
        assert!(
            tla.contains("Init == seq = seq_init /\\ lo = lo_init"),
            "{tla}"
        );
        // The action / Next / Spec are the SAME source as the concrete form.
        assert!(
            tla.contains("Push == seq =< MaxSeq - 1 /\\ seq' = seq + 1 /\\ lo' = (IF"),
            "{tla}"
        );
        let mut init = BTreeMap::new();
        init.insert("seq", 42i64);
        init.insert("lo", 1i64);
        let cfg = m.transition_cfg(&init, &[("Cap", 65536), ("MaxSeq", 1_000_000)]);
        assert!(cfg.contains("CONSTANT Cap = 65536"), "{cfg}");
        assert!(cfg.contains("CONSTANT MaxSeq = 1000000"), "{cfg}");
        assert!(cfg.contains("CONSTANT seq_init = 42"), "{cfg}");
        assert!(cfg.contains("CONSTANT lo_init = 1"), "{cfg}");
    }

    #[test]
    fn cursor_model_emits_unchanged_and_disjunctive_next() {
        // The paths the ring never exercises: partial updates -> UNCHANGED, and
        // two actions -> a disjunctive Next.
        let tla = cursor_model().to_tla();
        assert!(
            tla.contains("Grow == seq =< MaxSeq - 1 /\\ seq' = seq + 1 /\\ UNCHANGED << cursor >>"),
            "{tla}"
        );
        assert!(
            tla.contains("Deliver == seq > cursor /\\ cursor' = seq /\\ UNCHANGED << seq >>"),
            "{tla}"
        );
        assert!(tla.contains("Next == Grow \\/ Deliver"), "{tla}");
        assert!(tla.contains("CursorBounded == cursor =< seq"), "{tla}");
    }

    #[test]
    fn cursor_interpreter_holds_invariant_under_both_actions() {
        let m = cursor_model();
        let mut st = m.init_state();
        // Interleave writer growth and reader delivery; the reader must never pass
        // the writer, and `Deliver` must leave `seq` UNCHANGED, `Grow` leave `cursor`.
        for _ in 0..3 {
            let before_cursor = st[&"cursor"];
            assert!(m.fire("Grow", &mut st));
            assert_eq!(
                st[&"cursor"], before_cursor,
                "Grow must leave cursor UNCHANGED"
            );
            assert!(m.check_invariant("CursorBounded", &st));
            let before_seq = st[&"seq"];
            assert!(m.fire("Deliver", &mut st));
            assert_eq!(st[&"seq"], before_seq, "Deliver must leave seq UNCHANGED");
            assert_eq!(
                st[&"cursor"], st[&"seq"],
                "Deliver catches the reader up to the writer"
            );
            assert!(m.check_invariant("CursorBounded", &st));
        }
        // Deliver is guarded by seq > cursor; once caught up it cannot fire.
        assert!(
            !m.fire("Deliver", &mut st),
            "Deliver guard (seq > cursor) blocks when caught up"
        );
    }

    #[test]
    fn subscribe_emits_parenthesized_disjunction_guard_and_eq() {
        let tla = subscribe_model().to_tla();
        // The disjunctive guard MUST be parenthesized, else `/\` captures it.
        assert!(
            tla.contains(
                "PollDeliver == (Buggy = 1 \\/ lo =< cursor + 1) /\\ cursor' = seq /\\ \
                 lost' = (IF lo > cursor + 1 THEN 1 ELSE lost) /\\ UNCHANGED << seq, lo >>"
            ),
            "{tla}"
        );
        assert!(
            tla.contains("Next == Grow \\/ PollGap \\/ PollDeliver"),
            "{tla}"
        );
        assert!(tla.contains("NoSilentLoss == lost = 0"), "{tla}");
    }

    #[test]
    fn action_enabled_reflects_guards() {
        let m = subscribe_model(); // Buggy = 0
        let behind: BTreeMap<&'static str, i64> =
            [("seq", 5), ("lo", 3), ("cursor", 0), ("lost", 0)]
                .into_iter()
                .collect();
        assert!(
            m.action_enabled("PollGap", &behind),
            "behind reader: PollGap enabled"
        );
        assert!(
            !m.action_enabled("PollDeliver", &behind),
            "behind reader: PollDeliver disabled"
        );
        let caught: BTreeMap<&'static str, i64> =
            [("seq", 5), ("lo", 3), ("cursor", 5), ("lost", 0)]
                .into_iter()
                .collect();
        assert!(
            !m.action_enabled("PollGap", &caught),
            "caught-up reader: PollGap disabled"
        );
        assert!(
            m.action_enabled("PollDeliver", &caught),
            "caught-up reader: PollDeliver enabled"
        );
    }

    #[test]
    fn subscribe_interpreter_enforces_no_silent_loss() {
        let m = subscribe_model(); // committed Buggy = 0
        let mut st = m.init_state();
        // Writer races ahead, evicting past the idle reader (cursor stays 0).
        for _ in 0..4 {
            assert!(m.fire("Grow", &mut st));
        }
        assert_eq!(st[&"seq"], 4);
        assert!(
            st[&"lo"] > st[&"cursor"] + 1,
            "reader has fallen behind the live window"
        );
        // A correct reader CANNOT silently deliver — the guard forbids it; it must
        // gap. `lost` stays 0.
        assert!(
            !m.fire("PollDeliver", &mut st),
            "a behind reader must not silently deliver (Buggy=0)"
        );
        assert!(
            m.fire("PollGap", &mut st),
            "a behind reader resyncs via gap"
        );
        assert_eq!(
            st[&"cursor"], st[&"seq"],
            "gap resyncs the cursor to the head"
        );
        assert!(m.check_invariant("NoSilentLoss", &st));
        assert_eq!(st[&"lost"], 0);
        // Caught up now: delivery is allowed and still loses nothing.
        assert!(m.fire("PollDeliver", &mut st));
        assert!(m.check_invariant("NoSilentLoss", &st));
    }

    #[test]
    fn transact_emits_conjunctive_guards() {
        let tla = transact_model().to_tla();
        assert!(
            tla.contains(
                "CommitClean == active = 1 /\\ seq = tbase /\\ seq =< MaxSeq - K /\\ \
                 seq' = seq + K /\\ active' = 0 /\\ UNCHANGED << tbase, lost >>"
            ),
            "{tla}"
        );
        assert!(
            tla.contains("Next == Write \\/ Begin \\/ CommitClean \\/ Abort \\/ BuggyCommit"),
            "{tla}"
        );
        assert!(tla.contains("NoLostUpdate == lost = 0"), "{tla}");
    }

    #[test]
    fn transact_interpreter_no_lost_update() {
        let m = transact_model(); // committed Buggy = 0
        let mut st = m.init_state();
        assert!(m.fire("Begin", &mut st)); // txn reads base = 0
        assert_eq!(st[&"tbase"], 0);
        assert!(m.fire("Write", &mut st)); // a concurrent write advances the head
        assert_eq!(st[&"seq"], 1);
        // Conflict (seq > tbase): a correct txn must NOT commit, and the buggy
        // commit is disabled at Buggy=0 — so no update can be lost.
        assert!(
            !m.fire("CommitClean", &mut st),
            "must not commit-clean under a conflict"
        );
        assert!(
            !m.fire("BuggyCommit", &mut st),
            "buggy commit is disabled at Buggy=0"
        );
        assert!(
            m.fire("Abort", &mut st),
            "the correct path aborts the conflicted txn"
        );
        assert_eq!(st[&"active"], 0);
        assert!(m.check_invariant("NoLostUpdate", &st));
        assert_eq!(st[&"lost"], 0);
        // A clean txn (no intervening write) commits K atomically.
        assert!(m.fire("Begin", &mut st)); // base = seq = 1
        assert!(m.fire("CommitClean", &mut st));
        assert_eq!(st[&"seq"], 1 + 2, "clean commit advances by K");
        assert!(m.check_invariant("NoLostUpdate", &st));
    }

    #[test]
    fn kernel_and_snapshot_generate_expected_tla() {
        let k = kernel_model().to_tla();
        assert!(
            k.contains("Emit == seq =< MaxSeq - 1 /\\ count' = count + 1 /\\ seq' = (IF Buggy = 1 THEN seq + 2 ELSE seq + 1)"),
            "{k}"
        );
        assert!(k.contains("Next == Emit"), "{k}");
        assert!(k.contains("SeqIsCount == seq = count"), "{k}");
        let s = snapshot_model().to_tla();
        assert!(
            s.contains("Write == seq =< MaxSeq - 1 /\\ seq' = seq + 1 /\\ leaked' = (IF Buggy = 1 /\\ snapped = 1 THEN 1 ELSE leaked) /\\ UNCHANGED << snapped >>"),
            "{s}"
        );
        assert!(s.contains("SnapshotIsolated == leaked = 0"), "{s}");
    }

    #[test]
    fn evict_full_emits_function_valued_tla() {
        let tla = evict_full_model().to_tla();
        assert!(tla.contains("VARIABLES seq, lo, live"), "{tla}");
        assert!(
            tla.contains("live = [n \\in 1..MaxSeq |-> FALSE]"),
            "function init: {tla}"
        );
        assert!(
            tla.contains("[n \\in 1..MaxSeq |->"),
            "function comprehension: {tla}"
        );
        assert!(
            tla.contains("[live EXCEPT ![seq + 1] = TRUE]"),
            "EXCEPT update: {tla}"
        );
        assert!(tla.contains("live[n]"), "function access: {tla}");
        assert!(tla.contains("n # lo"), "inequality: {tla}");
        assert!(
            tla.contains(
                "EvictOldestContiguous == \\A n \\in 1..MaxSeq : (live[n] <=> lo =< n /\\ n =< seq)"
            ),
            "quantified iff invariant: {tla}"
        );
    }

    #[test]
    fn kernel_interpreter_keeps_seq_eq_count() {
        let m = kernel_model();
        let mut st = m.init_state();
        for i in 1..=5 {
            assert!(m.fire("Emit", &mut st));
            assert_eq!(st[&"seq"], i, "gap-free: seq advances by exactly 1");
            assert_eq!(st[&"count"], i);
            assert!(m.check_invariant("SeqIsCount", &st));
        }
        assert!(!m.fire("Emit", &mut st), "guard bounds seq at MaxSeq");
    }

    #[test]
    fn snapshot_interpreter_isolates_from_later_writes() {
        let m = snapshot_model();
        let mut st = m.init_state();
        assert!(m.fire("Write", &mut st)); // pre-snapshot write
        assert!(m.fire("Snap", &mut st));
        assert_eq!(st[&"snapped"], 1);
        assert!(m.fire("Write", &mut st)); // post-snapshot write must not leak (Buggy=0)
        assert!(m.check_invariant("SnapshotIsolated", &st));
        assert_eq!(
            st[&"leaked"], 0,
            "a later write did not leak into the snapshot"
        );
        assert!(
            !m.fire("Snap", &mut st),
            "only one snapshot (guard snapped = 0)"
        );
    }

    #[test]
    fn interpreter_matches_ring_semantics_and_holds_invariant() {
        // The executable twin: drive the SAME model and check the ring discipline
        // (seq monotone +1; lo advances exactly at the cap) and the invariant.
        let m = ring_model();
        let mut st = m.init_state();
        assert_eq!(st[&"seq"], 0);
        assert_eq!(st[&"lo"], 1);
        let cap = 3;
        let mut fired = 0;
        while m.fire("Push", &mut st) {
            fired += 1;
            assert_eq!(st[&"seq"], fired, "seq must be monotone +1");
            // lo stays 1 until the window exceeds Cap, then tracks the head.
            let expected_lo = if fired - 1 + 1 > cap {
                fired - cap + 1
            } else {
                1
            };
            assert_eq!(
                st[&"lo"],
                expected_lo.max(1),
                "lo eviction discipline at seq={fired}"
            );
            assert!(
                m.check_invariant("LenBounded", &st),
                "LenBounded must hold at seq={fired}"
            );
        }
        // Guard `seq <= MaxSeq-1` (MaxSeq=6) stops Push after seq reaches 6.
        assert_eq!(st[&"seq"], 6, "guard must bound seq at MaxSeq");
    }
}
