# Orca Fork — Staging Launch Readiness Report

**Audited ref:** findings verified against HEAD `d53344be1` (v1.4.122-rc.3, 2026-07-04). All finding IDs re-checked against current code; statuses below reflect HEAD, not the audit snapshot.

## 1. Verdict

**NOT READY for staging.** The engineering core is genuinely strong — and got stronger since the audit snapshot: the 142-commit upstream re-merge landed (`1bf2a5543`), restoring strict superset at the audit ref (`git rev-list --count HEAD..refs/upstream-audit/main` = 0; fork is 356 commits ahead, 0 behind). The Rust daemon is real (34/34 tests, destub suite proves live probes and real exit codes), the aterm single-stack claim holds (zero `@xterm` packages vs upstream's 8), and three loudly-alleged security/parity gaps were refuted under adversarial verification. What is not ready is the *ship vehicle*: the staging build still wears public Orca's exact identity (appId, userData, update feed, daemon protocol number, crash/feedback endpoints), so the first accepted update prompt replaces the fork with the public build — and after that silent downgrade, the public app adopts the fork's Rust daemon and inherits its regressions invisibly. Add zero CI, zero telemetry reaching the fork team, a mac packaging path that cannot produce a correct Intel artifact, and a daemon that still corrupts split multibyte output, and staging as-is would self-destruct while telling you nothing about why.

## 2. Launch blockers (must fix before staging, in order)

1. **F1 (critical) — Updater points at the public feed; one accepted update erases the fork.** `src/main/updater.ts:838,1201` and `src/main/updater-prerelease-feed.ts:5-6` hardcode `github.com/stablyai/orca`. The fork now versions itself `1.4.122-rc.3` on upstream's own scheme, so public stable ≥ 1.4.122 outranks it — this fires, it is not hypothetical. Fix: repoint both URLs plus the publish owner/repo at a fork release location, or hard-disable the updater behind a build-time flag for staging; adopt `1.4.122-fork.N` versioning so public stable never wins a comparison.
2. **F13 (high) — Windows updater signature verification is stubbed to accept anything.** `src/main/updater.ts:1187`: `verifyUpdateCodeSignature = () => Promise.resolve(null)`. Combined with F1, a Windows staging install will install whatever the public feed serves, unverified. Fix with F1; remove the bypass once a fork publisher identity exists; document the unsigned/SmartScreen posture for the staging cohort.
3. **F14 + G3-0/G3-1/G3-3/G3-5 (high cluster) — Fork shares public Orca's identity AND daemon namespace.** `config/electron-builder.config.cjs:101-102` still `com.stablyai.orca` / `Orca` (shared userData, shared single-instance lock — staging launches just focus the public app). Worse: `rust/crates/orca-daemon/src/protocol.rs:12` and `src/main/daemon/types.ts:10` both pin `PROTOCOL_VERSION = 18`, identical to the public Node daemon — so after any downgrade/rollback, public Orca silently adopts the fork's Rust daemon (inheriting F4/F5/G2-2 inside the public app), and its client (`src/main/daemon/client.ts`) drops hello-coalesced stream events against it (the fork fixed its own side in `1bf4822bd`; the public side is unfixable — isolation is the only remedy). Fix: staging appId `com.stablyai.orca.staging`, distinct productName/AUMID/userData; bump the fork daemon protocol to a fork-reserved number in both files and add 18 to `PREVIOUS_DAEMON_PROTOCOL_VERSIONS` so live public Node daemons migrate via the existing legacy-adapter path (socket/token/pid names key off the version, so endpoints separate automatically).
4. **F2 (critical, narrowed at HEAD) — No correct multi-arch mac path.** The config now defaults to host-arch-only (`config/electron-builder.config.cjs:15-24`), which prevents the silent-broken-x64-DMG case but means Intel staging artifacts can only come from an Intel machine — and the `ORCA_MAC_BUILD_ARCHES` dual-arch override still packages the host-arch `rust/target/release/orca-daemon` and `native/orca-node/orca_node.node` (lines 47-59) into both arches. Fix: per-target cargo builds (`--target x86_64-apple-darwin`/`aarch64-apple-darwin`) + lipo or per-arch extraResources, plus an afterPack assertion that `lipo -archs` matches the bundle arch. Alternatively declare staging arm64-only in the launch notes.
5. **F5 (high) — Daemon output pump corrupts split multibyte UTF-8.** `rust/crates/orca-daemon/src/rpc.rs:386` still `String::from_utf8_lossy` per read; CJK/emoji split across PTY reads render U+FFFD live and are baked permanently into `takePendingOutput` checkpoint records. Fix: extract `connection.rs`'s `decode_streaming` into a shared module and use a per-session carry buffer in `pump_output`.
6. **F4 (high, narrowed at HEAD) — Shell-launch layer still incomplete in the Rust daemon.** Progress since audit: `build_command` (rpc.rs:307+) now honors `env`/`envToDelete` end-to-end and runs command sessions under `$SHELL -lc`; `shellState` honestly reports `unsupported`. Still open: `shellOverride` is forwarded by the adapter (`src/main/daemon/daemon-pty-adapter.ts:206`) but ignored — the shell picker silently does nothing on macOS/Linux; plain sessions spawn non-login (no `.zprofile`/brew PATH); no shell-ready barrier or pre-ready stdin queue; no ZDOTDIR/rc attribution wrappers. Port the remainder from `local-pty`.
7. **F8 (high) — Daemon failure is invisible.** `src/main/daemon/degraded-daemon-pty-provider.ts:19` still sets `isDegraded = true` and the only renderer consumer discards it; total daemon launch failure is console.error-only. The headline feature can turn off silently — exactly the class the launch bar forbids. Fix: IPC-expose daemon/degraded status, sticky toast mirroring the `App.tsx` "Session restore failed" pattern, status indicator + retry, per `docs/daemon-staleness-ux.md`.
8. **G2-1 + G2-0 + G2-3 (high/medium) — Scrollback-at-rest lifecycle.** `src/main/daemon/history-manager.ts` writes checkpoint/output/meta at umask-default perms (no 0o600/0o700), deletes only on explicit kill of a live session, and has no GC/TTL/size cap — secret-bearing scrollback outlives session, pane, and worktree indefinitely. Fix: mode-0o700 dirs / 0o600 files + one-time chmod sweep; delete on clean exit; sweep on worktree removal via the `${worktreeId}@@` prefix contract; startup/periodic age+size GC. Note G2-4 while there: move daemon sessions out of the shared `terminal-history` root before adding fields to meta.json.
9. **G0-0/G0-1/G0-2/G0-3/G0-4 (high cluster) — Staging produces zero observability, and what leaks goes to the wrong company.** No build script sets `ORCA_POSTHOG_WRITE_KEY`/`ORCA_BUILD_IDENTITY` (`package.json:74-78`), so telemetry compiles out; `config/scripts/verify-telemetry-constants.mjs` exists but is wired into no build; crash/feedback reports POST to `onorca.dev` (`src/main/ipc/feedback.ts:9-10`) — the public vendor's inbox. Fix: fork-owned PostHog project keyed at the `build:desktop` leg with the verifier wired in; never use the public write key (G0-3); repoint or fail-closed the feedback endpoints; add the small daemon/GPU event family (`daemon_launch_failed`, `daemon_degraded_fallback`, `terminal_gpu_downgrade`, `renderer_process_gone`) since the existing taxonomy is 100% upstream product-funnel; re-own `PRIVACY_URL` (`src/renderer/src/lib/telemetry.ts:28`) and the default-opt-in copy before keying.
10. **F15 (high) — No CI exists at all** (`.github/workflows` absent). ~1400 commits in 3 weeks with zero automated gates. Minimum before staging: typecheck + vitest + default e2e + `parity:daemon` both legs + `cargo check/test` + gauntlet on PRs; a Windows lane; a per-platform release job carrying the F2 arch assertion; bench JSONs as trend artifacts. Fold in **F16**: make worker-ON the e2e default (`tests/e2e/helpers/orca-app.ts:244-247` currently forces the in-process fallback suite-wide, so the shipped path gets the thin coverage and perf e2e measures the wrong path).
11. **F3 (critical, decision) — atpkg is entirely unconsumed.** No dependency edge, shell-out, packaged binary, doc, or UI anywhere in `package.json`/`config/`/`src/`. Decide now: drop "uses the aterm package manager" from all launch claims (zero code cost), or do the minimum honest integration (section 5). Shipping the claim as-is fails any audit.
12. **F37 (medium) — Unix packaging deterministically fails on rustup stable < 1.96, and all Rust gates currently pass only under `+nightly`.** No `rust/rust-toolchain.toml`. Fix: pin a concrete toolchain so rustup auto-installs it; add an up-front rustc-version preflight to `config/scripts/build-rust-daemon.mjs` with an accurate `rustup update stable` message.
13. **F39 (medium, decision) — No automated Gatekeeper-passing mac path.** Plain `build:mac` DMGs arrive quarantined for teammates. Decide: run `build:mac:release` with Developer ID credentials on a designated machine, or document the `xattr` workaround for the cohort; run `verify-macos-entitlements.mjs` pre-release either way.

## 3. The superset gap — now substantially closed; keep it closed

**Status change since audit:** merge `1bf2a5543` ("re-align with stablyai/orca — 142 commits, v1.4.122-rc.3, aterm work preserved") landed. Verified at HEAD: **F12 closed** (0 commits behind `refs/upstream-audit/main`); **F6 closed** (`533cafdfa` in — transcript cache keyed by resolved file path, confirmed in `src/main/native-chat/transcript-read-cache.ts:48`); **F11 closed** (all five Windows perf commits `2f3c7e866 d5c4372b3 9964d1ccb 10e8f8689 0ea354d45` are ancestors, plus ConPTY fixes like `c6bfd86d4`); **F33 closed** (`20201f3ae fa94065a8 4caea00f1 77a223a7e dc1c1e9fd` all in); **F35 closed** (`2c0313397 2848c5daf` in). The 142 commits grouped: terminal/daemon reliability (recovery-reload PTY sweep, cold-restore skip, hibernation wake, TCC login attribution, theme persistence, exported-handle recovery, detach placement), Windows perf/ConPTY, security (`0d8205ae7` CWD symlink validation), cross-worktree data integrity, i18n/IME (zh/es refinements, CJK IME Enter, Korean mobile mirror), SSH/GitLab fixes, startup perf, serve-mode automations.

**What remains:**
- **The ritual, not a backlog.** Upstream moves daily and F1's version-scheme collision compounds the risk. Establish a per-upstream-stable-release merge cadence. F47 confirms merges stay cheap: the dependency diff is *exactly* the xterm removal — no holdbacks, no new packages needed.
- **xterm-renderer-specific commits triaged as moot-for-aterm** need aterm-equivalence verification with parity tests where behavior differs (the unfinished tail of F33's remit).
- **G3-4 watch item:** `src/shared/workspace-session-schema.ts` is byte-identical to upstream today (clean round-trip), but its whole-partition zod fallback makes future skew total, not graceful — sync it on every staging cut, or (better) let the F14 userData fork remove the alternating-build exposure entirely.

## 4. The wiring gap — implemented but dark (ordered by value/effort)

1. **F8 — `isDegraded` flag discarded by its only consumer.** Blocker; see section 2.7.
2. **F26 — `reconcileOnStartup` has no production caller** (`src/main/daemon/daemon-pty-adapter.ts:457`; nothing in `daemon-init.ts` calls it). Sessions for worktrees deleted outside Orca leak forever. One wire: call it after workspace hydration, seeded with live ids + `WorktreeMeta.priorWorktreeIds`. Effort S, directly serves the memory claim.
3. **F43 — OSC 133 parsed twice**: TS re-scans every PTY chunk (`src/renderer/src/components/terminal-pane/terminal-command-lifecycle.ts:68`) while the engine's decoded OSC 133 events are drained and discarded. Move lifecycle onto `registerOscHandler(133, …)`, delete the raw scanner. Effort S; removes a hot-path cost and a dual-truth-source hazard.
4. **F30 — Per-pane renderer diagnostics are test-only.** Surface "Renderer: GPU (ANGLE Metal)" + `decideAtermGpu().reason` in the Terminal Engine pane; without it F22's silent Wayland CPU downgrade is undebuggable in the field.
5. **F36 — No automated wasm-to-submodule sync check**, and the engine moved v0.16 → v0.18 → v0.21 → `987d53d` in two days on manual convention alone. Have `build-aterm-wasm.mjs` write a committed `{atermCommit, sha256}` manifest; assert engine-rev equality between wasm glue and native addon at startup and in a unit test.
6. **F27 — `pnpm dev` never builds the Rust daemon** (`package.json:41` builds only the terminal addon), so dev silently runs `LocalPtyProvider` with `degraded=false` on fresh clones. Make dev build or mtime-verify the daemon and warn loudly.
7. **F19 — `orca serve` bypasses the daemon by design** (`src/main/index.ts:610-611`), so terminals die on serve restart on exactly the SSH/headless hosts where persistence matters most. Wire a `DaemonPtyAdapter` in serve mode (the binary already ships in the linux package), or document the limitation in launch notes.
8. **F29 — Scenes ship dark**: framework compiled into both wasm bundles, zero consumers, control-less placeholder row in `TerminalEngineEffectsSection.tsx:222-238` (copy is now honest, which mitigates). Ship one scene, or drop the row behind `scene_names().length > 0` and strip `aterm-scene` from the wasm builds.
9. **F48 — Eight Rust workspace crates fully unwired** (~14k lines; `orca-text`/`orca-agents` compiled into `orca_node.node` but unreachable). No launch action needed if claims stay scoped to "terminal daemon + git parsers" — just keep the narrative scoped.

## 5. The leverage roadmap — atpkg + introspection (ordered by value)

1. **F18 — Wire the observation kernel to agent-completion detection.** Today: hook pushes + title-emoji heuristics + 750ms–3s process-table polling (`src/renderer/src/components/terminal-pane/agent-completion-coordinator.ts:39`), worst on the SSH relay path. Add a daemon RPC `watch`/`awaitIdle` verb backed by `aterm_core` WatcherSet + `aterm-observe` prompt-ready matchers on each session's headless engine; demote polling to fallback. This is the single highest-value introspection wire: it converts the fork's flagship "agents in terminals" loop from heuristic to authoritative and kills continuous per-pane polling cost.
2. **F40 — Adopt `aterm-shell-integration`** (`rust/aterm/crates/aterm-shell-integration/src/lib.rs`) to replace the three already-drifting hand-rolled TS wrappers across local/daemon/relay. Immediate wins: fish gets OSC 133 (long-command notifications currently silently dead for fish users), and OSC 7 gives engine-authoritative cwd via the already-wired `take_osc_events` path, retiring process-table cwd fallbacks. Pairs naturally with finishing F4.
3. **F41 — "Contained agent" mode from `aterm-containment` + `aterm-sandbox`.** macOS sandbox-exec SBPL wrap + Unix rlimits are proven in aterm's own spawn seam and consumed nowhere; a per-worktree/per-agent toggle (no network / no `~/.ssh` / rlimits) is a genuine safety differentiator no competitor ships. Cost M-L; the hard parts already exist in-tree.
4. **F46 — `aterm-ctl`-grade read verbs on the daemon socket** (text/screen/cursor/search + F18's watch) under the existing uid/token discipline, plus a thin `orca-ctl`. Gives agents the read/await/drive channel without routing through the full Electron runtime RPC. If not on the roadmap, scope it out explicitly in launch notes.
5. **F3(b) — Minimum honest atpkg integration** (if you keep the claim rather than dropping it): ship `atpkg` as a macOS/Linux extraResource, pin an orca-owned root key, publish one real signed package orca actually consumes — the natural first candidates are the F40 shell-integration script bundles or theme/scene packs (which would also give F29 its consumer). Anything less: drop the claim (section 2.11).
6. **F44 — Marks-based navigation** (prompt jump, command-scoped copy, scrollbar mark overlay) — the engine already decodes and surfaces marks; competing terminals ship this as a headline. Post-launch.
7. **F45 — vi-mode**: fully implemented in `aterm-vi` (unconditional dep, public Terminal API), zero wiring. Expose enter/exit + key-feed from `aterm-wasm` and route a shortcut (Mac/other modifier rule) — or strip the crate from the build. Cost M.

## 6. The performance proof plan

**Current evidence state (F17):** the only re-runnable head-to-head is engine parse throughput with recorded numbers that are now many engine drops stale (the engine advanced to v0.21+`987d53d` since capture); the renderer keystroke xterm baseline was deleted and cannot be re-run. A skeptical reviewer can correctly say no verifiable number supports any headline metric today.

**The four proof points (F42)** — run against a packaged public Orca on the same macOS + Windows machines, results JSONs committed from at least two machines, wired into F15's CI as trend artifacts:
1. Cold/warm startup + time-to-first-prompt, median of 5 (`tools/benchmarks/startup-time-bench.mjs` exists on both refs).
2. Keydown-to-paint p50/p95, idle and under 9-pane replay load.
3. Summed multi-process RSS + wasm heap after 20 terminals x 5000 filled rows — this also tests the hidden-flood 2MB bound end-to-end.
4. Wall time to drain + paint the 16MB terminal-bench corpus.

**Additional measurements the claims depend on:** multi-pane flood p95 with 4 visible panes on the single shared render worker (F25 — worst-case latency is currently the *sum* of all panes' parse work, unmeasured); daemon RSS at 20/50/150 preserved sessions (F24 — scrollback is duplicated across up to 3 engines per attached terminal and the daemon count is unbounded until F26/G2-3 land).

**Known ceilings to fix or disclose before claiming:** F23 — daemon-backed bytes are VT-parsed 3x with a synchronous per-chunk main-thread pipeline (short-term: drop the main-process HeadlessEmulator on Unix where the Rust daemon owns authoritative state); F22 — Wayland defaults to the CPU path via an inherited xterm-era gate aterm's own GPU probe contradicts (re-test upstream #5319 against aterm, narrow to the driver blocklist); F10 — Windows has no Rust daemon (compile-time stub, `rust/crates/orca-daemon/src/lib.rs:92`), so all perf claims must say "macOS/Linux"; F50 — the shared worker's wasm heap holds its high-water mark by design (report it in `orca diagnostics memory` so post-activity comparisons don't read as a leak); F49 — gate the unconditional per-pane a11y mirror DOM on screen-reader presence.

## 7. UX gaps blocking "superior"

- **F8** — invisible daemon failure (blocker; section 2.7).
- **F9** — remote web client renders CJK/emoji as tofu: no `window.api.fonts` in `src/renderer/src/web/web-preload-api.ts` (verified still absent) and no runtime-RPC fonts endpoint, so `loadSharedWorkerFonts` starves. A visible regression vs the public web client on the SSH-adjacent flow AGENTS.md protects.
- **F21** — SSH relay reattach lacks the source-dims correction the daemon snapshot path got in `fccf9eaae`; resize-then-reconnect permanently truncates restored scrollback (`pty-connection.ts:4961`). Carry cols/rows through the relay attach result.
- **G2-2** — Rust daemon says `"unknown session"` (rpc.rs:55,72,81) but the matcher tests `/Session not found/i` (`src/main/ipc/pty.ts:424`): kill-after-exit surfaces renderer-visible rejections and `stopAndWait` can fail spuriously on macOS/Linux. One-string fix + a parity-corpus case so it can't drift again.
- **F31** — "Don't ask again" on close-confirmation is a one-way door: the design doc's settings switch was never built (no `skipCloseTerminalWithRunningProcessConfirm` row in any settings component) and settings-search points at the nonexistent row.
- **F32** — zero feature education for the terminal-engine capabilities that justify the fork; one feature-interaction id + a restart tip for the Terminal Engine pane closes it.
- **F51** — 85 settings-search keys + 4 visible keys missing from zh/ko/ja/es catalogs; affected entries unfindable via localized search.
- **F30 / F29** — renderer-state opacity and the scenes placeholder row (section 4).

## 8. Staging launch checklist (mechanical, in order)

1. **Fork the identity**: staging `appId`/`productName`/AUMID/userData behind a toggle in `config/electron-builder.config.cjs` (+ `src/main/startup/dev-instance-identity.ts`). [F14, G3-5]
2. **Fork the daemon namespace**: bump `PROTOCOL_VERSION` in `rust/crates/orca-daemon/src/protocol.rs:12` and `src/main/daemon/types.ts:10`; add 18 to `PREVIOUS_DAEMON_PROTOCOL_VERSIONS`. [G3-0/1/2/3]
3. **Neutralize the updater**: repoint `updater.ts:838,1201` + `updater-prerelease-feed.ts:5-6` at fork releases or hard-disable for staging; switch to `-fork.N` versioning; remove the `verifyUpdateCodeSignature` bypass (`updater.ts:1187`) or keep the updater off. [F1, F13]
4. **Own the data plane**: fork PostHog project; inject `ORCA_POSTHOG_WRITE_KEY` + `ORCA_BUILD_IDENTITY=rc` at the `build:desktop` leg and wire `config/scripts/verify-telemetry-constants.mjs` as a release gate; add the daemon/GPU event family; repoint or fail-closed `FEEDBACK_API_URL`/fallback; re-own `PRIVACY_URL` and the opt-in copy. Never build with the public write key. [G0-0..4, F47]
5. **Pin the toolchain**: `rust/rust-toolchain.toml` (≥1.96) + rustc preflight in `build-rust-daemon.mjs`/`build-aterm-wasm.mjs`/`run-parity.mjs`. [F37]
6. **Land the daemon fixes**: F5 streaming decode in `pump_output`; F4 remainder (shellOverride, `-l` plain sessions, shell-ready scanner, rc wrappers); G2-2 error string; F20 SIGTERM+5s kill ladder outside the registry lock + `shutdown(killSessions)`; call `reconcileOnStartup` (F26).
7. **Harden scrollback at rest**: 0o700/0o600 + chmod sweep, exit/worktree deletion wires, GC with size cap; split the daemon store out of the shared `terminal-history` root. [G2-0/1/3/4]
8. **Fix packaging**: per-arch daemon/addon or declared arm64-only scope + afterPack `lipo -archs` assertion (F2); add `orca-daemon` to `THIRD-PARTY-NOTICES.md` regeneration (F38); decide the mac signing story and run `verify-macos-entitlements.mjs` (F39); document the Windows unsigned posture (F13).
9. **Stand up CI**: typecheck + vitest + e2e (worker-ON default per F16) + `parity:daemon` both legs + cargo check/test + gauntlet on PRs; Windows lane; per-platform release job with the arch assertion; wasm-manifest check (F36); bench trend artifacts. [F15]
10. **Honest launch notes**: drop or qualify the atpkg claim (F3); platform-qualify perf claims to macOS/Linux (F10); document serve-mode persistence limitation if F19 isn't wired; state that raw-output persistence is inherited upstream behavior with hardening applied this release (G2-5).
11. **Run the proof points** (section 6) against a packaged public build and commit the JSONs. [F42, F17]
12. **Keep the merge ritual**: sync per upstream stable release; sync `workspace-session-schema.ts` on every cut until userData is forked. [F12 follow-through, G3-4]

## 9. What was checked and held up

**Positive findings (don't re-audit these):**
- **Rust gates all pass** (under `+nightly`; see F37): `cargo check --workspace` clean across 18 crates + aterm path-deps; orca-daemon 34/34 including the destub suite (`pty_spawn_health_runs_a_real_probe`, `exit_event_carries_the_real_child_code` — direct evidence for "no stubs" at the daemon layer); orca-terminal's 26 unit + 11 `aterm_parity.rs` tests green; clippy effectively clean (2 warnings workspace-wide). rustfmt drift (2103 hunks) recorded as F52, not a gate failure.
- **Vendored wasm is fresh**: both bundles embed exactly the pinned submodule version; regeneration ships in the same commit as each submodule bump (convention held through v0.18 → v0.21 → `987d53d`; F36 asks to make it a gate).
- **Daemon protocol integrity**: real token auth (/dev/urandom, 0600, correct hello ordering), all 18 request types dispatched, `takePendingOutput` mirrors the Node contract (2MB cap, overflow, seq, snapshot-supersedes-records atomicity), `env`/`envToDelete` honored end-to-end into the PTY CommandBuilder. No `todo!`/`unimplemented!` markers in orca-daemon/orca-pty/orca-session.
- **Single-stack claim verified**: zero `@xterm` packages anywhere (upstream ships 8); nothing in production disables the worker render path; e2e config exposure is bake-time only; search works on all three render paths; effects/kitty/OSC-notification wiring traced to real user flows.
- **Upstream merge verified landed**: F6, F11, F33, F35 spot-checked commit-by-commit as ancestors of HEAD; fork-side hello-coalescing client bug fixed in `1bf4822bd`.
- **Dependency hygiene (F47)**: fork-vs-upstream dep diff is exactly the xterm removal.

**Claims refuted under adversarial verification (raised, checked, killed — don't burn time on them):**
1. "Missing CWD symlink-escape validation (#7334)" — false; files byte-identical to upstream, and `0d8205ae7` is in HEAD.
2. "Mobile terminal is CDN-loaded xterm.js beta" — false; the WebView bundles vendored `XTERM_ENGINE_CSS/JS`, no CDN, no offline break.
3. "Missing GitLab/SSH compatibility fixes" — false; all five cited commits landed with the upstream sync.
4. "Runtime-RPC terminals exposed without auth" — overstated; terminal/exec verbs require a paired device token (G1-0's residual is the 0.0.0.0 default bind — worth a loopback default, not a vuln). G1-1/G1-2 (TOCTOU chmod, non-constant-time compares) are defense-in-depth items only, safe today via 0o600 token files.

---

## Addendum — 2026-07-04 evening: blocker remediation status

All launch blockers from this report are now **closed** across two fronts (verify with `git log 92c07a2e9..HEAD` and the commits below). The verdict moves from *not-ready* to **ready-pending-operational-setup**.

**Closed by origin remediations** (merged in `b1f754c2f`):
- F5 — daemon UTF-8 boundary carry (`utf8_stream_decoder.rs`), including checkpoint records.
- F4a — `shellOverride` honored in `rpc.rs` (validated, `$SHELL` fallback).
- F8 (first half) — degradation surfaced to Manage Sessions; dead launcher/provider deleted.
- Kill/shutdown wire parity; aterm-pin drift guard (`check:aterm-pin`, now in the lint gate).

**Closed by this session** (commits `92c07a2e9`, `19fce9bfb`, `2a15068f6`, `5afebe287`):
- G3 — protocol namespace split (18 → 1018, legacy migration path for 18, disjoint socket/token/pid endpoints).
- F4b/c/d — login shells, shell-ready barrier + pre-ready stdin queue, non-ports documented in `docs/rust-migration/daemon-shell-launch.md`.
- G2-0..4 — at-rest perms (0o700/0o600 + tighten sweep), daemon-owned store subdir + migration, clean-exit deletion with cold-restore survival, worktree `@@`-prefix sweep, age/size GC.
- F8 (second half) — daemon-status registry over IPC, sticky retry toast, status-bar indicator, localized (es/ja/ko/zh parity).
- G0-0..4 — fail-closed feedback endpoint (never onorca.dev), staging telemetry-key preflight in the build legs, fork privacy doc, reliability event family (2 of 4 emitting; daemon hook points documented in `docs/reference/staging-observability.md`).
- F1 — ALab-owned public update feed (`alabsystems/orca-alab`), separate from development source at `andrewyatesai/orca-alab`, dormant-if-unconfigured, version `1.4.122-fork.1`; macOS discovers releases but installs them manually because ALab has no Developer ID signature (`docs/reference/fork-versioning.md`).
- F13 — Windows release discovery stays available, but installation is manual until ALab has a trusted publisher identity.
- F14 — default identity `com.stablyai.orca.staging` / "Orca ALab Edition" (userData/lock/AUMID isolated; `ORCA_PUBLIC_IDENTITY=1` escape hatch).
- F2 — per-target cargo + lipo mac multi-arch under `ORCA_MAC_BUILD_ARCHES`; afterPack bundled-binary arch assertion.
- F15/F16 — the audit's CI findings were closed by the historical fork
  workflows, including the aterm-worker-ON e2e lane. Those workflows are not
  tracked in the current development snapshot and do not authorize publishing.
- The #7192 mirror-ordering test adapted to aterm widen-reflow (stale xterm-era expectation; ordering invariant intact).

**Operational setup still required before the first staging build** (no code):
1. Provision the fork PostHog project; set `ORCA_POSTHOG_WRITE_KEY` (and `ORCA_FEEDBACK_ENDPOINT` when a feedback inbox exists) as CI secrets — or export `ORCA_ALLOW_NO_TELEMETRY=1` for a deliberately dark build.
2. Review and provision a separate maintainer release procedure before
   publishing; the development repository currently tracks no release
   workflows.

The former `ATERM_SUBMODULE_TOKEN` requirement was removed when the submodule
moved to public `alabsystems/aterm`; CI checkout must remain anonymous.

**Known accepted residuals** (tracked, non-blocking): Linux deb/rpm keep the `orca-ide` package/executable name (a staging deb replaces an installed public deb; userData still isolated); semver orders `-fork.N` below public releases — safety rests on feed repointing + dormancy, not version comparison; e2e local default remains worker-OFF, and the current development snapshot tracks no CI workflow that supplies the worker-ON lane.
