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

### usize/u64 gate — sharpened diagnosis (2026-06-08, build #18)

Attempted the obvious fix (the unsigned BV add-no-overflow zero-extended to width+1
= a 65-bit BV the backend caps out at; rewrote it same-width as `bvadd(a,b) >=u a` —
sound, committed in the ay submodule `21861b2`). **It did NOT move the u64/usize
CheckedAdd gate** — because that obligation goes through `Inst::Overflow` (havoc'd in
the CHC route), not the BinOp BV path I fixed.

**Sharper finding (from the full reason string):** for u64/usize the proof-grade
CHC/PDR route returns evidence classified as a **counterexample**, which it then
rejects — *"proof evidence rejected for ChcPdr: counterexample evidence is not a
proof; primary evidence was unsupported"* — for BOTH `u64_unsafe` (real overflow,
should be `failed`) AND `u64_safe` (bounded, should `prove`). i32/u32/i64/isize all
get proper proof-or-failure verdicts. So the gate is in the **u64 proof-grade
CHC/PDR evidence path** (no `PdrInvariant` is produced for u64; the counterexample
model likely carries a value > i64::MAX that the evidence layer treats as
"unsupported"), NOT in the BV no-overflow encoding. Confirmed still NOT unsound
(never falsely proves). **Next step:** instrument the u64 obligation's CHC + the
solver verdict (why no PdrInvariant for `u64_safe`; why the counterexample is
"unsupported") — a focused solver-level debug. Remains the #1 lever (usize is the
dominant integer type).

### usize/u64 gate — complete cross-backend diagnosis (no-rebuild, build #18)

CHC-level probe (`-Z trust-verify -Z trust-verify-level=2`, the non-full path) on
`x + 200` per type:

| type | solver | verdict |
| --- | --- | --- |
| i64, u32 | **ay-smtlib** | `failed` (correctly refuted) |
| u64_safe, usize_safe | **interval** | `unknown` |
| u64_unsafe, usize_unsafe | **ay-incremental** | `runtime_checked` (deferred, not refuted) |

So **unsigned-64 falls out of the strong SMT path** (`ay-smtlib`, which solves
i64/u32) into weaker solvers (`interval`/`ay-incremental`) that can't decide it. And
the native full-verify path rejects the u64 counterexample as "unsupported". Root
cause is a **pervasive i64-limitation in u64 handling across backends** — e.g.
`trust-vcgen/value_analysis.rs:573` caps `width >= 64` at `i64::MAX` (its `ValueSet`
is i64-based and cannot represent `u64::MAX`); the native proof-grade evidence
likewise treats a counterexample value > i64::MAX as unsupported.

**Fix scope:** a multi-site i64→i128 (or BV-native) widening of unsigned-64 handling
across (a) `value_analysis` ranges, (b) the SMT dispatch so u64 reaches `ay-smtlib`
like i64, and (c) the native proof-grade counterexample/model layer. Soundness-
critical, validatable by the width probe (`u64_unsafe` must FAIL, `u64_safe` must
PROVE, i32/u32/i64 unchanged). The same-width BV fix (`ay 21861b2`) is one sound
piece but not the whole. Localizing each site exactly needs eprintln instrumentation
of the u64 obligation path — a focused debug cycle. STILL THE #1 LEVER (usize is the
dominant integer type), now fully scoped.

### usize/u64 gate — ROOT-CAUSED EXACTLY (2026-06-09, build #19, instrumented)

Added two env-gated probes (`TRUST_FORMULA_DEBUG` in `trust-mir-extract::vc_formula_payload`;
`TRUST_CHC_LOWER_DEBUG` wrapping `trust_verify.rs::trust_mc_typed_chc_lowering_for_obligation`),
rebuilt stage2, and re-ran the width probe in full survey mode. The output is unambiguous and
**overturns the build #18 "counterexample unsupported" framing**:

1. **The typed-CHC lowering SUCCEEDS for u64** — `SUPPORTED`, `has_payload=true`,
   `typed_lowered=true`. The obligation is *not* dropped at lowering (the build #18
   "native_trust_ir present:false / unsupported MIR" is downstream fallout, not the cause).
2. **The overflow VC is encoded in LIA (Int), not BV.** Probe smtlib for `u64_unsafe`:
   `… (or (< (+ x 200) 0) (> (+ x 200) 18446744073709551615))`. The bound
   `18446744073709551615` = `u64::MAX` = 2^64−1, **> i64::MAX**. `u32`/`i64` work *only*
   because their type bounds fit i64.
3. **The choke point is one line:** `first-party/trust-mc/trust-mc-driver/src/native/typed_chc_ay.rs:146`
   lowers `IntConst` as `ChcExpr::Int(parse_i64(value)?)`; `parse_i64` (line 289) errors on any
   literal > i64::MAX → `NativeSolveError::InvalidInput` → `trust-bmc/verifier_api.rs:2494` maps
   it to `EvidenceStatus::Unsupported` → `unknown`. `ChcExpr::Int` is `i64` across **2029 sites**
   in ay-chc (`SmtValue::Int` 666 more; enum defs at ay-chc `expr/types.rs:266`,
   `smt/types.rs:212`, `problem/mod.rs:30`).

**Why LIA and not BV:** `trust-vcgen/generate.rs:2603-2606` *deliberately* routes add/sub through
the LIA path (only `mul`→BV) so the conjoined guards/preconditions (`v2_formula_with_block_defs`,
`input_range_constraint`) survive — the existing fresh-operand BV path
(`v2_unsigned_bv_overflow_formula`) drops them and would false-Fail guarded-safe code.

**Two real fixes:**
- **(A) Widen ay-chc `Int` i64→i128.** Principled root fix, but ~2700 soundness-critical sites
  (`ChcExpr`/`SmtValue`/`problem` + all arithmetic/model-extraction). Multi-week; still misses u128.
- **(B) BV-encode wide add/sub overflow *with guards*** ← recommended next implementation. The BV
  path in `typed_chc_ay` is complete and proven (all BV ops lower; `BvConst`→`parse_u128` handles
  u64::MAX). **Key insight: in BV the type-range bound is implicit in the bit-width, so the
  oversized `u64::MAX` literal disappears.** For `u64_safe`: `bvult(x,1000) ∧ bvult(bvadd(x,200),x)`
  → UNSAT → PROVED; `u64_unsafe`: `bvult(bvadd(x,200),x)` → SAT → FAILED. Both correct, sound,
  complete. The bounded-but-real work is consistently BV-encoding the block-def guards (the
  soundness-sensitive piece — must not drop or mistranslate a guard).

Validate either fix with `/tmp/u64fix_probe.rs`: `u64_unsafe` must FAIL, `u64_safe` must PROVE,
i32/u32/i64 unchanged. The instrumentation lives in the working tree (uncommitted, env-gated,
inert under normal runs) and is baked into the current stage2 binary for reproduction.

**Fix (B) DE-RISKED — no stage2 rebuild needed (2026-06-09).** Added a direct ay-chc crate test
(`first-party/ay/crates/ay-chc/tests/u64_overflow_bv_derisk.rs`, uncommitted, passing) that builds
the exact width-64 single-step CHCs fix (B) would emit and solves them via `AdaptivePortfolio`:

| query (BitVec 64) | verdict | meaning |
| --- | --- | --- |
| `reach :- bvult(x,1000) ∧ bvult(bvadd(x,200),x)` (guarded) | **Safe (PROVED)**, `AlgebraicClosedForm`, 0.05s | proving guarded-safe u64 works at 64-bit, no bit-blast blowup |
| `reach :- bvult(bvadd(x,200),x)` (unguarded) | **Unknown** (sound; never Safe) | raw ay-chc returns Unknown on BV refutation |

So the **hard direction — proving safe code at width 64 — works and is fast** (the #6848 BV-loop perf
regression does *not* bite this loop-free overflow VC). The `Unsafe` witness for the unguarded case
is produced one layer up by the trust-mc typed full-verification direct-SMT acyclic-error shortcut
(proven at 8-bit by `native_typed_chc_pdr_solver_refutes_bitvector_add_overflow_with_witness`).

**Remaining work for (B)** is purely in trust-vcgen's overflow-VC builder (`generate.rs`): emit the BV
wrap idiom for wide-type add/sub AND consistently BV-translate the block-def guards (needs a BV-aware
`v2_formula_with_block_defs`), bailing to the LIA path — today's sound `unknown` — on any guard
outside the safe fragment (arithmetic-in-guard, out-of-`[0,2^w)` literal, sign mismatch). It is
soundness-sensitive (a mistranslated guard could yield a false PROVE); gate every increment on the
width probe plus a buggy mutant that must FAIL.

### 🟢 usize/u64 gate CLOSED for add/sub overflow — fix (B) LANDED + validated (2026-06-09, build #21)

