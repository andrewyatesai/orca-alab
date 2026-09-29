# The ideal Trust toolchain experience — written from a day of not having it

**2026-08-12.** This document exists because a user asked, reasonably, "you are
building with Trust verification on, right?" and the honest answer was no — not
because anyone chose to turn it off, but because the toolchain's activation model
made *off* the path of least resistance at every fork. Every failure mode below
was hit in this repository, this session, with the commit or command that
demonstrates it. The proposals follow from the failures, not from taste.

The one-sentence thesis: **a verifier earns its keep only when the lazy path runs
it.** Trust's verification is genuinely good — it found two real overflow
refutations and an unbounded allocation in fresh code, and its advisory reports
are precise. But its *packaging* meant that for weeks the workspace compiled as
vanilla Rust while every document said otherwise.

---

## 1. What happened, concretely

### 1.1 Verification was a default nobody was getting

`rust/.cargo/config.toml` opens: *"The Trust toolchain (repo-root
rust-toolchain, or the machine default) compiles this workspace, and the
batteries-on typed verification pipeline runs during compilation."*

Neither antecedent held. There was no `rust-toolchain` file, and the machine
default was stable. The consequence was not an error — it was **silent vanilla
compilation**, in three different shapes:

| invocation | what happened |
| --- | --- |
| `cargo build` from repo root | root has no `.cargo/config.toml` → no `-Z` flags → vanilla, silently |
| `cargo build` from `rust/` under stable | stable **fails to parse** `-Z` → hard error on every unit |
| `RUSTFLAGS="" cargo build` from `rust/` | the documented workaround for the above → vanilla, silently |

Two of the three silently skip the verifier; the third fails so unhelpfully that
it *teaches* you the second. Every gate script in this repo (E1, run-parity,
build-rust-daemon) independently learned the `RUSTFLAGS=""` workaround, which is
how the verification default eroded: each script's author solved their local
problem correctly and the global property quietly died.

**Fixed by:** `rust-toolchain.toml` pinning `trust`, plus a repo-root
`.cargo/config.toml` mirroring the advisory flags (commit `ef5459fbf`-lineage).
The pin alone was *worse* than nothing — trust toolchain with no advisory policy
and no step budget means strict verification of vendor code and a documented
memchr hang.

### 1.2 The compiler ICEs on serde_json, so the authority crate could not verify

The stage2 panics in `trust-mir-extract/convert.rs:2558` analyzing
`serde_json 1.0.150` — vendored or registry, `build` or `test`. Any crate that
depends on serde_json **cannot compile under the Trust toolchain at all.**

This had a sharp consequence: `orca-policy`, the crate holding the fleet's two
authority decisions, gained a serde_json dependency for a port five days ago.
One dependency made the *verification-target* crate unverifiable — and nothing
said so, because nothing was building with Trust anyway. The fix was to move the
JSON parse into the dispatch adapter (already serde-dependent, never a Trust
target) and keep the decision core zero-dep. The crate now builds verified: 51
tests under trust, exit 0.

### 1.3 Source identifiers leak into the solver unsanitized

With verification finally on, `cargo test -p orca-policy` ICE'd:

```
panicked at first-party/ay/crates/ay-dpll/src/api/terms.rs:68:37:
invalid argument to declare_const: symbol name 'store' is reserved
```

Trust lowers local bindings into ay solver constants **by their source name**,
and `store` is an SMT-LIB reserved word. A Rust variable named `store` — in a
credential-store module, where it is the obvious name — takes down the whole
test build with a compiler panic. `match`, `assert`, `and`, `or`, `not`, `select`
are presumably equally lethal. The in-repo fix was renaming variables, which is
backwards: source code should never need to know the solver's keyword table.

### 1.4 The toolchain is missing rustdoc, and it litters crash dumps

- `cargo test` runs doctests; the trust toolchain ships no rustdoc; under the
  pin, the doctest phase of every crate fails with *"'rustdoc' is not installed
  for the custom toolchain 'trust'"*. Worse, cargo resolves bare `rustdoc`
  through the **rustup proxy**, which follows the pin even when `cargo` and
  `rustc` are explicit stable binaries — so the stable escape lanes broke too,
  until they exported `RUSTDOC` explicitly.
