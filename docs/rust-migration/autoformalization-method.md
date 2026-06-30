# Autoformalization for the `orca-*` crates — adopting the Trust ts→tRust system

> **This is not a new design.** The autoformalizer already exists in the **Trust
> repo (`~/trust`)** and already runs on real orc code. This doc is the *orc-side
> adoption layer* — which `orca-*` modules plug in, with what corpus, behind which
> seam — and it defers to the canonical Trust designs for the engine itself:
>
> - `~/trust/tools/ts2rust/README.md` — the two-witness gate (W1 ∀-safety + W2 differential).
> - `~/trust/docs/2026-06-24-CLEAN-TRUST-AUDIT-AND-AUTOFORMALIZATION-PATH.md` — the grounded state + the prioritized §7 roadmap. **The ground truth.**
> - `~/trust/docs/2026-06-24-AUTOFORMALIZATION-FRAGMENT-STATUS.md` — the provable fragment + soundness basis per construct.
> - `~/trust/first-party/clean/designs/2026-06-19-autoform-{foundation-spec,build-plan}.md`, `2026-06-20-autoform-milestone-report.md` — foundation + Trust↔Clean fusion.
> - `~/trust/first-party/clean/designs/2026-06-16-aterm-parser-as-kernel-checked-theory.md` — where this meets Goal B (the aterm parser).

An earlier version of this file reinvented the engine (an "orca-IR", a "parity gate",
a "contract synthesis" stage) and **undersold the claim** ("differentially-verified,
not proven equivalent"). Both were wrong: `trust-ts-embed`/`trust-ir` already are the
IR; `io_refine` already proves **∀-refinement** for the in-fragment subset; and the
Certified spine lifts that to a **kernel-checked CIC proof**. This rewrite aligns to
reality.

## What the system actually is (one paragraph)

Autoformalization is a **translation + verification** problem: lower each TS construct
into the Rust/Trust construct that *already has a semantics*, then discharge a
refinement obligation **gated by the executable TS differential oracle** (TypeScript
is its own reference — it runs). The deliverable is **soundness, not completeness**: a
total, sound autoformalizer that **proves** the certifiable fragment (kernel-Certified
via the de Bruijn spine), is **SmtBacked/Trusted** for the rest, **refutes** the buggy
with a reproducible counterexample, and **declines** nothing silently. Rice/Gödel only
keep `Unknown` non-empty — they do not make it "impossible." (This corrects the earlier
"effects/async/closures are impossible" framing — those are hard to *denote as pure
SMT*, but Rust+Trust model them natively: mutation→ownership/borrows (`trust-vc`),
async→coroutine `CoroSuspend`, closures→`Box<dyn Fn>`+defunctionalization.)

## The honest ladder (use these words, not "100% Trust")

`Unchecked < Trusted (solver-in-TCB) < SmtBacked (∀-refinement via ay) < Certified
(kernel-checked CIC term, solver outside the TCB, mod 3 axioms)`. The label never
out-runs the proof (`require_assurance` is monotone). A witnessed counterexample can
never earn a certified label.

## Already true on orc code (don't re-derive — reuse)

- `ts2rust` is `TRUSTED` on verbatim orc functions incl. `computeEditorFontSize`
  (real `base+zoom` u32 overflow caught → u64 → proved), `parseCsiParams`,
  `clampNumber`, `unpackRgb`/`packRgb`, `getUtf8ByteLengthForCodePoint`.
- `trust-formalize io_refine` is `SmtBacked` on ~11–12 real orc reducers including the
  **VT500 parser** (`vtCharClass`/`vtParamDigit`/`vtNextState`); mutants refuted; zero
  false-PROVE; fail-closed out-of-fragment.
- The Certified spine reconstructs in-fragment obligations to **kernel-Certified**
  (trust-certify 86/86; `aterm_parser_certifies_through_the_real_kernel` green).

## The orc-side work (what is NOT in the `~/trust` docs)

The Trust repo owns the *engine*; orc owns *adoption*. Four concrete orc tasks:

1. **Fragment census of the 14 dormant `orca-*` crates.** Tag each exported function
   `in-fragment-now` (pure primitive/record/string/bounded-loop reducers — adopt today
   via `ts2rust` + `io_refine`) vs `needs-§7.5` (objects-as-ports, `Vec`/`HashMap`,
   effectful I/O behind capabilities). `orca-core` is already wired and in-fragment;
   the pure cores of `orca-config`/`orca-text` are the next adopters. The
   effect-heavy crates (`net`/`ssh`/`relay`/`store`/`session`/`runtime`) are **gated**
   on the engine roadmap below — do not promise them before it lands.

2. **Feed orc's real corpus to the witnesses.** orc already has the differential corpus
   the autoformalizer wants: the `orca-parity` vectors and the `.test.ts` suites *are*
   W2 inputs. Wire a small adapter so `orca-parity` cases become `ts2rust` fuzz/oracle
   inputs instead of a separate harness.

3. **Promotion workflow (orc-side).** A generated+proved crate ships behind
   `native/orca-node` + a flag, **dual-runs in shadow** against live TS in production,
   and is promoted to source-of-truth only after a clean parity window. This is the
   safe "verified backend IS the product," one crate at a time — never a big-bang cutover.

4. **Make the gauntlet call the real witnesses.** Add an `autoformalize` axis to
   `tools/terminal-bench/gauntlet.mjs` that shells to `~/trust/tools/ts2rust/
   autoformalize.mjs` over the per-crate in-fragment function list, so the orc agent
   gate runs W1+W2 and reports `TRUSTED / REFUTED{cex} / DECLINED` per function — not a
   reinvented check.

## What gates the effectful crates (the Trust §7 roadmap — track, don't duplicate)

Bulk adoption of the effect-heavy `orca-*` crates depends on the Trust repo landing,
in order: **§7.2** typed-equality lowering (W1 ∀-safety → ∀-functional refinement),
**§7.5.1** real TS-source parsing (swc/oxc, not yet vendored), **§7.5.3** MIR-extraction
of struct/closure *ports* (so the verified side is the real Rust, not a TS image), and
**§7.5.4** the unbuilt `trust-refine` two-program forward-simulation engine (an
LLM-proposed abstraction relation α gated by the two-witness shape). Until those land,
adopt only the in-fragment pure cores and keep the rest as TS + parity shadows.

## Where Goal A meets Goal B

The aterm/VT parser is the showcase: its transition function is already an
autoformalization target (`io_refine` SmtBacked today;
`aterm_parser_certifies_through_the_real_kernel` Certified). A kernel-Certified
equivalence between the parser's TS reference and its Rust is the single artifact that
serves **both** the "aterm is correct" (Goal B) and "the backend is autoformalized"
(Goal A) stories at once. Prioritize it.

## First orc milestone (small, real, gated by nothing)

Run `~/trust/tools/ts2rust/autoformalize.mjs` over the `orca-config` pure functions and
the `orca-core` reducers, driven by their `.test.ts` as the W2 corpus, and record the
`TRUSTED/REFUTED/DECLINED` table in `.gauntlet-report.json`. That reproduces the
existing capability *inside the orc repo's gate* and gives the fragment census its first
real rows — no engine work required.
