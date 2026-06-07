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

This file is the durable record of that loop; update the gap log with real
verifier output once stage2 is built.