- Every ICE writes `trustc-ice-*.txt` into the cwd. The dumps embed absolute
  home paths, so this repo's `check:exported-home-paths` gate correctly flagged
  them as publication leaks. A verifier whose crash artifacts fail the user's
  own lint is generating work with both hands.

### 1.5 The flag surface is a set of interlocking foot-guns

Learned by stepping on each one:

- `RUSTFLAGS` env **replaces** the config `rustflags` table wholesale — so any
  script that sets it for an unrelated reason silently disables verification.
- Doctests read `RUSTDOCFLAGS`, a separate variable that `RUSTFLAGS` does not
  touch; clearing one and not the other produces a half-verified build.
- `-Zno-trust-verify` was *deleted* and now fails flag-parse on every unit —
  the repo's own config warns that flag spellings are "a property of the
  installed stage2, not of the calendar."
- The wall-clock budget (`-ms`) silently drops the slowest functions and breaks
  reproducibility; only the step budget is sound. Nothing in the tool steers you
  to the right one.

### 1.6 The report does not reconcile with itself

One advisory survey of orca-core yields four defensible answers to "how many
failed": 69 (per-function report lines), 81 (Level-0 summaries), 46 (`[kind]
FAILED` rows), 34 (rows with a counterexample field). Meanwhile the single most
important distinction — **refuted with a verified counterexample** (a real bug)
vs **unestablished** (a coverage gap) — is only recoverable by grepping for
`verified_counterexample = true`. In orca-core that query returns 2 real
refutations against 425 gap rows; conflating them is how a fixable list becomes
an unread backlog. And proved obligations emit **no marker at all**, so counting
markers silently hides every success.

---

## 2. What the ideal experience looks like

Each item is the inverse of a failure above.

### 2.1 One switch, cwd-independent, loud when off

`trust` should be the pinned toolchain (`rust-toolchain.toml`, committed) and
verification should be **on by default with no flags anywhere** — the flags
moved into the toolchain's own defaults, not into per-directory config that
evaporates when cwd changes. When verification is *not* running, the build
should say so, once, unmissably:

```
warning: compiled WITHOUT Trust verification (RUSTFLAGS override) — 0 obligations checked
```

The silent-vanilla outcome — the exact bug class this repo spent three days
hunting in its own test gates, "a thing that did not run, reporting success" —
should be unrepresentable.

### 2.2 Degrade per crate, never per invocation

The correct response to "serde_json ICEs" is not `RUSTFLAGS=""` on the whole
build; it is a per-crate skip, declared where dependencies are declared:

```toml
# Cargo.toml of the consuming crate
[package.metadata.trust]
skip = ["serde_json"]        # reason: stage2 ICE, tracked upstream as <issue>
```

with the build printing what was skipped and why. The blast radius of a
compiler bug should be one crate's verification coverage, not the verifier.

### 2.3 Escape hatches that don't lie

`ORCA_RUST_TOOLCHAIN=stable`-style lanes (parity legs, napi builds) are
legitimate: parity asks whether ported logic matches its TS twin — a question
about the code, not the verifier. The ideal toolchain makes that lane *explicit
and honest*: a single `trust=off` spelling that (a) works identically from any
directory, (b) covers rustc and rustdoc together, and (c) leaves a one-line
record in the build output. Today that lane is four environment variables spread
across three scripts, each independently discovered.

### 2.4 Sanitize the solver boundary

Mangle source identifiers before they become solver symbols (`store` →
`v_store$0`), keep a reverse map for diagnostics. A user should learn SMT-LIB's
reserved words from a textbook, never from a compiler panic. Same rule
generalized: **no user input should ever reach a `panic!` in the toolchain** —
`declare_const` has a fallible twin; the pipeline called the panicking one.

### 2.5 Ship the whole toolchain

rustdoc (even a shim that delegates to a vanilla rustdoc for extraction and
skips verification of doctests, stated in one line), and crash dumps under
`$TARGET_DIR/trust-ice/` — never the user's cwd, never with embedded home paths.

### 2.6 One verdict schema, machine-first

A single JSON line per obligation:

```json
{"fn":"orca_policy::decode_once","kind":"overflow:add","verdict":"refuted",
 "counterexample":true,"policy":"advisory","site":"src/lib.rs:141:9"}
