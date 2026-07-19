# Session Handoff — Autoformalization Factory (Goal A / Moonshot Campaign 5)

**Last updated:** 2026-07-18 · **Prev session:** claude.ai/code/session_01PDoUVWcffFj7PnJKLqtQ6g

This is the resume point for the ts2rust autoformalization factory work. Read
[`extreme-performance-moonshot.md`](./extreme-performance-moonshot.md) §9 (Campaign 5)
and the memory `orc-goals-and-gauntlet.md` for the full frame; this doc is the operational
"what's true now / what to do next."

---

## Update 2026-07-18 (later) — F3 scanner shipped + ultracode burst + Electron fork kill-check

- **F3 candidate scanner is live** (`tools/autoformalize-candidate-scanner.mjs`, `pnpm
  autoformalize:candidates`): AST walk classifies every exported fn by purity → **392 pure +
  396 needs-inline** candidates, dup-flagged + security-ranked. This REPLACED hand-grep scouting and
  is now the factory's engine. Workflow: regenerate the JSON, pull the top N `pure-self-contained &&
  !ported` (dedup by existing `g_*.rs`), feed `grow-corpus`.
- **Ultracode burst**: 4 scanner-driven waves of ~20, **74/74 TRUSTED** (near-100% first-drive because
  candidates are pre-vetted pure). Corpus grew ~302 → **~413 `.rs` files**. Cadence used: commit each
  wave immediately; DON'T run a census mid-burst (it starves the waves — one hit 95 min contended and
  was killed); pause and run ONE clean census to bump the floor. **A consolidating census (census13)
  is running now — bump the ratchet to its result; if a heavy kernel flakes (30+ obligations near the
  solver budget, cf. g_titleframechg), defer it and set the census-confirmed floor.**
- **Electron fork (Campaign 3): green-lit, but measure-first RETIRED the marquee patch.** The Rung-2
  Phase-0 kill-check (`pnpm fork:killcheck`, `tools/orc-electron/`) found **STOCK-RUNG-2-SUFFICES** on
  Electron 43: `crossOriginIsolated` + durable SAB + `<webview>`-attaches-under-COEP all work on the
  stock binary → the origin-isolation fork patch is unnecessary; ship rung-2 (`orca://` + COOP/COEP)
  instead, no Chromium rebuild. The fork's remaining value (macOS low-latency canvas, component
  stripping, V8 snapshot, PGO) is `--i-mean-it`-gated in `bootstrap-fork.sh`. **The honest high-value
  next runtime step is in-app rung-2 serving** (doc §7 rung 2: no `orca://` scheme exists yet; two
  `file://`-hardcoded sender-trust gates to fix; then COOP/COEP → durable SAB + wasm threads).

## State of the world (verified, committed, pushed)

- **Corpus: 320 individually-verified TRUSTED kernels** (last FULL census confirmed **302/308**, 0
  residual; +18 additive light kernels since, final consolidating census in flight at handoff time —
  bump the ratchet to its result), i.e. **100% of all non-control kernels** carry a trustc ∀-safety
  proof (W1) **and** a 0-divergence differential (W2). The **6 `_bug`/`_naive` soundness controls stay
  refused** (computeEditorFontSize, countWhitespace, packRgb, parseCsiParams, sumPositive, unpackRgb).
  0 declined, 0 residual, 0 breaks. **Campaign 5's factory-breadth demonstration is COMPLETE**: ~77
  fresh real-orc functions proven past the original 100%, spanning every class — shell-injection
  quoters, credential scrubbing, JS/HTML/paste sanitizers, CSI/VT500 escape parsers, git-domain
  parsers, session-path parsers, dispatch parsers, clamps/predicates. The remaining moonshot fronts
  are XL (F3 swc/oxc auto-parsing, kernel promotion to shipped napi/wasm, the scoped T-D Shr fix) or
  gated on Andrew (ay/ty publication, Electron fork) — a fresh session should pick ONE, not more breadth.
  Methodological refinement the agents converged on (adopt it): a **discrimination control** — run a
  deliberate no-op port against the seed corpus and require divergences — proves W2 non-hollowness.
- Ratchet: `tools/terminal-bench/autoformalize-ratchet.json` (`minTrusted`). It is the regression gate.
- Reproduce: `cd ~/orc && pnpm gauntlet:autoformalize`. NOTE: a full census is now **~1 hour** (serial
  over ~299 kernels; the CSI/seed-heavy kernels are slow), not ~10 min. **Cadence:** an ADDITIVE
  grow-corpus wave changes no existing kernel and no toolchain file, so the controls + existing kernels
  cannot regress — ratchet on the wave's per-kernel TRUSTED verdicts + a spot-verify, and run the full
  census only as a PERIODIC checkpoint (every ~3 waves) and after any TOOLCHAIN change. Never skip the
  full census + soundgate after a trustc/verify.mjs/fuzz.mjs/gauntlet.mjs change.