Implemented as a single post-hoc BV re-encoding at the FINAL lowering boundary,
`trust-mir-extract::verifier_api.rs::vc_formula_payload` (NOT trust-vcgen — a first attempt there ran
too early: four downstream passes plus trust-router's `mir_router` re-wrap the formula with Int
ranges, and the full-verify formula is only complete at `vc_formula_payload`). `try_widen_unsigned_overflow_vc_to_bv`
recognizes the Int overflow disjunction `Or([Lt(a±b, 0), Gt(a±b, u64::MAX)])`, replaces it with the
exact unsigned wrap idiom (`bvult(bvadd(a,b), a)` / `bvult(a, b)`), translates the relational
guards/ranges to unsigned BV, and drops the now-implicit `[0, 2^w)` bounds. It **bails to the sound Int
formula** on anything outside the relational+overflow fragment, and only triggers on the `u64::MAX`
literal (width 64) — i64 (`lo = i64::MIN < 0`) and u32 (`hi = u32::MAX ≠ u64::MAX`) never match, so
they are untouched.

**Width-probe (`/tmp/u64fix_probe.rs`) — all 9 correct, soundness mutants included:**

| function | verdict | expected |
| --- | --- | --- |
| `u64_safe`, `usize_safe`, `u64_safe_sub` | **PROVED** (proof-grade, fail-closed) | prove |
| `u64_unsafe`, `usize_unsafe`, `u64_unsafe_sub` | **FAILED** | not prove |
| `u64_insufficient_guard` (`if x<u64::MAX {x+200}`) | **FAILED** | not prove (soundness mutant) |
| `i64_unsafe`, `u32_unsafe` | **FAILED** (unchanged) | unchanged |

Non-vacuity is proven by the mutant discrimination: a vacuous prover could not prove `u64_safe` while
*failing* the insufficient-guard mutant.