```

with `verdict ∈ {proved, refuted, runtime-checked, unsupported, skipped}` and
**refuted reserved for verified counterexamples**. Every number anyone quotes
becomes one `jq` filter, the four-way ambiguity disappears, and proved work is
countable instead of invisible. The human rendering can stay exactly as rich as
today — it is good — but it should be a *view* of this record, not the record.

### 2.7 A triage-ordered exit posture

The experience this repo converged on for its own gates applies verbatim:
distinct outcomes must have distinct exits. Proved-clean 0; refutations nonzero;
*nothing verified* (all skips, toolchain absent) a third code that no chain can
mistake for success. Trust's `advisory` maps all three onto "build succeeded,
read the warnings," which is how 244 vacuous proofs once passed review here.

---

## 3. What Trust already gets right

Credit where it is due, because the core is worth this polish:

- **The verdicts are real.** On fresh code it produced `[overflow:mul]` and
  `[overflow:add]` refutations with verified counterexamples and an
  `[unbounded_allocation]` finding — all three genuine, all three fixed by
  source changes it correctly accepted (`(hi << 4) | lo`, a constant
  allocation bound).
- **The obligation guidance is actionable.** "must hold: the `Add` result must
  fit in its integer type | fix: use `checked_*` …" is exactly the right level.
- **`ay` and `ty` are excellent as standalone tools.** The grant-authority
  certificate (5 obligations, prove-and-catch shape) and the TLA+ exit-delivery
  check were straightforward to author and falsify. The certificate workflow —
  non-vacuity SAT + catch-control SAT beside every UNSAT — is a discipline other
  verifiers should copy.
- **The advisory survey is honest about its own limits** ("verified-and-reported,
  NOT verified-clean"), and the absent-callee residue is documented as one gap
  rather than disguised as many bugs.

## 4. Postscript, one day later — every bug in §1 is fixed, and turning the verifier on found two more

The day after this document was written, the toolchain was actually repaired,
and the repair validated its thesis twice over.

**The §1 bugs.** The serde_json ICE and the reserved-symbol panic were both
ALREADY FIXED upstream (4f7ad78ebd, 439720e01b) — sitting in the 185 commits the
local checkout was behind. Merging origin/main + one local bootstrap commit
(76d408e1cd, the trustdoc→rustdoc sysroot alias) and one stage2 rebuild retired
all three. The doctest story resolved into policy, not absence: the embedded
doctest frontend accepts exactly one trust flag, `-Ztrust-verify=off` (its own
source calls it a "redundant deauthorization"), and anything else — including
nothing, which inherits strict-verify and fail-closes on assert bodies — fails
loudly. Both workload configs now carry that spelling with the reasoning.

**And then the first real workspace contact found two NEW compile-aborting bugs
in the verify pipeline**, both introduced by the very upstream delta that fixed
the old ones:

* `mir_for_trust_verification` promised its callers never need to check
  `Steal::is_stolen` — but its consumers gate on a deliberately WIDER predicate
  than its snapshot does (`optimized_mir_query_would_cycle` vs
  `trust_mir_is_mutually_recursive`), and the dynamic remainder cannot be
  snapshotted. A def in the gap aborted the build. Fixed by making the query
  `Option` and letting the consumers' existing fail-soft paths absorb `None`
  (775d74f050).
* The new dyn call-site reliance rule keys coercion candidates by BARE trait
  DefId and propagates them to supertraits, then resolves without an
  instantiation check — so `std::env::var_os` plus `std::process::exit` in one
  crate (two lines, safe code) ICE'd codegen with SignatureMismatch. Fixed by
  declining candidates whose tupled inputs do not match the site's demanded
  instantiation (21a34694be); the deeper re-keying is a recorded follow-up.

Both fixes are strictly fail-closed: a skipped body and a declined candidate are
unproved sites, never false proofs.

**What this postscript proves about the document above:** §2.1 (silent vanilla)
was the root failure — four of the five bugs existed *undetected* precisely
because nothing was building with the verifier on. The two new ICEs had been in
the tree for three days; the first honest workspace build found them in an hour.
A verifier that runs is a verifier that gets debugged.

## 4b. Postscript, 2026-08-18 — §2.1 happened again, and here is the first real yield number

**Silent vanilla recurred, exactly as §2.1 predicted.** Both cargo configs set
three flags the installed stage2 had deleted — `-Ztrust-verify`,
`-Ztrust-policy`, `-Ztrust-verify-function-budget-steps`. An unknown `-Z` is
fatal on every unit, so nothing under `rust/` compiled; and every workaround that
lets a build proceed (clearing `RUSTFLAGS`, building where the table is not read)
compiles as vanilla Rust with the verifier off. The repo's own instruction —
"PROBE before editing either flag" — had failed twice, so it is now
`config/scripts/check-trust-flag-surface.mjs`, wired into `pnpm lint`, and it
enumerates every tracked `*.cargo/config.toml` rather than naming one.
(Removed in the 2026-09-28 Trust migration: the cargo configs carry no `-Z` flags
any more, so there is no flag surface left to probe, and `pnpm lint` no longer
runs it.)

> **Superseded 2026-08-30 — the flag surface moved back; see §4c.** The
> paragraph below was true of the stage2 installed on 2026-08-18 and is false of
> the sealed toolchain in use now. It is kept because the *lesson* it teaches —
> flag spellings are a property of the installed toolchain, not of the calendar —
> is exactly what the reversal confirms.

The surface now: verification is batteries-on with no on-switch,
`-Zno-trust-verify` is the sole off-switch, `-Ztrust-lame` is what `advisory`
was, and the step budget is gone in favour of a wall-clock `-ms` twin. That last
one forced a decision this file had explicitly forbidden — §1.5's "never use the
`-ms` twin" was written when a step budget existed. It does not, and the memchr
hang it was written for is still live: measured today, `rustc --crate-name
memchr` ran **4m17s** on that one crate with no output. Terminating with a
wall-clock budget beats a verifier nobody leaves on.

### The first end-to-end yield measurement

`orca-core`, 217 functions, 322 obligations, 42m12s, `-Ztrust-lame`, 5s budget:

| outcome | count | share |
| --- | --- | --- |
| proved | 18 | 5.6% |
| failed | **0** | 0% |
| unknown | 54 | 16.8% |
| timed out | 48 | 14.9% |
| runtime-checked | 202 | 62.7% |

**Nothing was refuted.** The gap is coverage, not defects.

### §2.2 again: the dominant blocker is `unsupported`, on code with nothing to prove

The config in this repo attributes ~69% to "absent callee (a std/extern function
whose body is not in the lowered bundle)". That explanation is stale. The
current stage2 reports these as `runtime-checked` with
`native verifier status: ... unsupported=1`, and the worked example is
`agent_kind::agent_kind_to_tui_agent`:

```rust
pub fn agent_kind_to_tui_agent(kind: Option<&str>) -> Option<&'static str> {
    let kind = kind?;
    if kind.is_empty() { return None; }
    TUI_AGENT_KIND_PAIRS.iter().find(|(_, k)| *k == kind).map(|(agent, _)| *agent)
}
```

No indexing, no arithmetic, no `unwrap`, no `unsafe`. One obligation, `proved=0`,
`unsupported=1`. There is no way to write those nine lines more provably, which
is what makes this a prover-coverage report rather than a code-quality one. A
source-side lever does exist and does work — `v[i]` is unproved where
`v.get(i)` proves silently — but it cannot reach obligations shaped like this.

One reason string looks like a plain defect rather than a coverage limit, and is
the most actionable thing here:

```
compiler-derived trust-mc public semantic contract finalization failed:
verifier unsupported contract reason is empty, untrimmed, or exceeds its byte limit
```

That is the verifier failing to finalise its own contract string, reported as if
the user's code were unsupported. §2.6's "one verdict schema" would have caught
it: a status whose explanation is "the explanation did not fit" is not a verdict.

### What was NOT the lever, so nobody re-tries it

`-Ztrust-verify-include-dependencies` reads like the fix for a large `unsupported`
share and is not. It scopes in *authenticated dependency-crate* MIR, not std;
vendored crates built by stock rustc are unlikely to qualify; and it would pull
`memchr` and `regex` into verification scope — the crate measured hanging above.
Untested here deliberately: on `orca-core` the plausible outcome is a build that
never finishes, not more proofs.

## 4c. Postscript, 2026-08-30 — the flag surface moved BACK, and the toolchain is now sealed

**The reversal.** Everything §4b called gone is present again, and both of its
named replacements are gone. Probed directly against the installed toolchain
(`rustc +trust -Z help`):

| flag | §4b (2026-08-18) | measured 2026-08-30 |
| --- | --- | --- |
| `-Ztrust-verify` | deleted | present — `on` is the default, `off` is the sole deauthorization |
| `-Ztrust-policy` | deleted | present — `strict` (default), `certify`, `advisory`, `memory-safe` |
| `-Ztrust-verify-function-budget-steps` | deleted | present, and its help text now argues for itself: a "DEADLINE-INDEPENDENT … provable monotone-decreasing bound that guarantees termination even with the clock disabled" |
| `-Ztrust-lame` | the new `advisory` | **absent** |
| `-Zno-trust-verify` | the sole off-switch | **absent** |

Both config tables were repaired for this surface at `4fcb89723`, whose message
records the same reversal from the other side. `check-trust-flag-surface.mjs` —
the probe §4b introduced — now passes: *"every configured trust flag is accepted,
across: .cargo/config.toml, rust/.cargo/config.toml"*. This is the second flip in
twelve days, which retires any temptation to write a flag list down as durable:
the probe is the record, this table is a snapshot.
(The probe itself was removed in the 2026-09-28 Trust migration, for the reason
given in §4b: with no `-Z` flags left in any cargo config, there is nothing
for it to probe.)

**One consequence nobody has acted on yet.** §1.5 wanted the step budget and §4b
conceded the `-ms` twin only because no step budget existed. One exists again, so
the trade §4b was forced into is no longer forced — both tables still set
`-Ztrust-verify-function-budget-ms=5000`, and moving back to a deterministic step
budget is now an open, purely-mechanical choice rather than a wish.

**The toolchain is a sealed artifact now, not a live build tree.** `rustup`'s
`trust` channel resolves through `~/toolchains/trust-current` to an immutable
promoted seal (`rustc 1.99.0-dev (1979a7b85 2026-08-28)`), so a stage2 teardown
in the compiler repo can no longer empty this workspace's toolchain mid-build.
The cost is a new staleness axis: **a landed compiler fix is invisible here until
a new seal is promoted.** Measured against this seal:

* the cross-crate loop-stamp ICE ("unwrapping cross-crate data") is fixed at
  `189e37392a` (*fix(trust-mir-extract): reject foreign inlined loop scopes*),
  and that commit **is** an ancestor of the sealed revision — so this workspace
  has the fix;
* the vcgen per-function budget overrun is fixed at `d9d9d675e3`
  (*fix(verify): a preprocessing budget overrun costs completeness in advisory,
  not the build*), and that commit is **not** an ancestor of the sealed
  revision — so a budget overrun here still behaves the old way until the next
  seal.

**And §2.1's real answer is still outstanding in this repo.** The configs are
right and the pin is right, but every routine build and test script still selects
rustup `stable` explicitly, so the shipped artifacts are compiled without a
verifier and `pnpm verify:rust` is the only lane that runs one. That is silent
vanilla wearing a different hat: nothing lies about it any more — README and
AGENTS.md both say it — but the lazy path still does not run the verifier, which
was this document's one-sentence thesis.

## 5. Priority order, if the Trust repo takes one thing

1. **§2.1** — silent vanilla is the root failure; everything else was
   discoverable once builds stopped lying about whether the verifier ran.
2. **§2.2** — the serde_json ICE turned one upstream bug into "the authority
   crate is unverifiable," and the only mitigations were architectural.
3. **§2.6/2.7** — until verdicts are machine-readable and exits are truthful,
   every downstream gate reinvents grep-and-hope, and gets it differently wrong.

The rest is quality of life. These three are correctness of the *meta-system* —
whether "we verify this code" is a statement anyone can trust without reading
the build log.
