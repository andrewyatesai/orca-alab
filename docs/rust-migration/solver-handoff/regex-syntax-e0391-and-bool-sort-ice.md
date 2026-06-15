# Two trustc bugs block orca-text / orca-config / orca-agents surveys

_2026-06-15, build #68 trustc (`b721d9efe`). Found by extending the survey beyond orca-core._

orca-core is the only standing-goal crate WITHOUT a `regex` dependency, which is why it's the
only one that surveys. orca-text, orca-config, and orca-agents all pull
`regex 1.12.3 → regex-automata → regex-syntax 0.8.10`, and trustc cannot get through it.

## Bug 1 — E0391 MIR-opt query cycle on regex-syntax (blocks all 3 crates)

Under `-Z trust-verify`, trustc's verification MIR-opt pass (`TrustVerify`, run inside
`optimized_mir`) does mono-item collection ("collecting items used by …") which re-enters
`optimized_mir` on callees. On **mutually-recursive** functions that forms a cycle stock rustc
never hits:

```
error[E0391]: cycle detected when optimizing MIR for `unicode::gencat`
  …requires optimizing MIR for `unicode::gencat::imp`… completing the cycle
error[E0391]: …`hir::literal::…::extract` → `extract_repetition` → `extract`…
error[E0391]: …`hir::…::alternation` → `lift_common_prefix` → `alternation`…
```

It is a hard compile error, so the crate never builds and the whole
`tcargo trust check -p <crate>` run dies; the survey's 600s alarm then fires (exit 142, 0
rows). The spinning trustc also orphans at 100% CPU and starves sibling surveys (had to
`kill -9` it).

**Root cause:** `TrustVerify`'s item collection queries `optimized_mir(callee)` while the
caller's `optimized_mir` is still on the query stack. Real fix: collect callees from a
non-`optimized_mir` source (e.g. `mir_built`/`instance_mir`) or guard against re-entering a
DefId already being optimized. Owner territory (compiler-query structure).

**Workaround shipped (build #69):** `TRUST_VERIFY_PRIMARY_ONLY` env gate in
`verification_enabled()` — verify only the cargo-primary package, so deps (regex-syntax)
compile with stock MIR-opt and never run `TrustVerify`. Opt-in; the survey sets it. Sound
(skipping a crate only drops obligations, never false-proves). It dodges Bug 1 but does NOT
fix the underlying cycle — a *primary* crate with cycle-triggering mutually-recursive fns
would still ICE.

## Bug 2 — ay ICE: `not()` on a non-Bool-sort Expr (additionally blocks orca-text)

Even with Bug 1 dodged, orca-text's OWN verification hits an ay panic:

```
thread 'rustc' panicked at first-party/ay/crates/ay-bindings/src/expr/bool.rs:28:24:
  NOT requires Bool sort
```

`bool.rs:28` is `self.try_not().expect("NOT requires Bool sort")`. Backtrace: `trust_router
verify_one_with_components_from → verify_with_backend_fallback`, folding `trust_types::formula::
Formula → ay_bindings::expr::Expr`. So the **Formula→Expr lowering** builds a logical `not`
over an operand whose ay sort is not Bool. The `.expect()` correctly rejects misuse; the bug is
upstream in the lowering (a sort mismatch — likely an integer/bitvector predicate treated as a
boolean, or a missing `is_bool` coercion). Owner territory (trust→ay lowering). orca-config /
orca-agents may or may not trip this on their own code — measurable once Bug 1 is dodged.

## Consequence for the standing goal

"orca-core/text/config/agents verify clean" currently can only be *measured* for orca-core.
The other three are gated on Bug 1 (compile) and, for orca-text, Bug 2 (verify). Build #69's
`TRUST_VERIFY_PRIMARY_ONLY` is the unblock for measuring config/agents; orca-text additionally
needs Bug 2 fixed.
