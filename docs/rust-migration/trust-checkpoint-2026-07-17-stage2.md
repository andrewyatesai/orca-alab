# Trust checkpoint — measured on the 2026-07-17 stage2

A baseline, deliberately taken *before* upgrading the toolchain, so that the
re-measurement on current Trust has something to diff against. Every number here
was measured on 2026-08-19 against a verifier built a month earlier.

**Read the headline with its caveat attached, always: 18 of 322 obligations
proved, 0 refuted — on a stage2 that is ~1,584 commits stale and carries at
least 6 soundness bugs already fixed upstream.** That is a lower bound from a
known-buggy verifier. It is not Trust's ceiling, and this document exists so
nobody later mistakes it for one.

## 1. Exactly what was measured

| | |
| --- | --- |
| verifier | `rustc 1.99.0-dev (4c9dc90f4 2026-07-17)` |
| installed as | `~/.rustup/toolchains/trust` → symlink → `~/trust/build/host/stage2` |
| binary built | 2026-07-17 19:36 |
| local Trust checkout | `81e4b815d0`, 2026-07-18 |
| stage2 → local HEAD | **340 commits behind** |
| local HEAD → origin | **1,244 commits behind** |
| `fix(SOUNDNESS):` commits upstream, unbuilt | **6** |

Reproduce the identity with:

```sh
rustup run trust rustc --version --verbose
cd ~/trust && git log --oneline 4c9dc90f4..HEAD | wc -l
cd ~/trust && git log --oneline HEAD..@{u} | wc -l
```

## 2. Verification was OFF, and had been

Before anything could be measured it had to be turned on. Both cargo configs —
`.cargo/config.toml` and `rust/.cargo/config.toml` — set three flags this stage2
no longer accepts:

```
-Ztrust-verify=on                          unknown unstable option
-Ztrust-policy=advisory                    unknown unstable option
-Ztrust-verify-function-budget-steps=…     unknown unstable option
```

An unknown `-Z` is fatal on **every** unit, build scripts included, so nothing
under `rust/` compiled. Worse, every workaround that lets a build proceed —
clearing `RUSTFLAGS`, building from a directory where the table is not read —
compiles as vanilla Rust with the verifier silently off. `run-parity.mjs`,
`build-orca-git-wasm.mjs` and `build-terminal-addon.mjs` all pin to **stable**,
which has no verifier at all, so no routine gate would ever have noticed.

Fixed in `07f1b0681e`. `config/scripts/check-trust-flag-surface.mjs` now probes
every tracked `*.cargo/config.toml` against `rustc -Z help` on each `pnpm lint`.

### Flag surface as of this stage2

| accepted | meaning |
| --- | --- |
| *(nothing)* | verification is **batteries-on**; there is no on-switch |
| `-Zno-trust-verify` | the sole off-switch |
| `-Ztrust-lame` | non-fatal reporting — what `-Ztrust-policy=advisory` was |
| `-Ztrust-verify-level=0\|1\|2` | safety / functional / domain |
| `-Ztrust-verify-function-budget-ms` | wall-clock per-function budget |
| `-Ztrust-verify-output=human\|json\|both` | output mode |

Default policy is **strict**: one unproved Level 0 obligation is a build error.
Confirmed directly — `pub fn idx(v: &[u32], i: usize) -> u32 { v[i] }` fails the
build without `-Ztrust-lame`.

## 3. The yield

`orca-core`, full build, `-Ztrust-lame -Ztrust-verify-function-budget-ms=5000`:

| outcome | count | share |
| --- | --- | --- |
| **proved** | 18 | 5.6% |
| **failed** | **0** | 0% |
| unknown | 54 | 16.8% |
| timed out | 48 | 14.9% |
| runtime-checked | 202 | 62.7% |
| | **322 obligations over 217 functions, 42m12s** | |

Reason strings behind those buckets:

| count | tag | reason |
| --- | --- | --- |
| 202 | `[assert] runtime-checked` | "compiler retained the existing assertion because the proof is not yet static" |
| 36 | `[unknown] UNKNOWN` | "native full verifier evidence status: Unsupported" |
| 12 | `[unknown] TIMEOUT` | budget |
| 1 | `[postcond] UNKNOWN` | "proof evidence was not accepted by the compiler's strict full-verification policy" |

**Nothing was refuted.** The gap is coverage, not defects.

### The worked example

`orca_core::agent_kind::agent_kind_to_tui_agent`, in full:

```rust
pub fn agent_kind_to_tui_agent(kind: Option<&str>) -> Option<&'static str> {
    let kind = kind?;
    if kind.is_empty() { return None; }
    TUI_AGENT_KIND_PAIRS.iter().find(|(_, k)| *k == kind).map(|(agent, _)| *agent)
}
```

No indexing, no arithmetic, no `unwrap`, no `unsafe`. One obligation:
`proved=0, unsupported=1`, left as a runtime check. There is no way to write
those nine lines more provably — which is what makes this a prover-coverage
measurement rather than a code-quality one.

A source-side lever does exist and does work: `v[i]` is unproved where
`v.get(i)` proves silently. It simply cannot reach obligations of the above
shape.

### One reason string that looks like a defect, not a limit

```
compiler-derived trust-mc public semantic contract finalization failed:
verifier unsupported contract reason is empty, untrimmed, or exceeds its byte limit
```

The verifier failing to finalise its own explanation, surfaced as though the
user's code were unsupported.

### memchr still does not terminate

