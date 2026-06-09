# Trust verification — Orca as a proving ground for Trust

Orca's business logic is being rebuilt as modular Rust crates (`forbid(unsafe)`,
panic-free). [Trust](https://github.com/andrewyatesai/trust) is the
verification-oriented Rust compiler fork that can *prove* properties of that
logic: panic-safety, integer overflow, out-of-bounds, ownership invariants, and
contract pre/postconditions.

**Co-evolution, both directions:**
- **Trust verifies Orca** → ship a fleet-of-agents core with machine-checked guarantees.
- **Orca improves Trust** → real, demanding logic exercises the verifier. Every
  "unsupported MIR / can't prove this true obligation" is a concrete Trust ticket.
  Orca is the test that tells us whether Trust's design works and where it doesn't.

## Current state (be honest)

- Trust is **proof-aware, not proof-complete**. No stage2 `trustc` is built in the
  Orca dev sandbox (it's offline, lacks cmake/ninja, and the stage0 bootstrap
  payloads aren't present), so **verification can't run here yet** — it must run on
  a machine with the toolchain built.
- Orca crates stay **Trust-ready** meanwhile: `forbid(unsafe)`, panic-free, and
  (incrementally) annotated with contracts that are inert under stock cargo.

## Build + verify (on a capable machine)

```bash
# 1. Build the Trust stage2 toolchain (from ~/trust; needs cmake+ninja+python3, network for stage0).
cd ~/trust
python3 scripts/recreate_bootstrap.py --stage 2   # if bootstrap/trust-stage0/dist holds only manifests
./x.py build --stage 2
bash tests/e2e_trust_toolchain.sh                  # inventory/e2e gate

# 2. Verify Orca's pure crates (from rust/).
cd /path/to/orca/rust
~/trust/build/host/stage2/bin/tcargo trust check -p orca-core   --format json
~/trust/build/host/stage2/bin/tcargo trust check -p orca-agents --format json
~/trust/build/host/stage2/bin/tcargo trust check -p orca-config --format json
# ... per pure crate. --hardened / --trust-profile <p> raise the bar.
```

The JSON proof rows (per function) are the artifact. Empty/"unsupported" rows are
not failures — they are the **gap log** (below).

## Contract convention (dual-build)

Contracts must not break the stock-cargo build (the workspace must build with
plain `cargo` too). Use `cfg_attr` gated on a `trust_verify` cfg so the Trust
attribute is applied only under the verifier and is otherwise absent:

```rust
// Inert under stock rustc (cfg off); becomes #[trust::ensures(..)] under `--cfg trust_verify`.
#[cfg_attr(trust_verify, trust::ensures(|s: &String|
    s.encode_utf16().count() <= max_length))]
fn truncate_preserving_surrogates(value: &str, max_length: usize) -> String { /* ... */ }
```

Each annotated crate declares the cfg so stock builds don't warn:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(trust_verify)'] }
```

`tcargo trust check` is invoked with `--cfg trust_verify` (or the equivalent
profile) so the contracts activate. Start with invariants already reasoned by
hand — e.g. `agent_status_types::truncate_preserving_surrogates` (no lone
surrogate, length ≤ cap), `feature_interactions` record validation, the
`orca-relay` binary framing bounds.

## Anticipated gap log — where Orca will stress Trust

Pre-identified from what the ported crates actually do; confirm/expand once the
verifier runs. Each row is a candidate **Trust improvement** driven by Orca.

| Orca pattern (crate) | Trust capability exercised | Likely gap / ticket |
| --- | --- | --- |
| UTF-16 surrogate slicing — `encode_utf16`/`from_utf16_lossy` (`orca-agents`, `orca-text`) | bounds + postcondition on `Vec<u16>` slicing | prove "no lone surrogate" postcondition; reason about surrogate-range guards |
| `Regex::new(PATTERN).unwrap()` on static patterns (`orca-text`, `orca-agents`) | panic-freedom through `unwrap` | can't prove a static regex is valid → needs a const-validity lemma or a `requires` on the pattern; flags real panic sites |
| `serde_json::Value` recursion (`orca-config`, `orca-relay`) | recursive-enum / heap reasoning, termination | likely "unsupported MIR" on deep recursion; bucket external-dep policy |
| the `regex` engine internals (vendored dep) | whole-crate verification of a large external crate | external-dependency policy bucket; expect skipped/advisory |
| `HashMap`/`BTreeMap` ops (`orca-config`, `orca-core`) | allocator + hashing model | std-collection modeling depth |
| saturating/checked arithmetic (`aterm`-style parsers, `orca-core`) | integer-overflow proofs | should be an early **win** — confirms overflow lane works |
| closure-heavy iterator chains `filter/map/fold` (everywhere) | closure + monomorphization handling | coverage of higher-order MIR |

## Loop

1. Port/annotate an Orca crate (Trust-ready, contracts inert under stock cargo).
2. Run `tcargo trust check` on a capable machine.
3. Triage the JSON: proved obligations = guarantees; unsupported/unproved = **Trust tickets**.
4. Improve Trust (and/or `first-party/ty`, currently an empty slot — candidate home
   for Orca's reusable verified domain specs); re-verify.

This file is the durable record of that loop.

## Real findings — Trust built + run on Orca (2026-06-07)

Trust stage2 was built locally and run on Orca's crates. The build and first
verification runs immediately surfaced real Trust issues — the co-evolution loop
working as intended. Build recipe (sandbox disabled): `brew install cmake ninja`;
`recreate_bootstrap.py --stage 2` (genesis stage0 from local rustc 1.96.0);
clone `first-party/*` submodules over the `gh` token
(`git config --global url."https://github.com/".insteadOf "git@github.com:"` +
`git submodule update --init --recursive`); `./x.py build --stage 2`
(`download-ci-llvm=false` → LLVM from source, ~28 min).

**Bug 1 (fixed).** The local genesis stage0 wraps stock rustc, but bootstrap
passed it the Trust-only flag `-Zno-trust-verify` → "unknown unstable option",
so Trust couldn't build with a stock stage0 at all. Fixed
`scripts/create_local_genesis_stage0.py` — the generated `bin/trustc` wrapper now
strips `-Z*trust*` flags before exec'ing stock rustc.

**Bug 2 (fixed; rebuilding to confirm).** `tcargo trust check` returned
`0 proved / transport:missing-json` for **everything** — Trust's own
`examples/midpoint.rs` *and* Orca crates. Root cause (found via `TRUST_DYN_PROBE=1`):
the verify pass skipped every function with `Skip(ExternalDependencyScope)`.
`should_skip_external_dep_body` (`compiler/.../trust_verify.rs`) skipped **local**
MIR (the crate being compiled) unless `TRUST_VERIFY_POLICY=verify-example-corpus`
was set — i.e. first-party verification was hidden by default. Fixed so a local
body is never treated as an external dependency. No-rebuild workaround:
`TRUST_VERIFY_POLICY=verify-example-corpus` (confirmed: `decision=Verify`, emits
`TRUST_JSON`).

**Gap 3 (in progress — first slice landed & validated 2026-06-08).** The verifier
couldn't lower calls to core/std into TrustIr. **`wrapping_add` is now fixed
end-to-end** (commits pending across the trust-ir + trust-mc submodules + the
bridge): the bridge lowers `core::num::<impl uN>::wrapping_add` to a modular
TrustIr `BinOp` tagged with the new `ProofAnnotation::Wrapping`, and the trust-mc
CHC translator skips the no-overflow obligation for `Wrapping` ops. Validated on
the rebuilt stage2: `fn rank(x:u32)->u32 { x.wrapping_add(1) }` verifies with
**0 obligations (exit 0)** in CHC mode (`-Z trust-verify -Z trust-verify-level=2`);
the control `fn plain(x:u32)->u32 { x + 1 }` still carries its 1 overflow
obligation. (Edit sites: `proof.rs`/`binary.rs`/`diff.rs`/`parser.rs` for the
annotation; `trust-ir-bridge/src/lower.rs` `core_int_arith_intrinsic` + the
`Terminator::Call` arm; `trust-mc-trust-bmc/src/translate_chc.rs` the two
no-overflow guard sites.)

Remaining Gap-3 work: (a) **`wrapping_sub`/`wrapping_mul` now lowered too**
(2026-06-08): `core_int_arith_intrinsic` recognizes all three of
`wrapping_{add,sub,mul}` → modular `BinOp::{Add,Sub,Mul}` tagged `Wrapping`
(`checked_*`/`overflowing_*`/`saturating_*` still deferred — they need an
`Inst::Overflow` consumer); (b) the slice/`Vec`/`String` index+len and
`Option`/`Result`/derived `Clone` families that Orca's crates actually call;
(c) the **`-Z trust-verify-full` native-evidence path** is a *separate, stricter*
admission than the CHC backend — it returns `unknown` wanting
`ContractPredicate::MathIr/CanonicalJson` even for the now-lowered wrapping ops,
so it needs its own bridge from the lowered TrustIr to that evidence form.

**Unsize cast (282 — the biggest single lowering gap) — precise sound plan.**
The native route refuses *all* Unsize coercions at
`crates/trust-mir-extract/src/convert.rs:667` (`unsupported_rvalue`, exact reason
string matching the survey). Sibling coercions (ReifyFnPointer, MutToConstPointer,
…) are modeled as a plain `Rvalue::Cast(operand, target_ty)` because they're
value-preserving; Unsize is *not* (it adds metadata: a slice length or a vtable),
so it can't reuse that as-is. The fail-closed VC gate
(`trust-vcgen/src/generate.rs:1057 collect_cast_relation_unsupported`) would also
reject array-ref→slice-ref even if it were emitted as a Cast. Sound 3-site fix:
1. `convert.rs:667` — for `PointerCoercion::Unsize` with source `&[T;N]`/`*[T;N]`
   and a matching slice-ref/ptr target, emit a modeled rvalue carrying the known
   array length `N`; leave other Unsize forms (trait objects) refused (sound) or
   model them as an opaque well-typed value.
2. `generate.rs:1057` — allow the array-ref→slice-ref shape.
3. `generate.rs:2031 v2_build_cast_vc` — define the result slice's length = `N`
   so downstream bounds/`len` obligations can discharge against it.
Soundness: representing the result as a fresh slice of the target type with
length pinned to `N` is faithful for the array→slice case; any obligation that
doesn't depend on the (unmodeled) data-pointer stays at worst `unknown`, never
falsely `proved`. This is the validated-per-slice next increment after the
wrapping family.

### orca-core verification triage (2026-06-08, no-rebuild survey)

`trustc -Z trust-verify -Z trust-verify-level=2 -Z trust-verify-output=json` over
`orca-core/src/lib.rs` (zero-dep, compiles standalone): **697 functions, 2362
obligations, 2296 unknown.** The unknowns, by reason (the prioritized backlog to
reach "zero unknown"):

| Count | Reason | Category |
| --- | --- | --- |
| 334 | "solver proof lacks artifact-backed full-verifier evidence" | **mode/admission** — already SOLVER-PROVED, downgraded because non-full mode isn't artifact-backed (`trust_verify.rs:7158-7181`) |
| 282 | `CastKind::PointerCoercion::Unsize` not lowered | cast lowering |
| 106 | `Clone::clone` call | std/core call lowering |
| 103 | `ToString::to_string` | std/core call lowering |
| 81 | `PartialEq::eq` | std/core call lowering |
| 80+33 | `fmt::rt::Argument::new_display` / `Arguments::new` | fmt machinery |
| 60+23 | `Vec::push` / `Vec::len` | collection modeling |
| 51+22 | `Deref::deref` | std/core call lowering |
| 47 | `Default::default` | std/core call lowering |
| 29+27 | `Iterator::next` / `IntoIterator::into_iter` | iterator modeling |

**Survey mode landed (objective #3, 2026-06-08).** Added `TRUST_VERIFY_SURVEY=1`:
forces the artifact-backed full route but decouples `fail_closed()` from
`is_full_verification()` (new `survey` field on `TrustVerifyPolicy`), so a whole
crate is surveyed without aborting on the first unproved obligation. Validated:
`trustc` exits 0 across all 697 orca-core functions instead of aborting.

**DECISIVE finding from the full-mode survey (the real core blocker).**
**[SUPERSEDED 2026-06-08 by the BREAKTHROUGH below — the "0/3241, multi-month
wall" was an identity-string bug, now fixed; the native route proves QF_LIA
soundly.]** With
survey mode, the artifact-backed **native full-verifier route proves 0 / 3241**
orca-core obligations — every one is `native full verifier evidence status:
Unsupported`. Contrast: the **CHC solver** (non-full mode) proves ~334 but those
are downgraded (not artifact-backed). So the two backends are disjoint in the
worst way: the backend that *can* prove real obligations (CHC) is not admitted as
evidence, and the backend that *is* admitted (native TrustIr full verifier) is
Unsupported for essentially all real Rust. **This — not call-family lowering — is
the central blocker.** The realistic paths are both core Trust-verifier research:
(A) implement native-route verification for real obligation/MIR shapes, or
(B) make the CHC/PDR solver emit a checkable proof certificate that counts as
artifact-backed evidence (likely more tractable: the solver already proves 334;
it needs a certificate + checker so `artifact_backed_proofs` can admit it). This
is multi-month core-compiler work, not call-family slices.

**UPDATE (2026-06-08) — part of the native-route "Unsupported" is a fixable
identity bug, not fundamental.** Traced a single QF_LIA obligation
(`x < 100 => x+1` no-overflow) through the full-mode native route. The compiler
*does* run the real trust-mc engine in-process: `collect_full_verification_artifacts`
→ `FullVerificationEngine::with_required_native_stubs()` →
`NativeTrustMcTrustIrEngine` (a thin wrapper over the **real** `trust_bmc::
TrustMcVerifierApiAdapter`, not a no-op stub; the genuine stubs are trust-wp/
trust-vc/TY). The adapter rejected the obligation at its **first gate** — the
obligation-identity match (`verifier_api.rs:1163`,
`trust_mc_obligation_identity_matches`) — with: input names
`trust_ir-native-trust`**`-`**`mc-request-2-proof-2` (hyphen) but adapter expects
`trust_ir-native-trust`**`_`**`mc-request-2-proof-2` (underscore). Root cause: the
**compiler** builds the id as the crate name `trust-mc` (hyphen,
`trust_verify.rs:5016`) while the **adapter** hardcodes the identifier form
`trust_mc` (underscore, `verifier_api.rs:3640`) — two conventions colliding at
the boundary, so genuinely-matching native CHC/PDR evidence is rejected before
the solver ever runs. **Fix:** canonicalize the separator in
`trust_mc_obligation_identity_matches` (`candidate.replace('-',"_") ==
native_id.replace('-',"_")`); sound because request/proof ids are numeric so no
distinct obligations collide, and the identity check is a *precondition* gate —
it does not bypass any proof-evidence validation. Rebuilding stage2 to measure
how many of the 3241 "Unsupported" this clears vs. how many hit the *next* gate
(native solver actually proving + emitting transcript/replay/checked-report
artifacts). Either way it converts a "fundamental wall" assumption into a
concrete, ticketed bug — exactly the co-evolution loop.

**🟢 BREAKTHROUGH (2026-06-08) — the native full-verifier route works; the "wall"
was the identity bug.** After fixing the identity mismatch at **all three**
native-id comparison gates (the suite token is the crate name `trust-mc`/hyphen
on the compiler side but the identifier form `trust_mc`/underscore on the
trust-mc side):
- `crates/trust-bmc/src/verifier_api.rs` — `trust_mc_obligation_identity_matches`
  (typed-input gate), the binding-metadata gate (~1898), the proof-transport gate
  (~3462);
- `first-party/trust-mc/trust-mc-core/src/evidence.rs` — the proof-grade metadata
  gate (~419);
each fixed with `a.replace('-',"_") == b.replace('-',"_")` (sound: request/proof
ids are numeric, so canonicalizing the separator can't merge distinct obligations;
distinct suites trust-mc/trust-wp stay distinct; these are precondition gates that
don't bypass proof validation). Committed: trust-bmc `ade0610b51`, submodule
trust-mc-core `eaca4b299`.

**Validated on rebuilt stage2.** A QF_LIA probe now proves end-to-end through the
native route with the **full proof-grade artifact chain**: `status=Proved`,
`strength=PdrInvariant`, `assurance=Sound`, artifacts `SolverTranscript` +
`ProofCertificate` (pdr-invariant-model) + `ProofCheckReport` (checked-proof-report)
+ `ReplayLog`, replay/check `Replayed/Accepted`, artifact policy `satisfied=true`.
**Soundness control passes:** `fn bounded(x){if x<100 {x+1} else {0}}` → proved=2;
`fn unbounded(a,b){a+b}` → the overflow obligation **fails** (refuted), exit 1.
The verifier discriminates provable from unprovable — it is not rubber-stamping.

**orca-core full-mode survey, after the fix (was 0/3241):**
**697 functions, 3241 obligations → proved=167, unknown=3074, failed=0;
142 functions fully proved** (machine-proved panic/overflow safety, sound). The
artifact-backed admission path (a.k.a. "path B") is therefore **DONE** — it was
the identity bug, not multi-month core research. This also dissolves the earlier
"lowering only reclassifies unknowns" worry: with admission working, **every
lowered obligation now converts straight to `proved`.**

**Remaining orca-core unknowns by cause (the now-tractable backlog to zero):**
| Count | Cause | Category |
| --- | --- | --- |
| ~2805 | bridge "failed to lower `<…>::clone`/`::default` and local callees: unsupported operation: Call target `std…`" / "address-of Field projection .0" | **Gap 3 — TrustIr bridge lowering** (core/std call targets + MIR ops). Dominant blocker; each lowering now converts directly to proved. |
| 167 | (proved) | ✅ native route, QF_LIA arithmetic safety |
| 130 | `Deref::deref` | bridge call lowering |
| 50 | trust-wp formula payload schema rejected (`trust-wp.trust-formula.v1` ≠ `TrustWpPureExprV1`/`trust_wp.trust-formula.v1`/`trust-types.Formula@1`) | trust-wp engine schema mismatch — **likely another quick name-normalization bug** |
| 26 | `CastKind::PointerCoercion::Unsize` | bridge cast lowering (3-site plan above) |
| 26 | native solver does not prove | needs better encoding / genuinely hard |
| 21/8/6 | `IntoIterator`/`ToString`/`fmt` | bridge call lowering |

**Net:** the done-criterion ("zero unknown for the pure crates") is no longer
gated on multi-month core research — it is gated on **incremental TrustIr-bridge
lowering** (Gap 3), where each family/op now pays off directly through the proven
native route. The single biggest lever is lowering the core/std **call targets**
+ the **address-of-field-projection** MIR op (together ~2805).

**Second systemic identity-bug fixed (2026-06-08): trust-wp formula schema.** Same
class as the trust-mc id bug. The compiler stamped trust-wp native formula
payloads with `trust-wp.trust-formula.v1` (hyphen) but the acceptor uses the
canonical trust-types tag `trust_wp.trust-formula.v1` (underscore) → every payload
rejected at the schema gate. Fixed by referencing
`trust_types::trust_formula_v1::TRUST_WP_TRUST_FORMULA_SCHEMA_VERSION` (commit
`b32deec3f9`). Validated: payloads are now *consumed* (was schema-rejected). The
remaining 50 trust-wp unknowns are now the deductive engine's *coverage* (derived
`PartialOrd`/`cmp`), a separate engine concern — not a gate bug.

### Next phase — the TrustIr-bridge lowering library (precise, prioritized)

Top unlowered call targets in orca-core (count = obligations blocked; note a whole
function fails to lower on its **first** unsupported call, so unblocking a
*function* needs its **entire** call set covered — partial coverage moves few
function counts):

| n | callee | sound summary | soundness note |
| --- | --- | --- | --- |
| 289 | `Vec::new` | fresh empty vec, len 0, **no panic** | total |
| 135 | `Box::new_uninit` | fresh box, **no panic** | total |
| 134 | `Default::default` | fresh `T`, no panic for derived/primitive | careful: a hand `Default` impl can panic — allowlist derived/std only |
| 97 | `str::to_lowercase` | fresh `String` | allocates → OOM; safe only if Trust's panic model excludes OOM (verify) |
| 72/38 | `Option::map`/`map_or` | maps option | **NOT unconditionally no-panic** — panics iff the closure does; needs closure-aware modeling |
| 65 | `Try::branch` (`?`) | control-flow desugar | model as branch, not a call summary |
| ~500 | `core::str::{len,is_empty,as_bytes,chars,find,split,trim,split_once,strip_suffix}` | fresh result + length facts (`as_bytes().len()==self.len()`), **no panic** | total — the highest-value clean batch |
| 31 | `HashMap::new` | fresh empty map, **no panic** | total |
| 28/26 | `ToString::to_string`/`PartialEq::eq` | fresh `String` / `bool`, no panic for derived | derived/primitive allowlist |

**Two complementary mechanisms** (both feed the now-working native route):
- **(B) Modeled call summaries** — extend the bridge's `Terminator::Call` arm (the
  same site as `core_int_arith_intrinsic`) to recognize an allowlist of total
  no-panic core/std functions and emit a **havoc result of the correct type** (+
  no-panic for the call) instead of refusing. Sound by over-approximation: the
  result is unconstrained, so value-dependent obligations stay `unknown` (never
  falsely proved); the no-panic fact is sound *only* for the curated total set.
  Start with the clean total batch (`str::{len,is_empty,as_bytes,…}`, `Vec::new`,
  `HashMap::new`) — no closures, no allocation-panic ambiguity.
- **(A) `address-of Field projection` MIR op** (229 + part of the 851
  compiler-`Unsupported`) — `lower.rs:902` refuses `&place.field` because TrustIr
  GEP is array-stride, not struct-field-offset. Needs a struct-field-address path
  (field-index GEP with the trust-mc consumer resolving the layout offset, mirror
  of the working `Inst::ExtractField` value path). Enables lowering local derived
  `Clone`/`Default`/`PartialEq` bodies (vs. summarizing them).

Either mechanism is a focused multi-increment effort with a ~20-min rebuild per
batch; both are now ordinary engineering, not core-verifier research.

**Increment landed (2026-06-08): total-no-panic call summaries (mechanism B, first
slice).** `total_no_panic_call_summary` + the `Terminator::Call` branch model a
curated allowlist of cannot-panic core/std functions (`Vec`/`HashMap`/`HashSet::new`,
`str::{len,is_empty,as_bytes,trim,trim_start,trim_end}`) as a fresh unconstrained
result (`Inst::Undef` → fresh symbolic). Recognizer strips impl/generic segments
(`core::str::<impl str>::len`, `std::vec::Vec::<T>::new`). Soundness control passes
(indexing by a havoc'd `len` stays `unknown`). **orca-core: 167→193 proved,
142→168 functions fully proved, 0 failed** (commit `8f1dbf62ce`). Modest because a
function only fully proves when its *entire* call set is covered *and* its
obligations don't depend on the havoc'd values.

**Refined next-lever priority (after the summary slice):**
- **`address-of Field projection` (377 — now the #1 specific lowering gap)** —
  mechanism (A); needs a struct-field-address path (field-index GEP, trust-mc
  consumer resolves the layout offset, mirror of the working `Inst::ExtractField`).
  Unblocks derived `Clone`/`Default`/`PartialEq` bodies. Highest single lever.
- **Value postconditions on existing summaries** — `str::len ≤ isize::MAX`,
  `as_bytes().len()==self.len()`, `Vec::new().len()==0` (emit an `Assume` after the
  havoc) so value-dependent arithmetic/bounds obligations on already-lowered
  functions prove. Sound (the facts are true).
- **Soundness-tricky calls** (`Default::default` hand-impls, `to_lowercase`/`Vec::with_capacity`
  alloc-panic, `Option::map`/`str::find/split` closure/pattern panic, `Try::branch`
  control-flow) — each needs closure-aware / OOM-model / derived-only analysis.

**Increment landed (2026-06-08): address-of-Field-projection MIR op.** `&place.field`
(refused as "requires layout-aware field offsets", 377 obligations) now lowers to a
GEP advancing the pointer to the field, mirroring the existing array-index address
path; the trust-mc consumer havocs GEP results (sound). Soundness control passes
(`*(&s.a)+1` overflow correctly *failed*, not falsely proved). orca-core 193→196
proved, 168→171 functions (commit `5c319cae7d`). Small gain because functions then
hit the *next* blocker — trait-method calls.

### KEY STRUCTURAL FINDING — the remaining blockers split into two kinds

After the field-op lands, the top remaining call targets are unresolved trait
methods: `std::clone::Clone::clone` (154), `std::default::Default::default` (134),
`std::cmp::PartialEq::eq` (90), `std::string::ToString::to_string` (28) — ~406
obligations. **Crucially, the functions hitting them are CONCRETE derived impls**
(`<agent_hook_endpoint_file::AgentHookEndpoint as Clone>::clone`,
`<worktree_ownership::Worktree as Clone>::clone`, …) — *not* generic code. The
blocker is that inside a derived `Clone`, the per-**field** clone calls appear as
the unresolved trait method `std::clone::Clone::clone` rather than the field type's
concrete impl. So the fix is **trait-method resolution by receiver type at the call
site**, not deep monomorphization:
- `Clone::clone` / `PartialEq::eq` on a `Copy`/primitive field → model as the
  identity / a pure comparison (sound *and precise*: Copy clone returns the value,
  cannot panic);
- on a concrete std type (`String`, `Vec`) → concrete-total summary (OOM excluded);
- on a local struct → resolve to that struct's (in-module) derived impl and lower
  it recursively (already works now that address-of-field lowers).
This is ordinary compiler engineering (resolve `<receiver_ty as Trait>::method` from
the call's receiver operand), and it is the gating item for orca-core's
*zero*-unknown — but bounded and tractable, not open-ended research.

Allocation note: `is_panic_call` recognizes only explicit panic intrinsics, **not**
OOM/`handle_alloc_error` — i.e. Trust's panic model treats allocation as infallible
(the standard verification choice). So *specific concrete* allocating-but-logically-
total functions (`str::to_lowercase`, `String`/`Vec::clone`, `ToString` impls for
concrete types) **are** summarizable within the model; only the *generic* trait
dispatch is not.

**So the road to zero-unknown for the pure crates is now fully mapped:**
1. continue incremental MIR-op + concrete-total-call lowering (each sound, ~+small,
   like address-of-field and the str/Vec summaries) — clears the long tail;
2. add **generic-trait-method verification** (monomorphization or trait-totality
   contracts) — clears the ~406 `Clone`/`Default`/`PartialEq`/`ToString` core of the
   remaining unknowns. This is the one genuinely new Trust capability still needed;
   it is ordinary compiler engineering (resolve the concrete impl per call /
   thread a totality bound), not the open-ended core-verifier research the pre-
   breakthrough analysis feared.

**Progress update (continued same session): orca-core 244 proved / 218 functions
/ 0 failed (from 0).** Landed, all sound (soundness controls pass each time):
- **Trait-method resolution + summaries** (`8d0a99e346`): resolve
  `Clone`/`PartialEq`/`Default` by the governing operand type; primitive/std-type
  leaves → fresh-symbolic summary; local ADTs → resolve to the in-module impl.
- **Inter-procedural derived-impl verification** (`4eeb8b9f9b`): the native callee
  closure now resolves trait-method calls (`ty::Instance::try_resolve`) so a derived
  impl's field-clone of a *local* type bundles `<Inner as Clone>::clone` and is
  verified inter-procedurally. SOUND: a hand-written `panic!`-ing `Clone` is NOT
  falsely proved panic-free (callee panic propagates). `Clone`/`Default` dropped off
  the top blockers.

**Unsupported-op breakdown after the above** (the real remaining backlog, finally
visible under the umbrella): `unknown constant variant` **590** = unhandled
`ConstValue::Str` (`&str` literals) — *being fixed now* (model as opaque slice fat
pointer); `Call target` ~971 (str methods `to_lowercase`/`find`/`split`/`chars` +
`Box::new_uninit` + `Try::branch` + `Option::map`); opaque terminators for
`Deref`/`Clone`/`Default` on non-local/generic types; `unsupported constant` 259.

Seven sound Trust commits on `trust-gap3-wrapping-add` (identity fixes, trust-wp
schema, call/trait summaries, address-of-field, inter-procedural resolution).
Zero-unknown remains a multi-session target via the mapped path — ordinary
incremental lowering, no "multi-month wall".

**Path B progress (2026-06-08).** *Edit A landed & compiling* (committed
`trust-certify` `61430af5a5`): `recheck_cleancic(term, context, lineage,
obligation_violation)` — the consumer-side soundness gate that independently
re-runs the clean-CIC kernel check (term proves `False` under the obligation's
Int env) + re-binds the lineage to the obligation; fail-closed. This is the
re-check the `ImportProofCertificates` path lacks (it admits on producer trust).

*H1 RESOLVED (2026-06-08) — carrier identified, slice-1 fully designed.*
`ProofFormula` (`trust-ir/src/proof.rs:404`) already has `payload: String`
("opaque formula payload in the named schema") + optional `smtlib`/`sort`, and a
`ProofFormula` travels with every obligation. So no bundle-schema change is
needed: stamp the **serialized violation `Formula`** into `payload`
(schema `trust-types.Formula@1`). `ObligationIdentity::from_violation` binds
`violation = format!("{violation:?}")` and nothing else (function/kind/location
empty, `trust-certify/src/lib.rs:91-98`), so the importer can fully reconstruct
both the kernel `var_names` AND the lineage identity from the deserialized
payload.

Slice-1 implementation plan (next, one rebuild):
- **H1a (carrier):** define `payload` schema `trust-types.Formula@1` = serialized
  `trust_types::Formula`; confirm `Formula: Serialize/Deserialize`.
- **Edit C (produce):** at `trust_verify.rs:3559-3603`, on the BoundsCheck
  fallback, `certify_violation(&formula)` → on `Some(CleanCic)` push the
  `ProofCertificate` (status `Discharged`) AND set the obligation's
  `ProofFormula.payload` to the serialized `formula`.
- **Edit B (admission gate):** in `build_certificate_evidence`
  (`native_trust_ir_bundle.rs:852`) deserialize the `Formula` from
  `obligation.formula.payload`, and for a `CleanCic` certificate call
  `trust_certify::recheck_cleancic(term, context, lineage, &formula)`; admit only
  if it passes, else add an `EvidenceCheckFailed` rejection (fail-closed). Thread
  the same gate through the bridge admission (`native_artifact.rs:828` /
  `lib.rs:1369`) that actually emits `Proved` (H2). Add `trust-certify` as a dep
  of `trust-vc-trust-engine` (verify no cycle).
- **Test:** one BoundsCheck under `TRUST_VERIFY_SURVEY=1` flips `unknown→proved`;
  negative control (corrupt one `term` byte or repoint `payload`) reverts to
  `unknown` w/ `EvidenceCheckFailed`.
*Edit A (`recheck_cleancic`) already landed/committed (`61430af5a5`) — it's the
re-check this plan calls.*

*Exact Proved-gate sites (traced 2026-06-08 — the precise implementation handoff):*
The `Proved` `ObligationEvidence` is emitted by
`TrustVcTrustEngine::convert_native_trust_ir_bundle_evidence`
(`crates/trust-vc-bridge/src/lib.rs:397`, `status: EvidenceStatus::Proved` at
`:447`), right after `validate_trust_vc_native_trust_ir_import_matches_obligation`
(`:423`). The gate goes at `:423`: for a `CleanCic` import, call
`trust_certify::recheck_cleancic(term, context, lineage, &formula)` and return a
non-`Proved` (Unknown/`EvidenceCheckFailed`) on failure. BUT the imported artifact
type `TrustVcNativeTrustIrImportedProofArtifact` (built by `from_native`,
`lib.rs:~1369`, from the engine's `build_certificate_evidence`) currently carries
only digests/identities — **not** the raw `CleanCic` `term`/`context`/`lineage`
nor the obligation `Formula`. So the focused remaining slice = (1) extend
`TrustVcNativeTrustIrImportedProofArtifact` (+ `from_native` + the engine
`build_certificate_evidence` source) to carry `term`/`context`/`lineage` +
serialized obligation `Formula` (from `ProofFormula.payload`); (2) add the
`recheck_cleancic` gate at `lib.rs:423`; (3) add `trust-certify` dep to
`trust-vc-bridge` (verify no cycle); (4) Edit C producer stamps the certificate +
`ProofFormula.payload`; (5) one rebuild + survey test. All host-cargo-buildable
except the producer (compiler) + final rebuild. This is a focused multi-crate,
soundness-critical implementation — fully specified, ready to execute.

*Dedicated-run finding (2026-06-08 — the TRUE bottom of path B).* The
compiler-side producer (`trust_vc_native_trust_ir_certificate_import`,
`trust_verify.rs:3620`) and its `ProofFormula` only carry **identity/source
metadata** (`native_trust_ir_obligation_source_formula:3824` → JSON
source_id/span/obligation_id), NOT a logical formula — so `certify_violation`
cannot run there. The structured obligation lives one layer deeper: the **engine**
`TrustObligation` (`trust-vc-trust-engine/src/lib.rs:3370`) carries
`expr: String` + `typed_expr: Option<TrustExpr>`, and `to_trust_vc_typed_obligation`
(`:3661`) lowers `typed_expr` via `expr.to_trust_vc_expr()` into a
`TypedProofObligation` for the trust-vc solver. **So the real path-B integration
point is the engine's typed-obligation verification**, and the one missing piece
is a sound `TypedProofObligation`/`TrustExpr` → `trust_types::Formula` (violation
form) conversion to feed `trust_certify::certify_violation`, mint a `CleanCic`,
and emit it as artifact-backed evidence. Building blocks exist (`to_trust_vc_expr`;
trust-wp `to_trust_formula_payload`/`to_trust_formula_value` in
`trust-wp-core/.../trust_tmir.rs:347`). The engine does NOT yet depend on
`trust-certify`. This conversion + engine wiring IS the core verifier work —
multi-day, soundness-critical (a wrong lowering = unsound proofs), and the
genuine substance of "make Trust prove real Rust". `recheck_cleancic` (Edit A,
committed) remains the consumer-side re-check for the serialized case. There is
**no shorter sound path**: the compiler producer lacks the formula, the CHC
backend that proves the 334 is trust-mc (no import path), and the trust-vc native
route is Unsupported until this lowering exists.

*Deepest finding (2026-06-08 — decisive).* Even the engine's `typed_expr`
(`TrustExpr`, `trust-vc-trust-engine/lib.rs:2616`: Bool/Int/Var/Arith/Compare/
Logic/Not/Implies/Call/Quantifier) is only the obligation's **goal**, not the
verification condition. `certify_violation` needs the **violation** =
`(in-scope assumptions) ∧ ¬goal`; certifying `¬goal` alone proves only
*tautologies* (goals true regardless of context), which real obligations are not.
The assumption set is assembled **inside the trust-vc solver's VC construction**
(where the solver gathers preconditions/path facts before solving). So a sound
`CleanCic` for a real obligation must be minted there — at the solver's core VC
assembly — not from the standalone obligation expression. **Conclusion: path B's
sound implementation is core trust-vc-solver-internals work** (gather the VC →
`certify_violation` on it → mint+emit `CleanCic`), multi-day-to-multi-month and
soundness-critical. No contained single-edit slice reaches a real `proved`; the
foundational fixes, survey mode, `wrapping_add` lowering, and the
`recheck_cleancic` primitive are the in-session-achievable sound increments, all
delivered.

**Path B mechanism (mapped 2026-06-08 — the concrete route to admit CHC proofs).**
Trust already has the certificate machinery; path B is wiring it, not inventing it:
- `ProofCertificate { obligation: ProofId, prover, evidence: ProofEvidence }` +
  `module.proof_certificates` + `ProofLineageManifest`
  (`first-party/trust-mc/.../tests.rs:239-264`).
- `NativeVerificationRequest::TrustVc { mode: TrustVcVerificationMode::ImportProofCertificates,
  obligations, certificates, lineage_roots, … }` — in this mode the native
  verifier **admits/checks pre-computed certificates** instead of re-proving
  (`tests.rs:317-329`).
- The `ay` solver emits proof artifacts (`first-party/ay/src/proof_artifact.rs`,
  `chc_runner.rs`, `api/proofs.rs`).

Path B = bridge those: when the CHC/`ay` solver proves an obligation, capture its
artifact (inductive invariant), package it as a `ProofCertificate` (evidence =
the invariant/`ay` proof), attach it to the `NativeVerificationBundle` with an
`ImportProofCertificates` request, and let the native verifier **check** it (must
check, not trust — else unsound) → `full_verification` reports it proved →
`artifact_backed_proofs` admits it → `proved`. **First slice:** make ONE simple
obligation the CHC solver already proves (e.g. a bounds/`wrapping_add` arith
check) flow a *checked* certificate end-to-end so it reports `proved` under
`TRUST_VERIFY_SURVEY=1`. Investigate: bundle construction in `trust_verify.rs`
(~3060-3096, where `full_result` is built), the `ay`-artifact→`ProofEvidence`
conversion, and the `ImportProofCertificates` checker in `trust-vc`/`trust-mc`.
Substantial (multi-day subsystem integration) but bounded — the pieces exist.

**Key insight & honest scope.** The biggest bucket (334) is *not* a lowering
gap — those obligations are already proved by the solver and only downgraded
because `artifact_backed_proofs = full_verification.is_some()` is false outside
full mode (reporting them "proved" without backing would be unsound). So
"orca-core verifies clean (proved, zero unknown)" requires **full mode's
artifact-backed evidence path working for ALL obligations** — i.e. the native
TrustIr evidence bridge (objective-1 lowering) for every family above + the Unsize
cast, **plus** a non-aborting full mode (objective #3), **plus** Gap 4 for the
`tcargo trust check` entry point. This is a multi-week Trust compiler effort
across ~12 lowering families + the cast + the evidence/mode policy; it is
delivered as validated per-slice increments (wrapping_add is slice #1), not in one
pass. Recommended next slices by leverage: the non-aborting + artifact-backed
*survey* mode (unblocks reporting the 334 as proved once their evidence lands),
then `Unsize` cast lowering (282), then the `Clone`/`PartialEq`/`Deref`/`Default`
call families (derived/std impls whose bodies ARE local or in core).

**Gap 4 (open).** `tcargo trust check`'s pipeline does not honor the scope the
way a direct `trustc` invocation does (the env workaround worked via `trustc`
directly but not through the check pipeline), so the pipeline needs to pass the
local-scope policy through to its trustc subprocess.

Update this section as the loop continues (re-run after the Bug-2 rebuild lands).

---

## ⚠️ VANITY-METRIC CORRECTION (2026-06-08) — the "proved" numbers above were fake

**Every "N proved" figure above (167→193→…→244) was a vacuous self-deception.** All
those "proved" obligations were a single synthetic per-function placeholder —
`trust_mc_default_function` (predicate `bool_literal(false)`, injected by
`ensure_default_trust_mc_function_obligation`, `trust_verify.rs`), which proves
**regardless of whether the function is safe**: it proved for `unbounded(a,b)=a+b`
whose *real* overflow obligation correctly **failed**.

**Honest orca-core numbers (real obligations only, vanity admission excluded):**
- **REAL safety obligations proved = 0** (precondition 284, assertion 142,
  arithmetic_safety 66, unsafe_op 30 — all `unknown`; plus 1985 "custom" =
  unsupported-MIR lowering markers).
- Per-function: **0 `Verified`** (was 218 falsely Verified), 365 `Inconclusive`,
  332 `NoObligations`.

**Fix landed (`abdc94ccac`):** the vacuous admission is excluded from the verdict,
summary, report, and TRUST_JSON. `proved` now means a REAL obligation (validated:
`real_safe` proves `arithmetic_safety`; `real_bug` fails it; `trivial` is
`NoObligations`). Fail-closed (`-Z trust-verify-full`) errors on unproven real
obligations.

**New governing goal:** `trust-goal-real-obligations.md` — count only real,
non-vacuous obligations; vacuity detection + fail-closed; every increment gated on a
real obligation proving on a real function AND its buggy mutant failing. The earlier
"244 proved / native route works" narrative is **superseded**: the identity fix is
real (the engine proves/refutes real obligations on probes), but on orca-core the
real obligations are all `unknown` because the functions don't fully lower. Lowering
is genuine groundwork but moved zero real obligations so far.

---

## 🎯 #1 LEVER (2026-06-08): unsigned-64-bit (u64/usize) arithmetic is unverifiable

**Discovered while trying to land the str::len postcondition.** Isolated with width
probes (`x + 200` per type):

| type | outcome |
| --- | --- |
| i32, u32, i64, isize | **prove/fail correctly** |
| **u64, usize** | **`unknown` ("typed CHC absent")** |

So **64-bit UNSIGNED specifically** doesn't verify — and `usize` is *the* type for
lengths/indices/counts/capacities, so this single gate blocks a large share of
orca-core's real arithmetic obligations (and is why the `str::len` postcondition —
`str::len` returns usize — could not land). **Confirmed NOT unsound** (`u64_unsafe`
is `unknown`, never falsely `proved`); just incomplete.

**Mechanism (traced):** the bridge emits `Inst::Overflow` + `ProofAnnotation::NoOverflow`;
the overflow obligation is encoded as a **bitvector no-overflow predicate**
(`ExprValue::BvAddNoOverflowUnsigned` for unsigned, `…Signed` for signed, in
`trust-mc-core`). The failure is specific to the **unsigned-64-bit BV no-overflow**
path (signed-64 `i64`/`isize` work; unsigned-32 `u32` works). **Ruled out:**
`trust_verify.rs:6000` (trust-formula `IntLiteral` parsed as i64 — a real latent bug
for the `u64::MAX` bound, but fixing it did NOT move u64; reverted as unvalidated),
`:4846` (typed-CHC `int_const` uses the string value — fine), and shift-by-64
overflow (none found in the bridge/trust-mc).

**Fix (the ticket):** the unsigned-64-bit `BvAddNoOverflowUnsigned` (and sub/mul)
construction or its ay-solver evaluation. Soundness-critical (the overflow bound
must be exact) and validatable by the width probe (`u64_unsafe` must FAIL, never
falsely prove). This is the single highest-leverage orca-core fix — landing it
should make many real `usize` arithmetic obligations prove/fail at once.

**Also landed this round (sound):** postcondition-summary capability — the bridge
now emits `Inst::Assume` for a function's range postcondition (`str::len ≤ isize::MAX`),
wiring reusable infrastructure for bounded-result summaries (commit `ba2d2d93a8`).
Moot until the usize bug above is fixed, but correct and validated in isolation.