- Toolchain: the LOCAL `~/trust` stage2 build (`~/trust/build/host/stage2/bin/trustc`,
  rustc `1.99.0-dev 4c9dc90f4`). Corpus is LOCAL-only at `~/trust/tools/ts2rust/orca/*.{ts,rs,seed.jsonl}`.
- Session arc: **54 → 101 → 175 → 230 → 243 (100%) → 258 → 273 → 293** (+50 fresh real-orc kernels past
  100%). The grow-corpus loop's honest SKIP pattern: functions needing a runtime object (`new URL()` +
  IDNA/Punycode) are correctly declined, not forced — ~5 SKIPs so far, all URL-runtime.

### The invariant you must never break
`soundgate.sh` (in the prev session's scratchpad; re-derive if gone): the 6 controls MUST be
NOT-TRUSTED, and three panic-probes (`s[i]` OOB, `xs.iter().sum()` overflow, `a/b` div-by-zero) MUST
each keep >0 unproven obligations. Run it after ANY trustc rebuild or allowlist change BEFORE trusting
a census number. A control going TRUSTED or a probe going fully-proven = an unsound verifier; STOP.

---

## What was built (mechanisms, not just results)

Four **toolchain fixes** in `~/trust` (each dissolved a whole obligation class — the high-leverage move):
1. **Trait-tail callee matching** (`crates/trust-ir-bridge/src/lower.rs`, `TRUSTED_PANIC_FREE_TRAIT_TAILS`
   ~line 13371): `strip_generics` deleted the `<Recv as Trait>` qualifier, so trait-dispatched spellings
   (`<Chars as Iterator>::map`, `<String as PartialEq<str>>::ne`, `ToString::to_string`, `From::from`,
   `Deref::deref`) collapsed to a bare method name and never matched the flat allowlist. SOUND additions
   only: lazy adapters + alloc-conversions + Deref/PartialEq. DELIBERATELY EXCLUDED (would be unsound —
   drops the Undef-result cast obligation): eager consumers `all`/`any`/`find`/`collect`/`fold`,
   `count`/`sum`/`product`, `Index::index`.
2. **Struct-argspec derivation** (`orc tools/terminal-bench/rust-type-to-argspec.mjs`, extracted from
   gauntlet.mjs for max-lines): map struct field TYPES (String→str, bool→bool, Option<T>→`inner?`) and
   camelCase keys under `#[serde(rename_all="camelCase")]`. Fixes serde "invalid type"/"missing field".
3. **`verify.mjs` maxBuffer** 1 MiB → 512 MiB (large-output kernels ENOBUFS'd into false W2 errors).
4. **Fuzzer optional omit-key** (`fuzz.mjs`): a `T?` struct field OMITS the key ~1/3 of samples
   (TS `undefined` = serde `None`; a JSON `null` spuriously diverges since `null!==undefined` in TS).

**F2 first rung — per-kernel seed corpora** (`verify.mjs` appends `<base>.seed.jsonl` if present, purely
additive): lets the differential exercise domain-specific true-branches the generic STR/number fuzzer
can't reach (credential URLs, magic-code bands, shell metachars, CSI escape sequences). Without it, W2
degenerates to a HOLLOW identity==identity — indistinguishable from a no-op port. This is how the git
**credential scrubber**, the POSIX/PowerShell **shell-injection quoters**, and the **CSI reply parsers**
were proven. When a kernel's trigger inputs aren't in the generic pool, author a seed file; do NOT
claim TRUSTED on a hollow W2.

**Workflows** (in the prev session's scratchpad — re-create from these descriptions):
- `reform-v2.js` — diagnostic-first reformulation: each agent runs the driver, reads the EXACT W1
  obligation, applies the matching behavior-preserving pattern (byte-loop / `.get()` / widen-saturate /
  proof-simplify / uninhabited-enum / derive-strip). Used to clear the W1 residue.
- `grow-corpus.js` — the factory manual loop: each agent takes a real orc `file::fn`, writes a
  self-contained `.ts` oracle + a faithful+provable `.rs` port + (if needed) a `.seed.jsonl`, and drives
  to TRUSTED. SKIPs honestly if the fn needs a runtime object (`new URL()`+IDNA) — never forces it.

---

## Deferred / known gaps (honest, not faked)

1. **`hasCompatibleAgentTitleIdentity`** — W2-faithful (0/76 over 51 seeds, pre-validated on 1.2M
   strings) but W1-INCOMPLETE: 1 `runtime-checked` obligation on its heavy title-regex cascade. A
   near-miss; the port is correct, the verifier couldn't discharge one obligation. Files were removed
   (uncommitted). Retry with a reform pass on that one obligation, or after the toolchain gap below.
2. **`encodePowerShellCommand` (base64)** — W1 INCOMPLETE with 4 `unsupported MIR
   FullVerification::ArithmeticSafety: shift overflow (Shr)` unknowns (base64's bit-shifts), PLUS a W2
   build error. Files removed. **This is a real T-D toolchain gap**: right-shift-overflow modeling.
   **SCOPED 2026-07-18**: the ShiftOverflow VC is EMITTED in the MAIN TREE at
   `crates/trust-ir-bridge/src/lower.rs:7611` (`reconstruct_assert_vc_kind`, the `Overflow(Shl|Shr)` arm →
   `VcKind::ShiftOverflow{op, operand_ty, shift_ty}`) from rustc's inserted `assert(shift_amount <
   bit_width)`; the native trust-mc lane then times out trying to discharge it. Candidate fix (rebuild-
   able without the submodule): in that arm, read the `amount` operand — if it is a MIR CONSTANT and its
   value < the `value` type's bit-width, the shift is trivially overflow-free, so return `None`/skip the
   VC (mirrors how a constant divisor discharges div-by-zero). CAVEAT before doing it: confirm base64's
   shifts reach here as MIR constants (rustc may already const-fold the fully-constant case via
   `known_panics_lint.rs:350`, in which case the timeout is on a SYMBOLIC shift and this fix won't help —
   inspect the actual port's MIR first). Needs a trustc change + ~45min rebuild + soundgate + census.
   Unlocks the bit-manipulation class (base64/hashing/packing) IF the shifts are const.
3. **ICE fix (uncommitted)** — `~/trust/first-party/trust-mc/.../direct_smt_cex.rs:~364`
   `SmtValue::Int(i)` → `i.to_string()` (i128 bignum witnesses trip serde_json "number out of range").
   Applied in the trust-mc SUBMODULE working tree but NOT committed: the submodule is in a tangled
   three-way SHA state across parallel sessions (HEAD ≠ origin/main ≠ parent gitlink). Doesn't change any
   census verdict (an ICE'd control still reports NOT-TRUSTED). Commit it when the submodule state is
   untangled + you can rebuild.

---

## Next steps (prioritized)

**Non-gated, agent-executable (pick these to keep driving):**
- **T-D: fix the `Shr` shift-overflow modeling gap** in trustc → unlocks base64/hashing/packing kernels
  (recover `encodePowerShellCommand` + the whole bit-manipulation class). Highest-leverage toolchain fix.
- **Keep growing the corpus** on high-value classes via `grow-corpus` — many pure orc fns remain
  (scout: `grep -rE "^export function [a-z]\w*\(...\): (string|number|boolean)" src/shared mobile/src`,
  filter against already-ported fn names). Favor security/parsing/validation; use seeds for domain logic.
- **F2 next rung: trace-DERIVED corpora** (beyond hand-authored seeds) — `tools/parity-corpus-metrics.mjs`
  exists (F2 half-done). Wire real traces as W2 corpora.
- **F3 swc/oxc front-end** (XL) — auto-parse real orc TS instead of hand-authoring `.ts` oracles. The
  scale-up that makes the factory a factory rather than a manual loop.
- **Promotion** (the perf/safety payoff) — a proven kernel on a genuinely hot path → napi/wasm shadow →
  cutover → delete the TS twin (orca-git did this). Most corpus kernels aren't hot; pick a real one.

**Gated on Andrew's decision (surface, don't guess):**
- ay/ty publication (the public one-command re-check / paper artifact) — release machinery exists.
- The orc-electron fork go-ahead.

---

## Gotchas
- **zsh evaluates backticks in `git commit -m "..."`** even inside double-quotes → use a heredoc
  `git commit -F /dev/stdin <<'MSG' ... MSG` for any message with backticks.
- **`pgrep -f "x.py build"` self-matches** helper scripts containing that string → false readings. Match
  the real process: `ps -Ao pid,command | awk '/[p]ython3?.*x\.py build/ && !/pgrep|bash -c|sleep/'`.
- **A control char (`\t`, `\x1b`) in a Bash command** trips the harness control-char guard → write such
  seed files via a `node -e` script with JSON escapes, using plain ASCII in the command itself.
- **macOS has no `timeout`**; heavy kernels blow past a 2-min bash cap → run the driver detached (nohup)
  and poll for completion. The driver itself caps W1 at ~180s (a timeout → NOT-TRUSTED).
- **Shared-tree push**: both `~/orc` and `~/trust` have parallel-session writers. Always
  `git fetch` then `git rebase --autostash origin/main` on a non-fast-forward, then push.
- **`build-terminal-addon` skips Rust changes** unless `--force`; always prove the field flows end-to-end.
- Commit corpus changes with explicit paths (`git add tools/ts2rust/orca/ tools/ts2rust/verify.mjs`) to
  avoid sweeping in the dirty `first-party/*` submodule pointers.
