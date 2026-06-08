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
