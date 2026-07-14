# Upstream v1.4.139 merge — deferred Rust-backing ports

The `stablyai/orca` v1.4.139 merge is aligned at the TypeScript layer: `pnpm typecheck`
(TS7) and `pnpm lint` pass, terminal-keyboard-policy regressions are fixed, and the
new upstream features are adopted at the TS/test layer. ~170 tests still fail because
those upstream features need porting into the fork's Rust cores (the "integrate given
our rust pieces" work). Tracked here; to be landed incrementally.

## Feature-areas to port (by failing-test cluster)

1. **Orchestration store methods** (~26: `lifecycle-reconciliation`, `orchestration`, `db`,
   `coordinator`). TS shim (`src/main/runtime/orchestration/db.ts`) forwards new upstream
   calls (`convertLifecycleMessageToRejection`, `markAsReadAndDelivered`, `senderPaneKey`,
   dispatch creation) that the Rust `orca-store`/runtime doesn't implement yet — they throw
   "is not a function" at runtime. Port to the Rust store. (This is finding **F4**.)

2. **Agent CLI startup — Grok / Hermes** (~34: `git-wasm/tui-agent-startup`, `agent-tab-title`,
   `tui-agent-startup`). Upstream added Grok option-terminator + Hermes multiline/child-expansion
   startup handling; the fork cut agent-startup to the Rust `orca-agents` core, which lacks the
   new agents. Port the Grok/Hermes plan-builders to `orca-agents`.

3. **PTY flow-control / output-scheduler** (~44: `pty-connection`, `pane-terminal-output-scheduler`,
   `pty-transport`). Upstream's "terminal performance initiative" (pipeline fixes + PTY flow
   control + ack-credit) added tests the fork's aterm output path doesn't satisfy. Reconcile the
   flow-control/output-scheduler behavior against the fork's aterm write path (adopt ack-credit /
   delivery-interest where it fits aterm; drop xterm-write-path specifics that have no aterm analog).

4. **Workspace-cleanup classifier** (~16: `workspace-cleanup-removal-preflight`,
   `workspace-cleanup`, `workspace-cleanup-scan-progress`). Upstream added removal-preflight +
   classifier tiers the fork's Rust workspace-cleanup core doesn't emit (`'ready'` vs `'protected'`).
   Port to the Rust classifier.

5. **Daemon kitty-keyboard snapshot + serialize parity** (~12: `headless-emulator`,
   `reattach-snapshot`, `terminal-history-incremental-restore`, `terminal-replay-cursor-state`).
   The write-only daemon emulator needs `modes.kittyKeyboardFlags` tracking in
   `getModes`/`buildRehydrateSequences` for snapshot re-seed parity (finding **G4**), plus one
   alt-screen serialize case. Engine-independent byte scanner on the aterm-backed emulator.

6. **Git parse/normalize gaps** (~13: `remote`, `worktree`, `remove-worktree`, `worktree-list-paths`,
   `git-handler-utils`, `git-handler-worktree-ops`, `git-uncommitted-line-stats`).
   - **F3** worktree `locked`/`lockReason` parse in Rust `orca-git` — a LIVE removal-safety
     regression (the adopted `assertWorktreeUnlockedForRemoval` guard is inert without it).
   - **F5** push-hook-failure branch in Rust `orca-text` `normalize_git_error_message`.
   - **F1** `decode_git_cquoted_path` UTF-8 octal byte-run accumulation in `orca-core` (its test
     asserts the buggy `cafÃ©`; touches the SMT proofs in `orca-git/proofs/ay/decode_total`).

## Renderer capability-gap follow-ups (aterm-side)
- **G1** hard-wrapped HTTP-URL joining across wrapped rows (desktop click-fallback) — confirm/add
  to `aterm-link-input.ts`.
- **G2** PTY mouse-event suppression during a link click — no aterm analog; add.
- **G3** orphaned dead code after de-wiring: `terminal-http-link-activation.ts`(+test),
  `terminal-http-link-limits.ts` — rewire to aterm-link-input or delete.
- **G5** DECRQSS SGR/DECSCA answer defaults (aterm napi exposes no live SGR pen).

## Environmental (not code regressions)
- `daemon-init` (4) "Rust daemon failed to spawn" — needs `pnpm build:rust-daemon` in the test env.
- `run-electron-vite-dev` (1), `git-binary-compatibility-workflow` (1, no-CI ENOENT).

## Proof-carrying reconciliation (from F1)
- `rust/crates/orca-git/proofs/ay/decode_total/*.smt2` model the OLD per-codepoint
  octal arm and must be re-authored for the byte-accumulation + `from_utf8_lossy`
  decoder (the `catches_u8_truncation` obligation argued *for* the fixed bug).
  README marked SUPERSEDED. Re-derive with the Trust `ay` solver.
