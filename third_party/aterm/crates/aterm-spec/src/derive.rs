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
//!   - [`Model::fire`] — the executable transition semantics (a real interpreter,
//!     using TLA+ primed semantics: every right-hand side is evaluated against the
//!     pre-state, then applied).
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
                panic!("function-valued Expr is TLA+-generation only (Tier-0 ty-checked, not interpreter-evaluable)")
            }
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
                format!("(IF {} THEN {} ELSE {})", c.to_tla(), a.to_tla(), b.to_tla())
            }
            Expr::Iff(a, b) => format!("({} <=> {})", a.to_tla(), b.to_tla()),
            Expr::Forall(idx, lo, hi, body) => {
                format!("\\A {idx} \\in {}..{} : {}", lo.to_tla(), hi.to_tla(), body.to_tla())
            }
            Expr::FnAccess(f, index) => format!("{f}[{}]", index.to_tla()),
            Expr::Except(f, index, value) => {
                format!("[{f} EXCEPT ![{}] = {}]", index.to_tla(), value.to_tla())
            }
            Expr::Comprehension(idx, lo, hi, body) => {
                format!("[{idx} \\in {}..{} |-> {}]", lo.to_tla(), hi.to_tla(), body.to_tla())
            }
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
        let mut init_parts: Vec<String> =
            self.vars.iter().map(|v| format!("{} = {}", v.name, v.init)).collect();
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
                conj.push(format!("{}' = {}", u.var, u.expr.to_tla()));
            }
            // UNCHANGED for variables this action does not update.
            let updated: Vec<&str> = a.updates.iter().map(|u| u.var).collect();
            let unchanged: Vec<&str> =
                vars.iter().copied().filter(|v| !updated.contains(v)).collect();
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
            let val = overrides.iter().find(|(o, _)| o == n).map(|(_, v)| *v).unwrap_or(*default);
            s.push_str(&format!("CONSTANT {n} = {val}\n"));
        }
        s.push_str("SPECIFICATION Spec\n");
        for inv in &self.invariants {
            s.push_str(&format!("INVARIANT {}\n", inv.name));
        }
        s.push_str("CHECK_DEADLOCK FALSE\n");
        s
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
            let val = overrides.iter().find(|(o, _)| o == n).map(|(_, v)| *v).unwrap_or(*default);
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

    /// Fire a named action against `state` (TLA+ primed semantics: all RHS are
    /// evaluated against the pre-state, then applied atomically). Returns `false`
    /// without mutating if the action's guard is unsatisfied. This is the
    /// executable twin of the generated TLA+ action.
    pub fn fire(&self, action: &str, state: &mut BTreeMap<&'static str, i64>) -> bool {
        let act = self
            .actions
            .iter()
            .find(|a| a.name == action)
            .unwrap_or_else(|| panic!("no action `{action}` in model `{}`", self.name));
        let mut env = self.const_env();
        env.extend(state.iter().map(|(k, v)| (*k, *v)));
        if act.guard.as_ref().is_some_and(|g| !g.eval(&env).as_bool()) {
            return false;
        }
        let news: Vec<(&'static str, i64)> = act
            .updates
            .iter()
            .map(|u| (u.var, u.expr.eval(&env).as_int()))
            .collect();
        for (v, val) in news {
            state.insert(v, val);
        }
        true
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
        act.guard.as_ref().map(|g| g.eval(&env).as_bool()).unwrap_or(true)
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
            StateVar { name: "seq", init: 0 },
            StateVar { name: "lo", init: 1 },
        ],
        fn_vars: vec![],
        actions: vec![Action {
            name: "Push",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))), // seq <= MaxSeq - 1
            updates: vec![
                Update { var: "seq", expr: add(var("seq"), int(1)) },
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
            StateVar { name: "seq", init: 0 },
            StateVar { name: "cursor", init: 0 },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Grow", // writer appends; cursor is UNCHANGED
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![Update { var: "seq", expr: add(var("seq"), int(1)) }],
            },
            Action {
                name: "Deliver", // reader catches up; seq is UNCHANGED
                guard: Some(gt(var("seq"), var("cursor"))),
                updates: vec![Update { var: "cursor", expr: var("seq") }],
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
            StateVar { name: "seq", init: 0 },
            StateVar { name: "lo", init: 1 },
            StateVar { name: "cursor", init: 0 },
            StateVar { name: "lost", init: 0 },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Grow", // writer appends + evicts oldest when over Cap
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![
                    Update { var: "seq", expr: add(var("seq"), int(1)) },
                    Update {
                        var: "lo",
                        expr: if_(
                            gt(add(sub(add(var("seq"), int(1)), var("lo")), int(1)), cst("Cap")),
                            add(var("lo"), int(1)),
                            var("lo"),
                        ),
                    },
                ], // cursor, lost UNCHANGED
            },
            Action {
                name: "PollGap", // reader fell behind (lo > cursor + 1): resync, no loss
                guard: Some(gt(var("lo"), add(var("cursor"), int(1)))),
                updates: vec![Update { var: "cursor", expr: var("seq") }], // seq, lo, lost UNCHANGED
            },
            Action {
                name: "PollDeliver", // deliver; correct iff the reader is still in window
                // Buggy = 1 \/ lo =< cursor + 1  (Buggy removes the in-window guard)
                guard: Some(or_(eq(cst("Buggy"), int(1)), le(var("lo"), add(var("cursor"), int(1))))),
                updates: vec![
                    Update { var: "cursor", expr: var("seq") },
                    // lost' = IF lo > cursor + 1 THEN 1 ELSE lost  (records a silent skip)
                    Update {
                        var: "lost",
                        expr: if_(gt(var("lo"), add(var("cursor"), int(1))), int(1), var("lost")),
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

/// A fourth derived model — TRANSACTION ATOMICITY / no-lost-update under
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
            StateVar { name: "seq", init: 0 },
            StateVar { name: "tbase", init: 0 },
            StateVar { name: "active", init: 0 },
            StateVar { name: "lost", init: 0 },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Write", // a concurrent writer advances the committed head
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![Update { var: "seq", expr: add(var("seq"), int(1)) }],
            },
            Action {
                name: "Begin", // a txn reads the current head as its base version
                guard: Some(eq(var("active"), int(0))),
                updates: vec![
                    Update { var: "active", expr: int(1) },
                    Update { var: "tbase", expr: var("seq") },
                ],
            },
            Action {
                name: "CommitClean", // no write intervened: commit K edits atomically
                guard: Some(and_(
                    and_(eq(var("active"), int(1)), eq(var("seq"), var("tbase"))),
                    le(var("seq"), sub(cst("MaxSeq"), cst("K"))),
                )),
                updates: vec![
                    Update { var: "seq", expr: add(var("seq"), cst("K")) },
                    Update { var: "active", expr: int(0) },
                ],
            },
            Action {
                name: "Abort", // a write intervened (seq > tbase): correct path aborts
                guard: Some(and_(eq(var("active"), int(1)), gt(var("seq"), var("tbase")))),
                updates: vec![Update { var: "active", expr: int(0) }],
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
                    Update { var: "seq", expr: add(var("tbase"), cst("K")) },
                    Update { var: "active", expr: int(0) },
                    Update { var: "lost", expr: int(1) },
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
            StateVar { name: "seq", init: 0 },
            StateVar { name: "count", init: 0 },
        ],
        // Action `Emit` (not `Append`, which clashes with ty's Sequences builtin in
        // a single-action spec — see the ring's `Push`).
        fn_vars: vec![],
        actions: vec![Action {
            name: "Emit",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
            updates: vec![
                Update { var: "count", expr: add(var("count"), int(1)) },
                // seq' = IF Buggy = 1 THEN seq + 2 ELSE seq + 1   (Buggy opens a gap)
                Update {
                    var: "seq",
                    expr: if_(eq(cst("Buggy"), int(1)), add(var("seq"), int(2)), add(var("seq"), int(1))),
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
            StateVar { name: "seq", init: 0 },
            StateVar { name: "snapped", init: 0 },
            StateVar { name: "leaked", init: 0 },
        ],
        fn_vars: vec![],
        actions: vec![
            Action {
                name: "Snap", // take a single snapshot of the current head
                guard: Some(eq(var("snapped"), int(0))),
                updates: vec![Update { var: "snapped", expr: int(1) }], // seq, leaked UNCHANGED
            },
            Action {
                name: "Write", // advance the head; must not leak into an active snapshot
                guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
                updates: vec![
                    Update { var: "seq", expr: add(var("seq"), int(1)) },
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
    let evicting = || gt(add(sub(add(var("seq"), int(1)), var("lo")), int(1)), cst("Cap"));
    Model {
        name: "EvictFull",
        consts: vec![("MaxSeq", 5), ("Cap", 3)],
        vars: vec![StateVar { name: "seq", init: 0 }, StateVar { name: "lo", init: 1 }],
        fn_vars: vec![FnVar { name: "live", range: "MaxSeq" }],
        actions: vec![Action {
            name: "Push",
            guard: Some(le(var("seq"), sub(cst("MaxSeq"), int(1)))),
            updates: vec![
                Update { var: "seq", expr: add(var("seq"), int(1)) },
                Update { var: "lo", expr: if_(evicting(), add(var("lo"), int(1)), var("lo")) },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tla_check::TlaSpec;

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
        assert!(spec.actions.contains("LenBounded"), "defs: {:?}", spec.actions);
    }

    #[test]
    fn transition_spec_parameterizes_init_but_shares_the_action() {
        let m = ring_model();
        let tla = m.transition_spec();
        assert!(tla.contains("CONSTANT MaxSeq, Cap, seq_init, lo_init"), "{tla}");
        assert!(tla.contains("Init == seq = seq_init /\\ lo = lo_init"), "{tla}");
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
        assert!(tla.contains("Grow == seq =< MaxSeq - 1 /\\ seq' = seq + 1 /\\ UNCHANGED << cursor >>"), "{tla}");
        assert!(tla.contains("Deliver == seq > cursor /\\ cursor' = seq /\\ UNCHANGED << seq >>"), "{tla}");
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
            assert_eq!(st[&"cursor"], before_cursor, "Grow must leave cursor UNCHANGED");
            assert!(m.check_invariant("CursorBounded", &st));
            let before_seq = st[&"seq"];
            assert!(m.fire("Deliver", &mut st));
            assert_eq!(st[&"seq"], before_seq, "Deliver must leave seq UNCHANGED");
            assert_eq!(st[&"cursor"], st[&"seq"], "Deliver catches the reader up to the writer");
            assert!(m.check_invariant("CursorBounded", &st));
        }
        // Deliver is guarded by seq > cursor; once caught up it cannot fire.
        assert!(!m.fire("Deliver", &mut st), "Deliver guard (seq > cursor) blocks when caught up");
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
        assert!(tla.contains("Next == Grow \\/ PollGap \\/ PollDeliver"), "{tla}");
        assert!(tla.contains("NoSilentLoss == lost = 0"), "{tla}");
    }

    #[test]
    fn action_enabled_reflects_guards() {
        let m = subscribe_model(); // Buggy = 0
        let behind: BTreeMap<&'static str, i64> =
            [("seq", 5), ("lo", 3), ("cursor", 0), ("lost", 0)].into_iter().collect();
        assert!(m.action_enabled("PollGap", &behind), "behind reader: PollGap enabled");
        assert!(!m.action_enabled("PollDeliver", &behind), "behind reader: PollDeliver disabled");
        let caught: BTreeMap<&'static str, i64> =
            [("seq", 5), ("lo", 3), ("cursor", 5), ("lost", 0)].into_iter().collect();
        assert!(!m.action_enabled("PollGap", &caught), "caught-up reader: PollGap disabled");
        assert!(m.action_enabled("PollDeliver", &caught), "caught-up reader: PollDeliver enabled");
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
        assert!(st[&"lo"] > st[&"cursor"] + 1, "reader has fallen behind the live window");
        // A correct reader CANNOT silently deliver — the guard forbids it; it must
        // gap. `lost` stays 0.
        assert!(!m.fire("PollDeliver", &mut st), "a behind reader must not silently deliver (Buggy=0)");
        assert!(m.fire("PollGap", &mut st), "a behind reader resyncs via gap");
        assert_eq!(st[&"cursor"], st[&"seq"], "gap resyncs the cursor to the head");
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
        assert!(!m.fire("CommitClean", &mut st), "must not commit-clean under a conflict");
        assert!(!m.fire("BuggyCommit", &mut st), "buggy commit is disabled at Buggy=0");
        assert!(m.fire("Abort", &mut st), "the correct path aborts the conflicted txn");
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
        assert!(tla.contains("live = [n \\in 1..MaxSeq |-> FALSE]"), "function init: {tla}");
        assert!(tla.contains("[n \\in 1..MaxSeq |->"), "function comprehension: {tla}");
        assert!(tla.contains("[live EXCEPT ![seq + 1] = TRUE]"), "EXCEPT update: {tla}");
        assert!(tla.contains("live[n]"), "function access: {tla}");
        assert!(tla.contains("n # lo"), "inequality: {tla}");
        assert!(
            tla.contains("EvictOldestContiguous == \\A n \\in 1..MaxSeq : (live[n] <=> lo =< n /\\ n =< seq)"),
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
        assert_eq!(st[&"leaked"], 0, "a later write did not leak into the snapshot");
        assert!(!m.fire("Snap", &mut st), "only one snapshot (guard snapped = 0)");
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
            let expected_lo = if fired - 1 + 1 > cap { fired - cap + 1 } else { 1 };
            assert_eq!(st[&"lo"], expected_lo.max(1), "lo eviction discipline at seq={fired}");
            assert!(m.check_invariant("LenBounded", &st), "LenBounded must hold at seq={fired}");
        }
        // Guard `seq <= MaxSeq-1` (MaxSeq=6) stops Push after seq reaches 6.
        assert_eq!(st[&"seq"], 6, "guard must bound seq at MaxSeq");
    }
}