Measured today: `rustc --crate-name memchr` ran **4m17s** on that one crate with
no output and no sign of stopping. The documented fix was a *step* budget, which
this stage2 deleted. The surviving `-ms` twin is what makes builds finish, and
`rust/.cargo/config.toml` used to forbid it in writing — that prohibition was
written when a step budget existed. A verifier that hangs is a verifier nobody
leaves on, which is how it came to be off.

## 4. What this checkpoint does NOT license concluding

Mid-measurement I concluded "this is Trust's ceiling." **That was wrong**, and
the reason is in the unpulled commits:

```
e177c96cf1 fix(SOUNDNESS): a refuted predicate reports Failed, not Unsupported
ebee63ca8a fix(SOUNDNESS): proved counts distinct obligations, and bounded proofs block the run
a5e0b3d844 fix(SOUNDNESS): a `library/` path segment no longer deletes unsafe obligations
```

The first means some of the 202 + 36 `Unsupported` may actually be **refuted
predicates mislabelled** — i.e. real findings hidden as coverage gaps. The second
changes how `proved` itself is counted. So both the numerator and the
denominator above are suspect, in *both* directions.

## 5. Plan: re-measure on current Trust

The point of this file is the diff. Sequence:

1. **Pull and rebuild.** `cd ~/trust && git pull && ./x.py build --stage 2`.
   Expect this to be long, and expect the `~/trust` bootstrap caveats recorded in
   `docs/rust-migration/trust-toolchain-experience.md` to apply.
2. **Re-probe the flag surface first.** `node config/scripts/check-trust-flag-surface.mjs`.
   (Removed in the 2026-09-28 Trust migration: the cargo configs carry no `-Z` flags
   any more, so there is no flag surface left to probe.)
   The surface has now flipped three times; assume it moved again before assuming
   anything else. Fix both configs if it did.
3. **Re-run the identical measurement.** `pnpm verify:rust orca-core` — same
   crate, same `-Ztrust-lame`, same 5000ms budget, so the diff means something.
4. **Diff against §3**, four numbers specifically:
   - Did **failed** move off 0? If the refuted-vs-unsupported fix converts even a
     few, those are real defects this checkpoint could not see, and they matter
     more than the proof count.
   - Did **proved** move, and is it comparable given the distinct-obligation
     counting fix?
   - Did **runtime-checked** (202) fall?
   - Did **42m12s** change, and does `memchr` still hang unbudgeted?
5. **Only then** tune the budget. Raising 5000ms recovers some of the 48
   timeouts, but that is the smallest of the three gaps and the easiest to
   mistake for progress.

### What NOT to chase

`-Ztrust-verify-include-dependencies` scopes *authenticated dependency-crate*
MIR, not std. Vendored crates built by stock rustc are unlikely to qualify, and
it would pull `memchr` and `regex` into verification scope — `memchr` being the
crate measured hanging above. On `orca-core` the plausible outcome is a build
that never finishes, not more proofs.

Chasing the 48 timeouts first, for the same reason: it is 14.9% and it is the
one bucket whose size I chose.

## 6. Wishlist — check these off against the new toolchain

Written from the friction of actually using it, in rough order of how much time
each cost. Later, count how many already exist.

**Reporting**

1. **A per-crate verdict summary the compiler emits itself.** I had to write
   `config/scripts/run-rust-verification.mjs` to parse per-function lines into a
   table. The full build emitted only *2 warnings* for a crate with 54 unknown
   and 48 timed-out obligations, because lame mode warns on `failed` and there
   were none. The interesting numbers were invisible in build output.
2. **`--message-format=json` integration**, so verdicts arrive as structured
   cargo diagnostics rather than needing a regex over stderr. (`-Ztrust-verify-output=json`
   exists — check whether it composes with cargo.)
3. **A verdict schema stable enough to gate on**, so a downstream check can
   assert "proved count did not regress" without grep-and-hope.

**The flag surface**

4. **A version-pinned flag manifest** the build can validate against — the thing
   `check-trust-flag-surface.mjs` is a workaround for. The surface flipped three
   times; each flip presented as a total build failure that read like a broken
   toolchain rather than a stale config.
5. **Deprecation instead of deletion.** `-Ztrust-verify=on` vanishing silently is
   what turned a rename into a month of no verification. A warning that named the
   replacement would have cost minutes.

**Termination**

6. **Either a step budget back, or a documented termination guarantee.** The
   `-ms` twin trades reproducibility for finishing; the step budget traded
   nothing. Losing it forced a worse default.
7. **A hang diagnostic.** `memchr` produced 4m17s of nothing. Even
   "still working on function N of M" would distinguish slow from stuck.

**Coverage**

8. **Actionable `unsupported` reasons.** "Unsupported" on a nine-line function
   with no indexing tells me nothing I can act on. What construct? Is it the
   iterator chain, the `?`, the const slice?
9. **"What would make this provable?"** — the single highest-value feature on
   this list. The verifier knows why it failed; `v[i]` → `v.get(i)` is a rewrite
   it could suggest.
10. **Contracts for std**, or a story for the absent-callee residue that is not
    "wait for the Trust-Std campaign."

**Practicality**

11. **Incremental verification.** 42 minutes for one crate means it cannot sit in
    front of a commit, which is why it ended up in a lane nobody ran.
12. **Cached proofs across builds**, keyed by function body hash — most functions
    do not change between builds.
13. **Per-crate policy without `RUSTFLAGS` gymnastics.** Today ratcheting one
    crate to strict means duplicating the whole flag set into an env var and a
    separate `CARGO_TARGET_DIR`.