**orca-core impact (native survey):** real obligations **proved 0 → 26** (0 failed). The dominant
remaining blocker is unchanged — **3113 unsupported**, overwhelmingly derived-`Clone`/`Debug`/`fmt` and
dyn-dispatch lowering (Gap 3's bulk), not arithmetic. So zero-unknown is still far off, but the #1
*arithmetic* lever (u64/usize add/sub) is closed soundly.

**Still open for full usize closure:** `mul` (NIA; a separate fresh-operand BV path exists), `u128`,
signed-wide (`i128`), and guards outside the relational fragment (e.g. preconditions containing
arithmetic) — all remain sound `unknown`. Changes are uncommitted in the working tree
(`trust-mir-extract/verifier_api.rs`); the `ay-chc` de-risk test and the env-gated `TRUST_FORMULA_DEBUG`
probe are also uncommitted.

### NEXT #1 BLOCKER (scoped) — cleanup-edge calls → `Opaque` blocks derived Clone/Debug/fmt (2026-06-09)

With the arithmetic lever closed, orca-core's dominant blocker is **3113 unsupported**, overwhelmingly
`failed to lower <T as Clone>::clone` / `<T as Debug>::fmt` and dyn-dispatch. Root cause located:
`trust-mir-extract/src/convert.rs:118` forces ANY `Call` whose successors exceed its normal return
target — i.e. any call carrying an **unwind/cleanup edge** (drop-on-panic) — to `Terminator::Opaque`,
which `trust-ir-bridge/src/lower.rs:4637` refuses. Derived `Clone::clone` calls its fields' `clone`
(each a call with a drop-on-panic unwind edge), so it's forced to `Opaque` and the bridge's existing
Clone/Default/PartialEq resolution (`resolve_local_trait_impl`, `total_trait_call_on_total_type`,
lower.rs:4563/4583) never runs.

**Fix design (sound):** in convert.rs, preserve cleanup-carrying calls as `Terminator::Call` (normal
target) instead of `Opaque`, so the bridge can apply its totality/resolution logic. Dropping the unwind
edge is sound ONLY when the callee cannot panic; the bridge's `total_trait_call_on_total_type`
(fresh-symbolic) and `resolve_local_trait_impl` (inter-procedural — callee panic IS verified) handle
that, but the **generic fallback** (lower.rs:4596, plain `Inst::Call` to an unresolved callee) must
**refuse** a cleanup-carrying call (else a panicking external call is verified as total — unsound).
That needs a `has_cleanup` signal on `Terminator::Call` (most of the 76 match sites use `{ .. }` and
are unaffected; only construction sites change).

**Deeper finding (completed scoping, 2026-06-09):** the call-Opaque gate is only part 1. The cleanup
BLOCKS themselves (reachable only via the unwind edge) contain non-trivial `Drop` terminators (e.g.
dropping a partially-cloned `String` field during unwind), which the bridge ALSO refuses
(lower.rs:4628 `is_trivially_no_drop_ty`). They sink to `Terminator::Resume` (already a no-obligation
sink). So derived `Clone`/`Debug` for any struct with droppable fields (String/Vec/…) — i.e. most of
orca-core — is blocked by the whole cleanup CFG, not just the call.

**Sound 3-part fix (the unwinding path carries none of the per-operation overflow/bounds obligations
we claim, so it can be dropped soundly — we simply don't verify drop-glue safety, an honest separate
concern):**
1. `trust-types` `BasicBlock`: add `is_cleanup: bool` (rustc's `bb_data.is_cleanup`, available in
   convert.rs:20 but currently unused).
2. `convert.rs`: set `is_cleanup`; for a call whose only extra successor is a cleanup block, emit
   `Terminator::Call` with the normal target (drop the unwind edge).
3. bridge: lower cleanup-block `Drop`s as no-ops and emit no obligations for cleanup blocks (they are
   the unwinding path).

Soundness rests on "cleanup blocks carry no per-operation obligation we claim" — validate carefully
(don't skip a real obligation that lands in a cleanup block). Gate every increment on a derived
`Clone` over a `String`-field struct that PROVES + a hand-written panicking `Clone` mutant that must
NOT prove. This is a substantial multi-crate, soundness-sensitive capability — a focused effort, not a
session-tail add-on.

**SOUNDNESS LANDMINE (found while scoping the minimal convert.rs-only variant, 2026-06-09):** a
tempting minimal fix is (a) drop the unwind edge for direct calls (`convert.rs`: change the Opaque
condition to `func_name=="<indirect>" || target.is_none()`) and (b) stub each `is_cleanup` block to an
empty `Terminator::Resume` in `convert_block`. The CALL part looks sound (the unwinding path carries
none of the per-operation overflow/bounds obligations). BUT stubbing cleanup blocks is **NOT trivially
sound**: `Terminator::Drop` generates obligations — `trust-vcgen/memory_provenance.rs:334` (drop
provenance) and `ownership.rs:299` (ownership) build VCs from drops. Emptying cleanup blocks silently
drops those drop-safety obligations, which could make a function falsely clean on the
provenance/ownership dimension. So the fix MUST first establish which obligation dimensions the
native full-verify route actually claims for cleanup-path drops, and either preserve or honestly
exclude them. **Resolve the drop/provenance/ownership obligation model BEFORE implementing.** (The
arithmetic-lever fix this session had no such landmine — its fragment was provably equisatisfiable;
this one is not, hence the extra care.)

### DETERMINISTIC per-obligation histogram OBTAINED (Obj 5 ✓) + Tier-1 total-summaries batch (build #29, 2026-06-09)

**Objective 5 (CI-grade per-function JSON with reasons) is effectively ACHIEVED.** Ran the *fixed*
standalone `tcargo-trust` (Gap-4 force-clean) in survey/warning mode:
`TRUST_VERIFY_SURVEY=1 tcargo-trust trust check -p orca-core --format json --allow-l0-gaps`. It emitted
**7.8 MB of deterministic per-function rows** (287 fns, 1280 obligations) — NOT the degraded
`transport:missing-json` probe. Each obligation row carries `description` + `outcome.reason` with the
precise blocking MIR op. The Gap-4 fix WORKS end-to-end. (`tcargo-trust` confirmed it targets the
repo-local stage2 trustc.)

**The accounting "bug" from #28 is NOT a bug — it is SOUND.** The survey reports
`0/181 hardened obligations have publishable native proof evidence` and `total_proved=0`. The #28
`proved=1` was the *raw native-engine status*, not the publication-grade manifest disposition. The
manifest gate (`trust-verifier-api/src/lib.rs:2340 evidence_disposition`) requires non-bounded +
sufficient-strength + proof artifacts (`replay_or_check`/`solver_transcript`); the proof_evidence shows
`proof artifact policy satisfied=false; replay_or_check=false; solver_transcript=false`. So
`full_verification_legacy_result_for_obligation` (trust_verify.rs:6688) correctly downgrades it to
Unknown when it isn't in `strict_accepted_ids`. NOTHING to "fix" there — doing so would be unsound. The
real latent gap (separate, deferred): genuine BV-route proofs don't emit solver-transcript artifacts, so
even the 26 arithmetic proofs aren't *publishable*. Attaching real transcripts is a sound future win.

**Deterministic blocker histogram (1280 obligations, FIRST-blocker per obligation):**

| count | blocking op | family |
| --- | --- | --- |
| 679 | `Call target … not present in the TrustIr module` (ALL std/core/alloc; ZERO local) | needs modeled summary |
| 274 | `CastKind::PointerCoercion::Unsize` (→ `&dyn`) + `Transmute` | dyn dispatch |
| 39 | `Drop` opaque terminator | drop |
| 38 | `unsupported operand Const` | constants |

Top std/core call targets (the 679; `not present in module`), ranked:
`Index::index` 68 · `Formatter::write_str` 59+13 · `Box::new_uninit` 58 · `Iterator::collect` 42 ·
`IntoIterator::into_iter` 41 · `Option::unwrap_or` 38 · `ToString::to_string` 37 · `slice::iter` 34 ·
`Vec::with_capacity` 31 · `PartialEq::eq` 31 · `Option::map` 27 · `Option::map_or` 24 ·
`is_ascii_*` 23 · `Option::and_then` 20 · `f64::is_finite` 14 · `String::new` 11 · others ≤8.

**KEY: histogram is FIRST-blocker-per-obligation** → it OVERSTATES any single capability's yield (the
multi-link chain reality, now quantified). Clearing one op peels to the next.

**Tier split by soundness:**
- **Tier 1 (sound modeled-summary — runs NO user code, cannot panic):** `Option`/`Result` structural
  selectors (`unwrap_or`/`is_none`/`as_ref`/…), `String`/`Vec` inherent accessors
  (`new`/`len`/`is_empty`/`with_capacity`/`push`/…), `slice::len`/`is_empty`/`iter`/`first`/`last`,
  primitive `Copy` predicates (`f64::is_finite`, `u8::is_ascii_*`). LANDED in build #29
  (`total_no_panic_call_summary`, lower.rs ~2455). ALSO generalized the sound `len ≤ isize::MAX` assume
  from `str::len` to `String`/`Vec`/`slice` len so `container.len()+k` proves.
- **STRICTLY EXCLUDED as unsound-to-fake (dispatch to user code → can panic):** `HashMap::get`/
  `HashSet::contains` (Hash+Eq on key), `slice::contains`/`to_vec` (Eq/Clone on elements),
  `Option::map`/`and_then`/`filter`/`map_or`/`unwrap_or_else`/`as_deref` (closure/Deref), `IntoIterator`/
  `Iterator::collect`/`next`/`any`, `Ord::clamp`, `ToString::to_string`, `PartialEq::eq` (already
  type-gated). These need closure-panic or type-gated modeling — Tier 2.
- **Structural (real obligations, not fakes):** `Index::index` (68; emits a real bounds VC — high VALUE,
  but only proves when the bound is derivable, so count-flat in isolation), `Box::new_uninit`,
  `Formatter::write_str`, `Unsize`→`&dyn`.

**Validation pending (build #29 rebuilding):** `/tmp/total_probe.rs` — (A) `str/String/Vec/slice .len()+1`
must PROVE (len-assume), (C/D) `o.unwrap_or(0)+200` and `v.len()+k` must stay UNKNOWN (soundness anchors).
Then re-survey orca-core to MEASURE the real count drop (expect a peel, flip on pure-Tier-1 functions).

**Estimated yield (from the deterministic survey, before rebuild):** only **9 functions / 30 obligations**
have ALL first-blockers in the Tier-1 set — the optimistic upper bound on this build's count drop. The
layer-peel wall, now precisely measured: the vast majority wedge on a non-Tier-1 first-blocker
(`Unsize`/`Index`/closures/fmt/`Box`/`Drop`). Tier-1 is sound and correct but modest.

**Higher-value frontier found in the same survey — the 240 obligations that LOWERED (reached the solver,
not lowering failures):** **111** fail on `TyKind::Alias` "Projection was not normalized"
(`ty_convert.rs:444`) — the single biggest lever, but a RESEARCH problem: 4 prior `try_normalize_erasing_
regions` attempts (incl. roadmap §1's `adt_arg_depth<=16` guard) caused E0275 trait-solver overflow on
typenum/zlib-rs even during the stage2 build → DEFERRED, no safe approach yet. **46** fail on
`BvSdivNoOverflow` "unsupported expression for native typed ay-chc lowering" (`days_from_civil` date math)
— REAL signed-div overflow obligations that reached the solver. **~50** derived `PartialEq::eq`
preconditions hit a trust-wp "unsupported trust formula schema".

### NoOverflow predicate expansions LANDED + build #29 torn-read failure → combined build #30 (2026-06-09)

**build #29 FAILED (torn read, not a real bug):** editing `typed_chc_ay.rs` mid-build (to batch NoOverflow
into the Tier-1 build) caused cargo to compile `trust-mc-driver` from a snapshot taken BETWEEN the two
edits (match-arms present, helper fns not yet) → `E0425 cannot find function lower_no_overflow_*`, whole
build aborted (no new trustc, Tier-1 not built either). **Lesson: never edit a source file while a build
that compiles it is running.** Both files (`lower.rs` Tier-1, `typed_chc_ay.rs` NoOverflow) re-validated
standalone (`cargo check`); build #30 rebuilds with BOTH.

**NoOverflow capability (Obj 1, build #30):** added exact, sound two's-complement expansions for ALL 8
bit-vector overflow PREDICATES in `typed_chc_ay.rs lower_expr` (ay-chc has no native overflow op, so they
fell to the `unsupported_expr` catch-all → blocked the 46 `days_from_civil` obligations). Each predicate
is TRUE iff NO overflow (ay-bindings `test_bv.rs`: "returns true if no overflow"). Expansions:
`BvSdivNoOverflow(a,b)`=`¬(a=MIN ∧ b=-1)`; `BvNegNoOverflow(a)`=`a≠MIN`;
`BvSubNoUnderflowUnsigned(a,b)`=`bvule(b,a)`; `BvAddNoOverflowUnsigned(a,b)`=`bvule(b,¬a)`;
`BvAdd/SubNoOverflowSigned`=sign-extend by 1, top two result bits equal; `BvMulNoOverflow{Unsigned,Signed}`=
extend to 2w, multiply, high half zero / equals sign-extension of low w. All equisatisfiable → never
false-PROVE/false-FAIL. Validate `/tmp/overflow_probe.rs`: `safe_div(x)=x/4`/`safe_neg` (bounded) must
PROVE; `still_unsafe_div(x,y)=x/y`/`still_unsafe_neg(x)=-x` must stay UNKNOWN.

**build #30 VALIDATED (2026-06-09, exit 0, 18:43):**
- **NoOverflow SOUND + non-vacuous ✓:** `safe_div(x)=x/4` and `safe_neg` (bounded) → PROVED;
  `still_unsafe_div(x,y)=x/y` (div-overflow + div-by-zero), `still_unsafe_neg(x)=-x`, `still_unsafe`
  (unwrap_or+200) → all correctly REFUTED, NONE falsely proved. Signed-div/neg/add/sub overflow
  predicates now lower and prove/refute correctly.
- **Tier-1 len-assume BROKEN (false-FAIL, not unsound):** `str/String/Vec/slice .len()+1` gets a FALSE
  counterexample — the `len<=isize::MAX` assume (ICmp Ule) does NOT constrain the BV-encoded overflow
  check (mixed Int/BV; likely a build #21 u64-BV-fix regression: overflow moved LIA→BV, assume stayed
  LIA). Over-conservative, sound. Follow-up (task): BV-translate the assume guard into the overflow VC.
- **orca-core count ~FLAT (1280 unknown → 1276 unknown + 4 failed) — for the RIGHT reason.** NoOverflow
  made `days_from_civil`'s `BvSdivNoOverflow(year, 400)` (leap-year `/400`) and workspace_cleanup's
  `scanned_at - last_activity_at` (i64 sub) ANALYZABLE: moved from "unsupported expression" →
  "counterexample". The counterexamples are LEGITIMATE: `days_from_civil` does `year_of_era*365`,
  `era*146097`, `153*month` on raw i64 (overflows for extreme inputs); the i64 timestamp sub overflows
  for extreme values. **These need PRECONDITIONS (`#[requires]`) to prove — a Contracts (Obj 4) signal,
  not a lowering gap.** (tcargo `total_proved` is publication-gated to 0; native BV proofs of safe
  arithmetic don't show there — use direct trustc survey.)

**STRATEGIC PIVOT (data-driven, build #30):** the lowering-capability campaign (u64 ✓, constants ✓,
str/Deref/`?` ✓, NoOverflow ✓, Tier-1-call-peel ✓) made the arithmetic/derived obligations ANALYZABLE —
but the dominant frontier for PROVING orca-core's real obligations is now **CONTRACTS (Obj 4)**:
arithmetic-heavy functions are only safe for valid inputs, so absent `#[trust::requires]` the verifier
correctly refuses. **Prerequisite found: the trust-wp formula-schema separator bug** (`trust_wp.`
underscore emitted vs `trust-wp.` hyphen decoded at trust_formula.rs:62) blocks BOTH derived-PartialEq
preconditions (~50) AND user `#[requires]` claims (both route through trust-wp formula claims). Fixing it
(separator-canonicalize the decoder, like the trust-mc identity fix) is build #31 and unblocks contracts.

### build #41: HANG FIXED + orca-core UNBLOCKED — first full gap count since the regression (2026-06-14)

**The survey completes again.** Validated fix (build #40, ay branch `survey-execute-direct-timeout`,
patch in `solver-handoff/execute-direct-timeout-fix.patch`): `execute_direct`'s `ExecutionContext::new`
reads `AY_DIRECT_SOLVE_TIMEOUT_MS` and calls `solver.set_timeout`, enabling the ay solver's OWN
deadline→should_stop→`theory_backend.rs:531` abort (no solver-logic change; env-gated; Unknown on
timeout, never Proved). **Proof = a clean control:** same build #40 binary + same source — env set →
survey COMPLETES (927 obligations, 2m46s); env unset → `trustc` spins 100% CPU 84s+ in
`ay_lra::compute_implied_bounds` (sampled). Only variable is the env var. (The earlier watchdog #38
covers the typed-CHC path; this covers the *direct* `check_sat` path it missed.)

**First complete orca-core gap count since the hang (260 fns, 927 obligations):**

| outcome | count |
| --- | --- |
| proved | 68 |
| failed (refuted) | 124 |
| unknown | 579 |
| design_requirement | 156 |
| **GAP (not proved)** | **859** |

**Top blocking reasons:** 539 `#[trust(static)]` "solver returned unknown: nat" (the u64/nat arithmetic
— gap-log lever #1, now Unknown not hanging); 82 Add-overflow, 24 Sub, 8 Mul; ~120 hardened-boundary
unsafe_operation (FFI/unsafe). **Work-list (most-unproved fns):** `days_from_civil` (46),
`parse_iso8601_utc_ms` (25), then the string-index family — `title_has_token` (15),
`decode_uri_component` (15), `decode_git_cquoted_path` (15), `build_rg_args_for_quick_open` (20). The
string-index functions are the slice/index-length lever (task #20); the date functions are the contract
+ u64-decision frontier. Tooling: `tools/trust-survey/{survey-orca-verify.sh,survey-summary.py}`.

### build #39: hang REPRODUCED + ROOT-CAUSED — ay-lra level-0 non-termination on u64-overflow atoms (2026-06-14)

Rebuilt stage2 with the watchdog (#38) and ran the bounded survey (`tools/trust-survey/survey-orca-verify.sh`).
The watchdog did NOT stop the hang — because the hang is on a **direct `check_sat`** path
(`ay_bindings::execute_direct → run_check_sat → Solver::check_sat_with_details`), not the typed-CHC/PDR
path the watchdog wraps. Caught it live (trustc pinned 100% CPU) and **sampled the stuck process** —
exact, owner-actionable diagnosis in `docs/rust-migration/solver-handoff/`:

- **Loop:** `ay_dpll::extension::propagate_impl` re-asserts the SAME theory atoms at **decision level 0**
  forever; `ay_sat::cdcl_loop_impl` never advances. Hot spot = `ay_lra::implied_bounds::compute_implied_bounds`
  + `run_post_simplex_propagation`, spilling into bignum (`Rational::to_big`).
- **Trigger:** every looping atom is an **unsigned-64-bit overflow obligation** — `(<= _43 Int(18446744073709551615))`
  (u64::MAX), `(< (+ start _43) Int(0))`, `(< Int(u64::MAX) (+ start _43))`. The `start + len` wrap check
  is encoded in **LIA (unbounded int) with a u64::MAX literal** instead of the finite **bit-vector** theory.
- **Why pervasive:** any orca-core fn doing usize string/slice-offset arithmetic emits this shape (the
  looping formula had THREE distinct `start` SSA vars), so skip-iterate won't scale — orca-core is
  fundamentally un-surveyable until the encoding changes.

**The fix is OWNER-SIDE (corrected after reading generate.rs:2554-2582).** First instinct was "route
add/sub overflow to BV" — but Trust *deliberately* keeps unsigned add/sub on Int/LIA (only MUL goes BV)
because the Int path conjoins operand preconditions/guards/block-defs that let a precondition-bounded
add/sub PROVE; BV uses fresh unconstrained operands and would false-Fail provably-safe code. So
BV-routing add/sub would trade a solver bug for a pervasive completeness regression — the encoding is
sound and intentional. **The actual fix: a same-decision-level no-progress/round cap (or `should_stop`
poll) in the `ay_sat::cdcl_loop ↔ ay_dpll::extension::propagate_impl ↔ ay_lra::check_during_propagate`
handshake** so a non-converging level-0 propagation degrades to Unknown instead of spinning. The owner
already uses caps of this flavor nearby (`dual_simplex_with_max_iters`, `MAX_RECURSIVE_CALLS=256`, the
`#8256` fixpoint count, `expr_split_seen_count>=50`). This spans ay_sat/ay-dpll/ay-lra (their actively-
tuned core) — handed off, not patched here. Lever #1 (u64 unverifiable) is downstream of the SAME bug:
the solver can't decide these linear u64 formulas because it hangs on them. Full hand-off:
`solver-handoff/ay-lra-level0-nontermination.md`.

### build #38: typed-CHC watchdog LANDED on main; stage2 rebuild to exercise it (2026-06-14)

Implemented the watchdog (task #21): `run_native_solve_within_deadline<T,F: FnOnce()->T+Send, T: Send>`
(native.rs:1326) = `thread::spawn` + `rx.recv_timeout(deadline)`, applied in `solve_typed_chc_pdr_full_with_ay`
around BOTH the acyclic-direct SMT counterexample and `solve_pdr_proof`; `watchdog_ceiling =
timeout.saturating_add(2s)` (default 120s); on timeout returns `FullVerificationVerdict::Unknown` (not a hang,
not a false-PROVE). Mirrors the existing SMT-LIB BMC watchdog (native.rs:1712). **Landed: trust-mc `be05d7f14`
→ parent gitlink `aaffe3b879`, pushed, trust 0/0 aligned.**

**Key build-topology finding:** `trust-mc-driver` is linked as a LIBRARY (feature `native-typed-chc-pdr`)
into the compiler via `trust-bmc` → `rustc_mir_transform`, so the watchdog lives inside `librustc_driver`.
The stale stage2 (Jun 13 23:28) PREDATED the watchdog commit (Jun 14 00:04) → **a full stage2 rebuild (#38)
is required to exercise it.** Rebuilding now.

**Honest scope (still PARTIAL):** this watchdog covers the trust-mc typed-CHC/PDR path only. The earlier
survey #7 hang reached an `ay_dpll` level-0 propagation loop via an *uncovered* engine path (trust-vc /
trust-wp / BMC-no-timeout); engines are non-Send borrowed trait objects, so a single clean router-level
thread-watchdog isn't available. **Decision: after #38 finishes, empirically test days_from_civil/orca_core
under a PROCESS-level perl-alarm timeout. If the watchdog now catches the hang → survey unblocked. If it
still hangs on the uncovered path → build an engine-agnostic process-level per-function survey harness so the
orca-core gap count is measurable regardless of any single hang, and hand the precise ay_dpll loop diagnosis
to the owner (their core-solver territory; don't keep chasing engine paths in actively-churning code).**

### CRITICAL: 23h solver HANG found + un-hung; native typed-CHC has no watchdog (builds #36-#37, 2026-06-13)

The build #36 orca-core survey HUNG — a `trustc --crate-name orca_core` process spun at 98% CPU for ~23
hours. Root: my conjunctive-precondition→BV-mul flatten (build #36) made `days_from_civil`'s bound reach
its 4+ i64 muls (128-bit BV products), turning a fast-Fail into a provable-in-principle but intractable
formula — and **the native typed-CHC/PDR solve path (`solve_typed_chc_with_adaptive_portfolio`,
native.rs:1536) runs ay_chc::AdaptivePortfolio IN-THREAD with NO wall-clock watchdog** (only the SMT-LIB
BMC path, native.rs:1712, has `thread::spawn`+`recv_timeout`). So a hard obligation has no deadline and
spins forever (the ay-chc `solve_timeout` isn't checked during bit-blast). Single-mul contract cases
(`t_control`/`p_mul`) proved fast; only mul/div-heavy functions blew up.

**Mitigation (pushed, b604686446):** REVERTED the flatten (restores build-#35 fast-Fail; the
definition-site requires-marker exclusion / Custom-fix is KEPT — orthogonal + hang-free). Rebuilt (#37).
**The watchdog gap is a HIGH-priority systemic robustness ticket (task #21, owner's solver territory —
coordinate): wrap the typed solve in spawn+recv_timeout so any hard obligation returns Timeout=Unknown
instead of hanging.** That is the prerequisite to safely re-enable contract bounds on mul-heavy
functions (days_from_civil would then time-out gracefully). Lesson: a verifier MUST never be able to
hang on one obligation — a per-obligation deadline is a correctness-of-the-tool invariant, not a nicety.

### build #35 MEASURED + mul-precondition regression FIXED (build #36, 2026-06-12)

**build #35 (latest main + my Custom-fix) measured:** Custom-fix WORKS (unsupported=0 everywhere — the
Custom{trust.contract,unsupported} marker no longer fail-closes requires-bearing fns). Contract add/sub/div
PROVE (p_add/p_sub/p_div clean); len_add/vlen_add PROVE; all soundness anchors hold (still_unsafe,
era_calc_unsafe, p_mul_unsafe, len_sub refute); the 3 traps refute with skip-warnings.

**But a REGRESSION surfaced: contract-bounded MUL false-Fails (t_control `x*2` under requires(x>0&&x<10)
PROVED in build #34, FAILS in #35).** Formula dump showed the precondition bounds the Int var `x` but the
mul overflow check uses a fresh BV operand `__trust_ovf_bv_lhs_x` never linked to it. Root cause: the
owner's 133 commits migrated mul-overflow to a fresh-BV-operand lane (9fabe9ab5b) with a reconnection
fn `v2_bv_mul_dominating_guard_constraints`, but its precondition loop called `v2_linear_var_const_fact`
on each WHOLE `func.preconditions` entry — and a real `#[requires(a && b)]` is ONE `And` formula, which
that fn rejects (returns None). The owner's regression test used two SEPARATE flat preconditions, so the
conjunctive (real-source) shape was never exercised. **Fix (build #36, generate.rs): `v2_flatten_conjuncts`
flattens `And` before extracting var-const facts** — sound (only surfaces more genuine caller-discharged
bounds → can't false-PROVE). Companion test `conjunctive_contract_precondition_reaches_bv_mul_operand`
(one-And shape) + the owner's flat test both pass. Co-evolution: extends the owner's recent fn; coordinate.

**Scope of the flatten fix:** enables DIRECT-param contract-mul (t_control, p_mul) + restores the
regression. Does NOT cover DERIVED-operand muls (era_calc's `era*146097` where era=y/400;
days_from_civil's `era*146097`, `year_of_era*365`, `153*(month±)`) — the BV mul lane binds a precondition
fact only to an operand whose NAME matches the param; a derived operand needs block-def bound propagation
(relate era=year/400 to year's bound). **That derived-operand-mul propagation is the next sub-lever for
days_from_civil** (owner's BV-mul territory — coordinate), alongside the slice/len reasoning (task #20).

### Multi-day re-align + latest features + contracts finished (builds #35, 2026-06-12)

Returned after ~4 days; owner advanced origin/main +133 commits. **My gate commit (946bb7a) is an
ancestor — all soundness work (contract_assumption_gate, multimap shadow-detection, bookkeeping
exclusion, len mirror) survived intact.** Fast-forwarded trust local main to b030100324 (clean, 0
ahead), submodules updated; orc 0/0. Rebuilding stage2 (#35) to use the latest features.

**Co-evolution convergence:** the owner's commits directly complete my contracts work —
`9fabe9ab5b vcgen: BV-encode GATED contract preconditions into the mul overflow formula` explicitly
composes with my contract_assumption_gate (it says "holds only GATED assumptions ... coordinating with
the parallel session"), resolving the era_calc/days_from_civil **mul** limitation I'd deferred to v2.
Also `dbc4c454fd` (Neg-wrapped literals in BV precondition translation — my negative bounds) and
`a511ce9c8` (BoundsCheck→trust-vc — native bounds proofs count).

**Last contract blocker FIXED (build #35, reviewed):** every `#[requires]`-bearing fn emitted a
`Custom{trust.contract,unsupported}` definition-site obligation with NO full-verification owner
(engine/mod.rs:163) → fail-close. A requires is the CALLER's burden (call-site VCs) + ASSUMED in the
body (my gate) — so it must not emit a provable definition-site obligation. Fix (verifier_api.rs,
symmetric to the existing Bool(false)-Precondition exclusion): for `api_kind == Requires` skip the
unsupported marker + record a `definition_site_requires_markers_excluded` metadata count.
Ensures/Assert markers UNTOUCHED (still surface as gaps). The router-owner alternative was rejected as
unsound (would swallow ensures gaps; trivial-pass blocked by the artifact policy anyway). 2 regression
tests (requires-skips + ensures-still-surfaces).

**SCOUT (strategic pivot for orca-core contracts):** of the ~130 arithmetic-refuting obligations, only
`days_from_civil` (24) + `parse_iso8601_utc_ms` (18) are clean scalar-param contract candidates — BOTH
done this session (ISO-bounds requires with the shadow-rename the gate demands; validate-then-compute
hardening). **The other 7 refuting fns have NO bounded scalar param** — their overflows derive from
`&str`/`Vec`/`Vec<char>` `.len()` (title_has_token, decode_uri_component, build_feature_wall_tour_depth_
summary, the stable_pane_id/quick_open_filter parsers) or a struct-field delta (workspace_cleanup). They
need a **slice/index-length reasoning capability** (relate an index `i` to the container's `len()` so
`i+k`/`len-k`/`i-1` prove), NOT contracts. **That is the next lever** — and it co-evolves with the
owner's BoundsCheck→trust-vc + aterm spatial-bounds work. After build #35 measures the contract slice,
the loop turns to slice/len reasoning.

### P0 SOUNDNESS HOLE found+fixed: shadowed param transfers requires-bound to wrong variable (build #32, 2026-06-09)

**Context (E0 probe, current merged main):** the contracts body-assumption is ALREADY live — the owner's
parallel lane delivers `#[trust::requires]` conjuncts into VC formulas via `func.preconditions` →
`conjoin_live_preconditions` (trust-vcgen generate.rs:2007/4329). Probes: `p_add/p_sub/p_div/p_mul`
(direct-param arithmetic with honest bounds) all PROVE their body op. The remaining contract noise: each
contracted fn gets +4 spurious obligations (3 failed + 1 unrouted) of pure bookkeeping.

**P0 (probe `t_shadow`):** `#[trust::requires(x > 0 && x < 10)] fn t_shadow(x: i64, z: i64) { let x = z;
x * 2 }` — the body mul PROVED. Formula dump shows why: the VC variable namespace is debug names and
`build_debug_name_map` (trust-mir-extract lib.rs:1281) is last-write-wins, so the precondition's `x`
and the mul operand `__trust_ovf_bv_lhs_x` (the SHADOWED x = unconstrained z) unify → bound transfers
to the wrong variable → **false PROVE on overflowable code**. Confirmed also: `t_mut` (param
reassignment) is correctly caught by the existing kill-set; `t_vacuous` (contradictory requires)
happens not to prove but had no principled gate.

**Fix (build #32, trust-mir-extract):** new `contract_assumption_gate.rs` applied at the single
chokepoint where contract preconditions enter `func.preconditions` (lib.rs, before the enum-discriminant
extend; trust_verify.rs reads only the gated list — verified). Gates: (1) v1 syntax allowlist
(int/bool fragment; Div/Bv/quantifiers → skip); (2) every free var must be a parameter debug name;
(3) **ShadowedParamName**: any body local sharing a referenced param's debug name → drop (the P0 fix);
(4) **ground-witness vacuity gate**: exact i128 evaluation over candidate assignments (literals ±1, 0,
±1; capped 4096) — no witness → no assumption (a found witness is a model, can't false-positive).
Failure drops the assumption with a compiler warning, NEVER the function (assume-nothing is sound:
only weakens PROVE toward FAIL). Call-site Precondition VCs are generated from a separate re-parse and
stay ungated (callers prove the FULL predicate). 6 unit tests in the module.

**Also in build #32:**
- **Bookkeeping exclusion (task #18 class):** the 3 definition-site `Bool(false)` Precondition VCs per
  requires (emitted by contracts.rs/spec_parser.rs/generate.rs for legacy counting, designed to prove
  trivially) were misread by the native lane as CLAIMS ("typed predicate is false") → 3 spurious FAILED
  per contracted fn. Now excluded from the native bundle at `function_to_verifier_api_bundle`
  (verifier_api.rs) with a bundle-metadata count — never silent. Remaining known noise: 1
  `Custom{trust.contract,"unsupported"}` unrouted obligation per fn (v2).
- **len-assume fix (task #15):** root cause confirmed = the TrustIr `Inst::Assume` is invisible to the
  formula lane that produces verdicts. Fix: shared matcher `trust_types::total_call_summaries::
  total_summary_len_bound` (drift-proof, used by BOTH lanes) + the formula-lane mirror in
  `build_semantic_guard_map` (generate.rs: `0 <= len_dest <= isize::MAX` semantic guard pushed after the
  terminator kill). 1911 vcgen tests pass.

**Validation suite staged** (`/tmp/validate_32.sh`): t_shadow must STOP proving; t_control keeps proving;
p_* keep proving with bookkeeping gone; len_add proves / len_sub stays refuted; era_calc sub/div prove
(mul = known derived-operand limitation, v2); all still_unsafe anchors keep failing.

**build #32 VALIDATED (exit 0, 21:21) — everything works EXCEPT one case → build #33:**
- ✅ Bookkeeping exclusion: contract overhead 4→1 obligations/fn (3 false-FAILs GONE); body ops keep
  proving (p_add/sub/div/mul = 1 proved 0 failed; p_noop = just the 1 unrouted Custom). Task #18 core done.
- ✅ len fix: `len_add`/`vlen_add` FULLY PROVE (the build #30 false counterexample is gone); `len_sub`
  (underflow) correctly still refutes — no over-discharge. Task #15 done.
- ✅ era_calc: sub+div PROVE under the requires (2 proved); mul still fails (derived-operand `era`, v2);
  era_calc_unsafe + all still_unsafe anchors keep failing; t_vacuous correctly dropped (witness gate
  fired, warning emitted); t_mut correctly refuted.
- ❌ **t_shadow STILL false-proved.** Diagnosis: optimized-MIR **copy-prop** moves the shadowed binding's
  debug entry onto `z`'s PARAM local (`let x = z;` → debug "x" → _2), so the collapsed per-local name map
  shows no body-indexed local named "x" — the index-based shadow check can't see it; the collision hides
  INSIDE the param range. **Fix (build #33):** gate on the RAW `var_debug_info` name→locals multimap
  (`build_debug_name_multimap`): a referenced name must map to EXACTLY ONE local and it must be the
  matching parameter; any multiplicity (classic shadow `[1,2]`, copy-prop alias, param carrying two
  names) → drop the assumption. Also drops `days_from_civil`-style self-shadowing
  (`let year = year - 1;`) — CORRECT, since the param bound differs from the shifted value; orc-side
  contracts will rename such bindings. 8 unit tests incl. both regression shapes.

**builds #33-#34 VALIDATED + LANDED ON MAIN (trust 946bb7a74d):** t_shadow now REFUTES (proved=0,
the multimap gate fires, 3 skip-warnings); t_control/p_add/sub/div/mul keep proving; len/era/anchor
results unchanged. Build #34 re-validated the combined state after rebasing onto the owner's 6 parallel
commits ("native trust-mc proves arithmetic end-to-end", literal-width adoption, fail-closed binop
lowering) — identical full pass; pushed. **The contracts body-assumption capability (Obj 4 core) is
sound, validated, and on main.** Next: orc-side contracts — days_from_civil (rename shadowed binding +
ISO-bounds requires) and parse_iso8601_utc_ms (validate-then-compute range guard, a genuine safety fix)
→ first real Orca functions proved under documented preconditions.

### MERGED-MAIN RE-BASELINE: parallel main work landed; contracts payoff quantified (2026-06-09, post-align)

All builds #29–#31 work was rebased onto the owner's moving `origin/main` (which independently
reworked the precondition lane) and fast-forwarded; every repo's main now carries the merged state,
build-verified + soundness-probed. Fresh survey on the merged toolchain:

**orca-core: 1280 obligations → 1003 unknown + 205 failed + 72 design_requirements + 0 proved**
(publication gate still requires solver-transcript artifacts). vs build #31 (1209 unknown + 71
failed): the owner's parallel commits moved −206 unknown and introduced a new
`design_requirements` category (72) — the new lane where contract obligations land. The
`"not MIR-derived; router placeholders"` rejection message is GONE from the era_calc probe
(plumbing reworked), but **the core gap is unchanged: a `#[trust::requires]` predicate still does
not constrain body verification** — era_calc still refutes its bounded arithmetic.

**The 205 failed quantify the contracts (Obj 4) payoff:** 130 are REAL arithmetic refutations
(add 86 / mul 16 / sub 28) on input-dependent functions awaiting preconditions —
`days_from_civil` (24), `parse_iso8601_utc_ms` (18), `title_has_token` (12),
`decode_uri_component` (8), `build_feature_wall_tour_depth_summary` (8) … — exactly what the
body-assumption capability converts to PROVED. The other 75 are assertions, of which 48 are the
derived-`PartialEq::eq` placeholder class (vacuity-exclusion ticket). Verdict flipped to
`HasViolations` (70 functions) — honest: these functions genuinely can overflow for extreme inputs.

### trust-wp schema fix LANDED + contracts frontier SCOPED (build #31, 2026-06-09)

**trust-wp schema separator fix (build #31, exit 0):** separator-canonicalized BOTH the decoder
(`trust-wp-core/.../verify_bundle/trust_formula.rs:61`, `schema.replace('_',"-") != …`) AND the router
(`trust-wp-lib/.../trust_ir_native.rs:927 claim_format_for_tmir_schema`, `match
schema.replace('_',"-")…`). Sound (only `_`↔`-` normalized; distinct schemas like `…pure-expr.v1` stay
distinct). **Measured: the "unsupported trust formula schema" errors went to ZERO** (was ~50), and
**orca-core moved 1276 unknown + 4 failed → 1209 unknown + 71 failed** — 67 derived-PartialEq precondition
obligations now DECODE and reach the prover.

**But they FAIL (vacuously), not prove — and that exposes the REAL contracts gap.** The 67 are all
derived-`<Enum as PartialEq>::eq` precondition obligations; they arrive at trust-wp's prover with
`predicate=false` (a PLACEHOLDER, not the real predicate) → `prove(false)` fails → "failed". The contract
probe (`/tmp/contract_probe.rs`, `era_calc` with `#[trust::requires(y∈[-262143,262143])]`) confirms the
mechanism: the precondition is rejected as **"typed trust_mc CHC/PDR input is not MIR-derived; router
placeholders are not proof input"** (`trust-mc-driver/native.rs`, `MirChcPdrObligation::router_placeholder`
gate). So `era_calc`'s bounded arithmetic still FAILS with a counterexample — the precondition is NOT
conjoined into the body's MIR-derived VC as an assumption.

**Contract attribute opt-in (learned):** `#![feature(register_tool)]` + `#![register_tool(trust)]` at the
crate root + `--cfg trust_verify`; then `#[trust::requires(<bool expr over params>)]` /
`#[trust::ensures(|ret| …)]`. tcargo injects this; a bare probe must add it.

**THE contracts capability (Obj 4), now precisely scoped — the validated next major ticket:**
1. **Precondition-as-body-assumption (what `era_calc` needs):** lower the `#[requires(P)]` predicate into a
   typed CHC formula in the SAME MIR-derived representation as the body's VC, and CONJOIN it as a
   hypothesis when discharging the body's obligations. Today P is a disconnected router-placeholder
   (`predicate=false`), so it never constrains the body. This is substantial (THIR/AST predicate →
   MIR-derived typed CHC + assumption wiring), careful, multi-build — do NOT rush at a session tail.
2. Call-site precondition proof already has scaffolding (`generate_callsite_precondition_vcs`,
   fail-closed `Bool(true)` when param-name mapping is incomplete).
- **Honesty note:** the 67 placeholder-`false` precondition obligations are VACUOUS (same class as the
  excluded `trust_mc_default_function` admission). The schema fix moved them unknown→failed; until real
  precondition lowering lands, they should be EXCLUDED from the honest count (a `predicate==false`
  placeholder precondition is not a real obligation). Either exclude them or land MIR-derived predicates.

**Stray DEBUG defect (FOUND + removed in tree, awaiting next build):** an unconditional
`println!("DEBUG: prove_native_pure_predicate: predicate={}")` (`trust-wp-core/.../verify_bundle/proof.rs:95`)
polluted stdout → broke `--format json` (Obj 5 deliverable) once preconditions reach the prover. Removed.
Workaround until next build: `grep -v '^DEBUG:'` the survey JSON.

**Session net (builds #29–#31):** NoOverflow ✓ (sound capability, all 8 BV overflow predicates),
trust-wp schema ✓ (decode layer cleared), Objective 5 ✓ (deterministic per-fn JSON). The lowering
campaign has made orca-core's arithmetic/derived obligations ANALYZABLE; the count stays ~flat because
the remaining proofs need CONTRACTS (precondition-as-MIR-assumption) — the clearly-identified next lever.

### Gap-4 reporting fix IMPLEMENTED + a native-proved-vs-skipped accounting bug SURFACED (build #28, 2026-06-09)

**Gap-4 fix (Obj 2) implemented** in `tcargo-trust/src/pipeline/run.rs` (`run_compiler` + new
`cargo_package_args`): a verification run now force-cleans the target package(s) (`cargo clean -p <pkg>`,
package-only) before the build, so trustc re-runs and re-emits per-function `TRUST_JSON` every time
instead of intermittently degrading to the synthetic `transport:missing-json` probe on a build-cache
hit. `tcargo-trust` is a STANDALONE workspace (path/vendor deps, not rustc-internal) — it builds in ~12s
with plain cargo, INDEPENDENT of the 22-min compiler. Built at `tcargo-trust/target/debug/tcargo-trust`;
NOT deployed to `build/.../stage2/bin/tcargo` (that needs an x.py rebuild). Found the degradation is
INTERMITTENT (old tcargo gave 287 fns this run, 1 earlier) — the fix makes it deterministic.

**Bug it surfaced (the real find):** running the fixed tcargo fresh-verifies, and on the derived
`<AgentHookEndpoint as Clone>::clone` the accounting is INCONSISTENT:
`native full verifier status: Proved; requested=1, proved=1, skipped=0` — the native verifier PROVES the
derived Clone (the cleanup-edge work pays off) — but the legacy summary reports `1 skipped out of 0` and
`-Z trust-verify-full` fail-closes ("skipped verification artifacts are not permitted"). So a
**natively-proved derived-Clone obligation is mis-counted as "skipped"** in the legacy reconciliation
(`trust_verify.rs:1634-1657`: `index_run_result_obligations` / `full_verification_legacy_result_for_obligation`
puts a proved obligation into `skipped_by_id` / returns Unknown). This (a) makes fail-closed `trust check`
abort on orca-core, and (b) means some surveyed `unknown` obligations may actually be native-PROVED but
mis-accounted. **Next ticket:** fix the reconciliation so a native-proof-grade obligation counts as
Proved (not skipped) — likely an obligation-ID match issue between the native run's evidence and the
requested-obligation list (echoes the earlier trust-mc identity-string bug). Modest-but-real count
impact (derived-Clone obligations) + unblocks fail-closed `trust check`. Caveat: the Gap-4 fix as-is
makes fail-closed `trust check` surface this abort — land the accounting fix WITH it.

### trust-wp metadata-dedup infra bug FIXED (build #27, 2026-06-09)

Fixed the active fail-closed duplicate check at `first-party/trust-wp/crates/trust-wp-core/src/
verify_bundle/metadata.rs:359` (`insert_singleton_metadata`): now tolerates an IDENTICAL duplicate
singleton-metadata entry and rejects only a CONFLICTING value (sound either way — identical entries
carry one unambiguous fact). This was the active variant (the survey's "singleton metadata key" wording;
the cfg-gated `verifier_api.rs` 2449/2576 copies were NOT the live path). Result: the `?`-generated
precondition obligations no longer fail closed on the metadata bug — they now REACH the trust-wp pure
verifier. Soundness held (`still_unsafe`→FAILED).

**Count still 1280** — the precondition now wedges one layer deeper: trust-wp "native pure verifier
returned no result for obligation `…:precondition:2`" (it can't PROVE the precondition — likely needs
inter-procedural callee-contract reasoning), plus the Custom-obligation no-primary-owner routing gap for
the function's other obligations. So `?` is a deep chain: `Try::branch` (#26) → metadata-dedup (#27) →
precondition-proving + a residual lowering op (open). Each fix peels one layer.

**Correction (build #27 follow-up):** the "Custom-obligation routing gap" is NOT a fix — those obligations
are `Custom { namespace: "trust.vc", name: "unsupported_mir" }`, i.e. legitimately-unsupported MIR
markers (a `?` op still doesn't lower). So `?` has a residual LOWERING gap beyond `Try::branch`, but its
specific op is OBSCURED in the survey output (only the `unsupported_mir` marker shows, not the op) — a
diagnostic-reporting limitation that ties back to the Gap-4/Obj-5 work (surface per-obligation op
detail). Pinning the residual `?` op needs added instrumentation in `convert.rs`/the bridge before the
next `?` fix.

### `?`/`Try::branch` lowering LANDED — but exposes infra bugs, not lowering gaps (build #26, 2026-06-09)

Added `Try::branch` + `FromResidual::from_residual` to the receiver/dest-governed total-trait path
(type-gated: std `Result`/`Option` → total; custom `Try` falls through, sound). The `?` desugar now
LOWERS — `Try::branch` is gone from both the probe and orca-core (117→0); `from_residual` + the
`ControlFlow` match/downcast are handled by the bridge's normal enum lowering. Soundness held
(`still_unsafe`→FAILED).

**But the count stayed 1280** — and the residual is NOT a lowering gap, it's verifier INFRA bugs the `?`
preconditions now hit:
1. **trust-wp duplicate-metadata fail-closed:** `trust_wp_native_origin_from_metadata`
   (`crates/trust-wp/src/verifier_api.rs:2576`, and the generic helper at 2449) ERRORS if the
   `trust.trust_wp.native-origin.v1` key appears more than once on an obligation — and the `?`-generated
   `precondition` obligations carry it twice. **Sound fix:** tolerate IDENTICAL duplicates, reject only
   CONFLICTING ones (safe either way). Caveat: these fns are cfg-gated (`trust_wp_proof_transport_api`
   etc., set by `build.rs`) with multiple variants — fix the ACTIVE variant.
2. **Custom-obligation routing gap:** "no full-verification primary owner is defined for obligation kind
   `Custom { namespace: … }`" — some obligation kinds have no native full-verifier owner.

These are infra/routing tickets (not Gap-3 lowering). They gate the `?` and table-accessor obligations.
Next-after-those: the genuinely hard `Unsize`→dyn (274, still #1), `Formatter`/fmt (59), closure-bearing
`Option::map`/`into_iter`.

### 🟢 Aggregate-constant modeling LANDED — FIRST count drop since arithmetic (build #25, 2026-06-09)

Added `ConstValue::OpaqueConst` (trust-types): a reference-to-slice/array constant (`&[&str]`/`&[T]`/
`&[…;N]` static tables) lowers to a fresh-symbolic opaque slice fat pointer (reusing the `&str`
pattern — bridge `emit_const`; `convert.rs` produces it for `TyKind::Ref(Slice|Array)`). Sound
over-approximation (contents unconstrained → value-dependent obligations stay `unknown`, never proved).
`ConstValue` is `#[non_exhaustive]`, so only ONE internal match needed an arm (`patterns.rs`
`operand_ty_hint` → neutral `Unit`).

**RESULT — the count moved for the first time since the arithmetic fix:**
- orca-core unknown obligations: **1364 → 1280 (−84)**; "unsupported constant" blocker **211 → 38**.
- SOUND: probe `still_unsafe(x)=x+200` still **FAILED** (the opaque-const change proves nothing it
  shouldn't); table-accessor probes lower past the constant (residual = a separate "Custom obligation
  kind has no full-verification primary owner" routing gap, not the constant).
- Clearing the constant let functions lower deeper, EXPOSING more obligations — `Unsize`/dyn jumped
  **140 → 274** (now #1). Net still −84.

**Lesson refined:** unlike pure layer-peels (cleanup-edge/str/Deref held the count flat), an
opaque-modelable blocker that is some functions' LAST lowering obstacle gets REMOVED, dropping the
count. So the campaign does drive the count down — each opaque/total capability removes its share, even
as deeper obligations surface. Current top blockers: `Unsize`→dyn (274, hard), `Try::branch`/`?` (117),
`Formatter`/fmt (59), `Box::new_uninit` (58), `Option::map` (51, closure), slice methods (46),
`into_iter` (41, closure), remaining constants (38), `ToString` (37), `PartialEq::eq` (25).

### HARD FRONTIER REACHED — easy total-summary layers exhausted (build #24, 2026-06-09)

Added `Deref::deref` + `ToString::to_string` to the receiver-governed total-trait path (type-gated:
std/primitive → total, custom → unsupported, sound). Result: **Deref cleared (92→0)**; ToString
only partially (std-Adt receivers clear; `str`/custom receivers fall through — `str` isn't in
`is_primitive_copy_ty` nor an `Adt`, so it stays unsupported, sound). **Count STILL 1364 unknown / 0
newly proved** — the 4th flat-count build in a row.

**Conclusion (data-proven across 4 builds): the easy total-summary layers are exhausted.** arithmetic
(moved 0→26), `str::*`, and `Deref` are landed and validated; each peeled a layer but the count is
pinned at 1364 because EVERY remaining orca-core function has ≥1 HARD blocker, none of which is a
total-summary:
- **unsupported constants (211)** — `&[&str]`/`&[Enum]` aggregate consts; needs a new `ConstValue`
  opaque-aggregate representation + bridge lowering (convert.rs:1083). Tractable-ish but non-trivial;
  MIGHT move the count for closure/dyn-free table-ACCESSOR fns (`fn x()->&[&str]{&[...]}`).
- **`Unsize`→`&dyn` (140), `Formatter`/fmt (59)** — dyn dispatch + fmt machinery. Hard.
- **`Try::branch`/`?` (120)** — `?` desugar: `Try::branch`→`ControlFlow` match→`from_residual` return.
  Total (no closure) so summarizable IN PRINCIPLE, but needs the ControlFlow enum + match + from_residual
  modeled together. Medium.
- **`Option::map` (57), `into_iter` (45)** — closure-bearing; modeling as total is UNSOUND (hides the
  closure's panic). Need real inter-procedural closure verification + iterator desugar. Hard.
- **`Box::new_uninit` (61)** — uninit alloc. Hard.

No more easy wins: count-movement now requires these substantial, soundness-sensitive capabilities. The
next single fix with a CHANCE of moving the count is the `&[&str]` constant modeling (closure/dyn-free
table accessors); everything else is multi-capability hard frontier. (3 more crates — orca-text/config/
agents — untouched; same blocker families expected.)

### str-method summaries LANDED + validated; chain depth QUANTIFIED (build #23, 2026-06-09)

Added the inherent `str::*` total-summaries (`find/rfind/contains/starts_with/ends_with/split_once/
strip_*/replace/replacen/to_lowercase/to_uppercase/chars/char_indices/bytes/lines/split*/trim_*matches/
repeat`) to `total_no_panic_call_summary` (lower.rs). All total (no panic; OOM excluded), modeled as
fresh-symbolic results. **Validated:** a probe returning each method's result directly (`Option<usize>`,
`String`, `Option<(&str,&str)>`, `bool`) lowers cleanly — so `map_type_ctx` DOES map those result types
(the campaign's key uncertainty, resolved). orca-core: the str-method blockers (~180) are GONE from the
histogram.

**But the count is flat (1364 unknown) — clearing the str layer EXPOSED the next layer** (whole-function
lowering; functions now wedge deeper):

| blocker | build #22 | build #23 |
| --- | --- | --- |
| `str::*` methods (~180) | present | **gone** |
| `Try::branch` (`?` operator) | 31 | **113** |
| `Deref::deref` | 45 | **92** |
| `Option::<T>::map` | 33 | **57** |
| `ToString::to_string` | 32 | **37** |
| unsupported constant / `Unsize` / `Box::new_uninit` / `Formatter` | 211/140/61/59 | unchanged |

**Quantified lesson:** orca-core's string-processing functions are ~6-8 capability layers deep
(`str::*` → `?`/`Deref`/`Option::map` → constants/iterators → …); the unknown count only drops when a
function's ENTIRE stack clears. Next tractable total-summary layer: `Deref::deref` (92, total for
std/Copy types). The HARD layers are closure/control-flow-bearing and NOT total summaries: `Try::branch`
(`?` desugar — early-return control flow), `Option::map`/`into_iter` (call a closure → propagate its
panic, so modeling as total would be UNSOUND). These need real lowering (the `?`/iterator desugar +
inter-procedural closure verification), which is the substantive remaining capability work.

### DATA-DRIVEN RANKED BLOCKER QUEUE + Gap-4 root cause confirmed (2026-06-09)

**Gap-4 root cause CONFIRMED + fix located.** The `--format json` degradation to a single
`transport:missing-json` row happens on a cargo CACHE HIT: `tcargo trust check` runs `cargo build`
(run.rs:213), so when the target crate is already built with the trust RUSTFLAGS, trustc does NOT
re-run, emits no `TRUST_JSON`, and `transport.rs:430 missing_structured_transport_result` fires. Proof:
`cargo clean -p orca-core` before the survey yields **301 real per-function rows** (1364 obligations)
instead of 1 transport row. **Fix (Obj 2/4):** make the check pipeline force trustc to re-run/emit
(e.g. clean the target package, or a cache-busting cfg/flag) before aggregating. Reporting-only, no
soundness risk.

**Ranked blocker histogram (301 fns / 1364 obligations, all `unknown`, post cleanup-edge fix):**

| count | blocker | family |
| --- | --- | --- |
| 211 | `unsupported constant` — **150 are `&[&str]`** (slice-of-str-literal tables), rest `&[Enum]`, `&RangeInclusive`, `&Option<bool>` | reference-to-aggregate CONSTANTS (`convert.rs:1083`) |
| 140 | `CastKind::PointerCoercion::Unsize` (→ `&dyn`) | dyn dispatch |
| 61 | `Box::<T>::new_uninit` | alloc |
| 59 | `Formatter::write_str` | fmt machinery |
| ~200 | `str::split_once/find/replace/chars/to_lowercase`, `ToString::to_string` | str modeled-summaries |
| 45 | `IntoIterator::into_iter` | iterators |
| 45 | `Deref::deref` | deref |
| 33 | `Option::<T>::map` | Option/Result combinators |
| 31 | `Try::branch` | `?` operator |
| 27 | `PartialEq::eq` | derived PartialEq |

**Strategic plan (the loop's actionable queue).** These blockers CO-OCCUR in function families — a
string-processing fn uses a `&[&str]` table AND `str::find` AND `into_iter`, so clearing one alone
doesn't flip it (the multi-link reality, confirmed twice now). So the campaign is per-FAMILY, batching
co-occurring capabilities:
- **Family A — string/table processing (largest):** `&[&str]`/`&[T]` constants + `str::*` summaries +
  `into_iter`/`Option::map`/`Try::branch`. Land together → unblocks the 211-constant + ~200-str-method
  bulk.
- **Family B — derived `PartialEq`/`Clone`:** `PartialEq::eq` + the clone-internal modeling (cleanup-edge ✓).
- **Family C — fmt/dyn:** `Formatter::write_str` + `Unsize`→`&dyn` (the hardest; deferred).
Each capability is a modeled-summary (sound fresh-symbolic / total) gated on a real-obligation-proves +
mutant-fails test. Highest-leverage first fix: model reference-to-aggregate constants (`&[&str]` etc.)
as opaque/fresh-symbolic at `convert.rs:1083`, co-landed with the str-method summaries.

### DIAGNOSTIC-TOOLING GAP is now the bottleneck (2026-06-09) — Objectives 2 & 5

Peeling the remaining orca-core blockers requires a reliable per-obligation REASON, but the reporting
can't supply one: `tcargo trust check -p orca-core --format json` flakily degrades to a single
`<transport>` `transport:missing-json` row (Gap 4) instead of the 365-function rows it sometimes
emits; `--format terminal` prints only `native verifier status: …; unsupported=N` counts (no reasons);
warning mode emits nothing. Net: I can get COUNTS (26 proved / 3113 unsupported) but not a stable
reason histogram, so I can't cheaply rank "which single capability unblocks the most functions."
**This makes Gap 4 / Objective 5 (stable per-function rows: function, obligation, kind, outcome,
REASON) the highest-leverage next ticket — it unblocks efficient diagnosis of everything else.** It's
a reporting fix (no soundness risk): make `tcargo trust check --format json` deterministically
aggregate the per-function `TRUST_JSON` rows (with the native evidence reason) instead of the synthetic
transport probe. Until then, blocker-peeling is guesswork-by-rebuild, which wastes 22-min cycles.

### Cleanup-edge fix LANDED, but ZERO standalone orca-core impact (2026-06-09, build #22)

Implemented the minimal sound version entirely in `trust-mir-extract/src/convert.rs`: for a direct
`Call` with a normal-return target, emit `Terminator::Call` with that target and DROP the unwind/cleanup
edge (don't go `Opaque`); same for `Assert`. The cleanup blocks become unreachable and the bridge
already prunes them (`reachable_block_ids`, lower.rs:2888), so the refused cleanup-block `Drop`s are
never reached. NO `is_cleanup` field, NO bridge change. Soundness: removing a CFG edge cannot introduce
a false PROVE (no assumption added; normal-path obligations still checked); the native route emits no
obligation from cleanup-path drops.

**Validation:** u64 width-probe still all-9-correct (no regression). The derived `Endpoint::clone`
(String+u64) probe no longer hits "Call::clone cannot be soundly lowered" — the cleanup-edge gate is
cleared. The panicking-`Clone` mutant (`Bomb`) correctly stays `unknown` (its `panic!`→`begin_panic` is
a diverging `target=None` call, still `Opaque`). No false-proves.

**But the orca-core impact is NIL:** native survey is UNCHANGED — proved **26** (not up), failed 0,
unsupported 3113; "failed to lower `Clone::clone`" count 39 → 39. Reason: orca-core's derived-Clone
functions are over richer types (nested structs, Vec, enums, String) whose clone INTERNALS hit the
NEXT blocker (String/Vec/enum clone semantics, aggregate construction) once the cleanup-edge gate is
cleared — so they remain `unsupported`, just one step later. The change is correct, sound, and neutral
(no verdict flipped either way), and is a necessary PREREQUISITE for derived-Clone verification, but it
does not move the metric alone. (Earlier "3256 → 0" was a measurement error: that survey's JSON had
degraded to the Gap-4 `transport:missing-json` probe; the authoritative err-file aggregate is
unchanged.) **Lesson: orca-core functions have multi-link blocker chains; the metric only moves when a
function's WHOLE chain is cleared. Derived-Clone needs the cleanup-edge fix AND String/Vec/enum clone
modeling AND aggregate lowering, landed together.** Changes uncommitted (`convert.rs`); kept as the
prerequisite for the coordinated derived-Clone effort.

---

**Resolution of the landmine (analysis, 2026-06-09).** orca-core's obligations include provenance
(295), drop (81), ownership (13) descriptions — all tied to `Drop`. Key facts: (1) a value's drop has
terminators on BOTH the normal exit and the unwind path; stubbing only the `is_cleanup` blocks loses
the **unwind-path** drop obligations only (normal-path drops keep their terminators and obligations);
(2) but unwind-only temporaries (e.g. a partially-cloned `String` dropped only if a later field-clone
panics) have their drop ONLY in cleanup — silently stubbing those is a **silent verification gap**,
which violates the no-bullshit/no-silent-gap bar. **Correct design:** don't stub silently — lower
cleanup-path drops as EXPLICIT ASSUMPTIONS via the existing `TreatedAsAssumption` third state
(`trust_verify.rs`), so the dropped coverage is recorded in the crate's trust base and surfaced in the
report, never hidden. Then: drop the call unwind edge (sound) + record cleanup-block drops as
assumptions (honest) → derived `Clone` verifies on the normal path with the cleanup coverage
explicitly assumed. Validate: derived `Clone` over a `String`-field struct verifies (with the
cleanup-drop assumption listed); a hand-written panicking `Clone` mutant must NOT verify clean.
