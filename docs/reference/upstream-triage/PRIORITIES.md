# Upstream Porting Priorities — orc fork

**Scope:** 216 vetted candidates from 18 deep-dive agents (`upstream-triage/dives/*.json`), consolidated, deduped, and re-tiered.
**Baseline assumption:** the in-flight upstream merge (v1.4.147-era head) is committed first — several port paths depend on that settling. Upstream PRs that are already *approved/merged upstream* will arrive on the next routine merge; cherry-pick only what is **open, stalled, draft, or fork-specific**.

Links: `https://github.com/stablyai/orca/issues/<n>` or `/pull/<n>`.
Effort: S = hours, M = a day-ish, L = multi-day.

---

## 1. Recommended sequencing

**Wave 0 — data-loss and dead-input fixes that cherry-pick nearly clean (all S).**
Start with the seven small P0s in fully shared code: [#5880](https://github.com/stablyai/orca/pull/5880) (git discard wipes staged work — genuine data loss in a core action, also fix the relay twin), [#9158](https://github.com/stablyai/orca/issues/9158)/[#9492](https://github.com/stablyai/orca/issues/9492) (rich-markdown editor silently drops saves on non-ASCII content), [#7456](https://github.com/stablyai/orca/pull/7456) (daemon write routing self-heal — the fork's constant daemon-protocol bumps make this *more* exposed than upstream), [#8426](https://github.com/stablyai/orca/pull/8426) (one thrown write permanently kills PTY input), [#9342](https://github.com/stablyai/orca/pull/9342) (phantom workspaces minted from a sidebar sort snapshot), [#9263](https://github.com/stablyai/orca/pull/9263) (SSH drop misread as agent completion), and [#7948](https://github.com/stablyai/orca/pull/7948) (recursive `fs.watch` on `$HOME` freezes the whole SSH connection). Each is a small, test-carrying change in code the fork tracks 1:1. A day or two of work retires most of the fork's worst user-facing failure modes.

**Wave 1 — the node-pty patch-delivery cluster (one work item, three payoffs).**
[#8855](https://github.com/stablyai/orca/pull/8855)/[#8362](https://github.com/stablyai/orca/issues/8362) (FD_CLOEXEC on PTY master fds) and [#9586](https://github.com/stablyai/orca/issues/9586) (Windows SSH relay crash — the fix already exists in `config/patches/node-pty@1.1.0.patch` but never reaches remote hosts) share one root problem: `ssh-relay-deploy.ts` / `ssh-relay-versioned-install.ts` install **vanilla** node-pty on remotes via npm. Build one mechanism that ships the patched node-pty files to relay hosts, then both fixes (and any future patch hunk) ride it. Extend the local pnpm patch with the FD_CLOEXEC hunk at the same time.

**Wave 2 — agent-status & auth correctness (the app's core loop).**
[#9582](https://github.com/stablyai/orca/issues/9582) (Orca clobbers Claude Code OAuth creds — repeated 401s; make the credential write compare-and-swap), [#9237](https://github.com/stablyai/orca/pull/9237)/[#9236](https://github.com/stablyai/orca/issues/9236) (Claude Code ≥2.1.206 shared daemon breaks pane attribution — broken *today* for users on current CC; port the draft's spawn-pin + prompt-time rebind or reimplement server-side session_id→pane binding), [#6555](https://github.com/stablyai/orca/pull/6555)/[#6011](https://github.com/stablyai/orca/issues/6011) (`terminal wait --for tui-idle` resolves instantly mid-stream — breaks the fork's own orchestration-skill primitive; also fix the name-only-title→'idle' fall-through from #6011), and [#6554](https://github.com/stablyai/orca/pull/6554)/[#5657](https://github.com/stablyai/orca/issues/5657) (startup PATH probe wedges unkillably on Jamf/CrowdStrike Macs; take the PR's `-lc` probe and additionally consider running it in a disposable helper process per the issue analysis).

**Wave 3 — SSH scale & resilience.**
[#9015](https://github.com/stablyai/orca/pull/9015)/[#9014](https://github.com/stablyai/orca/issues/9014) (warm high-latency connects 88.7s → 7.7s — the single biggest UX win for the fork's SSH mandate; expect conflicts in `ssh-relay-session.ts`/`relay.ts`) and [#8871](https://github.com/stablyai/orca/issues/8871) (a stale client mirror's stream-end can kill a live host PTY — gate exit propagation on subscription ownership).

**Wave 4 — the one large P0.**
[#6884](https://github.com/stablyai/orca/pull/6884) (keep worktree mounted on terminal death, in-place recovery overlay). Highest UX payoff of the batch, but the keep-mounted state machine must be hand-ported into the fork's heavily diverged `pty-connection.ts`. Take the overlay component + i18n verbatim; adapt the rest. Do it last in P0 so the small wins aren't blocked behind it.

**Then P1, by theme, in roughly this order:** (a) editor typing/data integrity ([#7747](https://github.com/stablyai/orca/pull/7747) first — intermittent typing loss); (b) the workspace-persistence cluster ([#9200](https://github.com/stablyai/orca/pull/9200) right after #9342, then [#9025](https://github.com/stablyai/orca/pull/9025), [#6255](https://github.com/stablyai/orca/pull/6255), [#8063](https://github.com/stablyai/orca/pull/8063)); (c) daemon/PTY reliability ([#8449](https://github.com/stablyai/orca/pull/8449), [#5651](https://github.com/stablyai/orca/pull/5651), [#5307](https://github.com/stablyai/orca/pull/5307), [#9587](https://github.com/stablyai/orca/pull/9587)); (d) agent-status truthfulness ([#8442](https://github.com/stablyai/orca/pull/8442), [#9151](https://github.com/stablyai/orca/issues/9151), [#7202](https://github.com/stablyai/orca/pull/7202), [#7047](https://github.com/stablyai/orca/issues/7047), [#6595](https://github.com/stablyai/orca/pull/6595)); (e) git/review; (f) SSH/relay hygiene; (g) Windows/WSL. IME/aterm items ([#6698](https://github.com/stablyai/orca/issues/6698), [#9039](https://github.com/stablyai/orca/pull/9039), [#8335](https://github.com/stablyai/orca/issues/8335), [#6106](https://github.com/stablyai/orca/issues/6106)) are reimplementations against the fork's own IME/snapshot subsystems — batch them as one focused aterm session with repros first.

---

## 2. P0 — do now

| # | Kind | Title | Area | Effort | Port path |
|---|------|-------|------|--------|-----------|
| [5880](https://github.com/stablyai/orca/pull/5880) | PR | Git discard restores from index, not HEAD (data loss) | git | S | Cherry-pick; drop `--source=HEAD` in `src/main/git/status.ts` discardChanges + mirror in `src/relay/git-handler.ts` (~L656) |
| [9158](https://github.com/stablyai/orca/issues/9158) | issue | Rich markdown silently drops saves in non-ASCII docs (UCS-2 vs UTF-8 offsets) | editor | S | First cherry-pick PR [#9494](https://github.com/stablyai/orca/pull/9494) (+8/−1 `allowExceedingIndices` crash guard, verified absent in fork), then the UCS-2 index adjustment + try/catch fallback to canonical text; CJK/emoji round-trip tests. Folds in [#9492](https://github.com/stablyai/orca/issues/9492) |
| [7456](https://github.com/stablyai/orca/pull/7456) | PR | Self-heal daemon write routing for undiscovered legacy sessions | daemon | S | Cherry-pick 2-file hunk into `daemon-pty-router.ts` adapterFor/write + test |
| [8426](https://github.com/stablyai/orca/pull/8426) | PR | Keep PTY input alive when a live write throws | daemon | S | Cherry-pick into `pty-subprocess.ts` (~L1057): keep crash guard, drop `dead = true` in write/resize; +3 tests |
| [9342](https://github.com/stablyai/orca/pull/9342) | PR | Don't mint worktreeMeta from a sidebar sort-order snapshot | persistence | S | Cherry-pick (`ipc/worktrees.ts` + `orca-runtime.ts` + test); near-clean |
| [9263](https://github.com/stablyai/orca/pull/9263) | PR | Stale/gone remote handle misclassified as agent completion | agent-status | S | Cherry-pick (`agent-completion-coordinator.ts` + `runtime-terminal-inspection.ts` + tests); do together with P1 [#9151](https://github.com/stablyai/orca/issues/9151) |
| [7948](https://github.com/stablyai/orca/pull/7948) | PR | Refuse recursive fs.watch on home/filesystem roots (SSH freeze) | relay | S | Cherry-pick onto `src/relay/fs-handler.ts`; if conflicted, guard at the fs.watch onRequest entry |
| [8855](https://github.com/stablyai/orca/pull/8855) | PR | node-pty master fds not FD_CLOEXEC (dead terminals kept alive) | pty/relay | M | Extend `config/patches/node-pty@1.1.0.patch` per PR; deliver to relay hosts via the Wave-1 patch-delivery mechanism (issue [#8362](https://github.com/stablyai/orca/issues/8362)) |
| [9586](https://github.com/stablyai/orca/issues/9586) | issue | Windows SSH relay crashes: AttachConsole failed (vanilla node-pty on remotes) | relay | S | Ship the already-authored patched `conpty_console_list_agent.js` with the relay bundle / post-install overwrite in `ssh-relay-deploy.ts` |
| [9582](https://github.com/stablyai/orca/issues/9582) | issue | Orca clobbers Claude Code OAuth credentials (multi-writer race → 401s) | auth | M | Make `.credentials.json` writes compare-and-swap on current contents/mtime in `runtime-auth-service.ts`; take upstream fix if it lands first |
| [9237](https://github.com/stablyai/orca/pull/9237) | PR | Claude Code ≥2.1.206 shared daemon breaks pane attribution | agent-status | M | Port draft (spawn pin + fork lineage + prompt-time rebind; files map 1:1) or reimplement session_id→paneKey binding in `agent-hooks/server.ts` per issue [#9236](https://github.com/stablyai/orca/issues/9236) |
| [6555](https://github.com/stablyai/orca/pull/6555) | PR | `tui-idle` waits need sustained quiescence (instant false satisfy) | runtime | M | Cherry-pick onto `orca-runtime.ts` immediate-resolve sites; unify with existing TUI_IDLE_QUIESCENCE_MS; also fix name-only-title→'idle' fall-through from issue [#6011](https://github.com/stablyai/orca/issues/6011) |
| [6554](https://github.com/stablyai/orca/pull/6554) | PR | Startup PATH probe hangs unkillably on managed Macs (Jamf/CrowdStrike) | startup | M | Cherry-pick `-ilc`→`-lc` + new shell-path modules; consider issue [#5657](https://github.com/stablyai/orca/issues/5657)'s disposable-helper-process hardening on top |
| [9015](https://github.com/stablyai/orca/pull/9015) | PR | Warm high-latency SSH connects: 88.7s → 7.7s | ssh | M | Cherry-pick after merge settles; conflicts expected in `ssh-relay-session.ts`/`relay.ts`; new managed-hook modules land clean (issue [#9014](https://github.com/stablyai/orca/issues/9014)) |
| [8871](https://github.com/stablyai/orca/issues/8871) | issue | Stale remote mirror pty-exit can kill live host terminal sessions | remote | M | Gate exit propagation on subscription ownership/generation in `remote-runtime-pty-transport.ts` onEnd→onPtyExit + daemon exit-driven cleanup |
| [6884](https://github.com/stablyai/orca/pull/6884) | PR | Keep worktree mounted on terminal death; in-place recovery overlay | terminal UX | L | Adapt, don't cherry-pick: overlay + i18n verbatim; hand-port keep-mounted state machine into fork's diverged `pty-connection.ts` / lifecycle hooks |

---

## 3. P1 — next

### Editor & data integrity
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [7747](https://github.com/stablyai/orca/pull/7747) | PR | Default editors to classic input path (intermittent typing loss) | M | Cherry-pick `monaco-input-mode.ts` + settings toggle + 5 call sites |
| [6535](https://github.com/stablyai/orca/pull/6535) | PR | Rich-markdown mangles pasted Windows file paths | S | Cherry-pick autolink/isAllowedUri guards in `rich-markdown-extensions.ts` |
| [1476](https://github.com/stablyai/orca/pull/1476) | PR | Editor close cleanup leaks (relocate to App level; URI half already fixed) | M | Port only the relocation of `useClosedEditorTabCleanup` to an always-mounted host |

### Workspace / session persistence (do as a cluster, after #9342)
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [9200](https://github.com/stablyai/orca/pull/9200) | PR | Reclaim orphaned worktree/session state on load (phantom duplicate workspace) | M | Cherry-pick with #9342; minor `persistence.ts` conflicts; upstream CI unstable — verify tests |
| [9025](https://github.com/stablyai/orca/pull/9025) | PR | Prune workspace session state when a project is removed | S | Cherry-pick after merge settles `persistence.ts` |
| [6255](https://github.com/stablyai/orca/pull/6255) | PR | Clear worktree session state when metadata is removed | S | Call `removeWorkspaceSessionOwner` from `removeWorktreeMeta` incl. per-host partitions |
| [8063](https://github.com/stablyai/orca/pull/8063) | PR | Remap stale worktree ids on restore ('Unknown' sessions) | S | Add 2 new files verbatim + 8-line wiring in `store/slices/terminals.ts` |
| [4573](https://github.com/stablyai/orca/issues/4573) | issue | Sleep filter shows inactive lineage parents (upstream fix merged then reverted) | S | Reimplement: gate `addVisibleLineageAncestors` with the sleep filter's own predicate (ref PR [#4574](https://github.com/stablyai/orca/pull/4574)) |
| [9352](https://github.com/stablyai/orca/issues/9352) | issue | Remote host respawns manually-closed terminal tabs | M | Prune pty-handle-keyed tab bindings on runtimeId churn per the #9585 analysis; take upstream fix if it lands |
| [9499](https://github.com/stablyai/orca/pull/9499) | PR | Claude session resume fails when last cwd ≠ start dir (fixes issue #9361) | S | Cherry-pick (70+/2f: `session-scanner-accumulator.ts` + test); verified fork's `updateLatestLocation` still overwrites cwd per record |
| [9514](https://github.com/stablyai/orca/pull/9514) | PR | Removed runtime environment leaves orphaned `runtime:<envId>` session partitions (error-loop toasts on launch) | S | Cherry-pick with the cluster: new self-heal module lands clean; re-apply `ipc/runtime-environments.ts` + `persistence.ts` wiring by hand |
| [9535](https://github.com/stablyai/orca/pull/9535) | PR | Claude Code sub-agent scratch worktrees (`.claude/worktrees/agent-*`) surface as sidebar workspaces | M | Cherry-pick (340+/12f): new `shared/agent-scratch-worktrees.ts` classifier clean; expect small conflicts in `ipc/worktrees.ts` / `orca-runtime.ts` |

### Daemon / PTY reliability
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [8449](https://github.com/stablyai/orca/pull/8449) | PR | Reattach live PTY sessions after daemon socket reconnect (App Nap stuck panes) | M | Adapt by hand next to fork's `producerResumesOwedOnReconnect`; port issue-8335-reconnect test |
| [5651](https://github.com/stablyai/orca/pull/5651) | PR | Chunk Windows ConPTY writes (large-paste truncation) | S | Cherry-pick into `pty-subprocess.ts` write closure, win32-only |
| [5307](https://github.com/stablyai/orca/pull/5307) | PR | Empty takePendingOutput burns seq → whole history log rejected | S | Port seq-advance-only-on-append into `daemon/session.ts`; align with fork's history log |
| [9587](https://github.com/stablyai/orca/pull/9587) | PR | Tolerate dead legacy daemon generations during worktree removal | S | Cherry-pick (`daemon-pty-router.ts` + test); fork's own generation bumps raise exposure |
| [9396](https://github.com/stablyai/orca/pull/9396) | PR | ConPTY conin drops VT state across write boundaries (backspace types a space after resume) | M | Port new `daemon/conin-atomic-sequence-writer.ts` + tests verbatim; hand-apply `daemon/session.ts` wiring; do as one pass with #5651 (same write path). Prior partial fix #9259 already merged |
| [8796](https://github.com/stablyai/orca/pull/8796) | PR | Wait for zsh line-init before agent startup (pasted launch never executes) | S | Cherry-pick `shell-templates.ts` + test; byte-identical base |
| [8000](https://github.com/stablyai/orca/pull/8000) | PR | Prune unbound layout leaves at mount (blank ghost panes) | S | New module verbatim + 19-line wiring into `use-terminal-pane-lifecycle` |
| [6566](https://github.com/stablyai/orca/pull/6566) | PR | Reflow surviving terminal when a split collapses | S | Port refit hook wired to fork's SYNC_FIT_PANES_EVENT / PaneManager.fitAllPanes |
| [6582](https://github.com/stablyai/orca/pull/6582) | PR | Window jitter / high CPU during continuous resize | M | Cherry-pick resize-settle module; route fit calls to aterm safeFit |
| [7936](https://github.com/stablyai/orca/issues/7936) | issue | macOS logout leaves orphaned PTYs → broken login terminals | M | SIGTERM/SIGHUP handler in detached daemon entry running the existing force-kill/dispose path |
| [9530](https://github.com/stablyai/orca/issues/9530) | issue | Stale OMP sessions survive terminal close (~38h memory) | M | Extend `pty-descendant-termination.ts` to plain-terminal teardown, narrowly scoped |
| [7806](https://github.com/stablyai/orca/pull/7806) | PR | Orphaned daemon cleanup after app quit (`orca status` daemon visibility, `stop --all`) | M | Cherry-pick CLI-side files after merge; re-target `local-daemon-sessions.ts` at fork daemon layout |
| [9045](https://github.com/stablyai/orca/issues/9045) | issue | Windows worktree deletion blocked by orphaned claude.exe handles | M | Windows last-resort sweep (RestartManager) in worktree-delete path; Rust `resolve_cwd` is None on Windows today |

### Terminal input / paste / links (some aterm-side)
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [7786](https://github.com/stablyai/orca/pull/7786) | PR | Paste OS-copied files as full shell-escaped paths | M | Cherry-pick main/preload files verbatim; hand-merge ~35 renderer lines (issue [#7777](https://github.com/stablyai/orca/issues/7777)) |
| [8993](https://github.com/stablyai/orca/pull/8993) | PR | X11 middle-click primary paste inserts twice | S | Cherry-pick hook/lib; re-apply TerminalPane.tsx hunk by hand |
| [6282](https://github.com/stablyai/orca/pull/6282) | PR | Re-probe stale "missing" path results (dead terminal links) | S | Add TTL to fork's `terminal-path-exists-cache.ts` (refactored shape — rework, not pick) |
| [8156](https://github.com/stablyai/orca/issues/8156) | issue | WSL POSIX file-path links never clickable | M | Map POSIX paths from WSL worktrees to `\\wsl.localhost\...` in pathExists/open routing |
| [9153](https://github.com/stablyai/orca/pull/9153) | PR | Pane divider drag dead under WSLg (pen-type pointer events) | M | Apply pen-acceptance to BOTH guard sites: `pane-divider-drag.ts` + fork's `tab-group-split-resize-drag.ts` |
| [6698](https://github.com/stablyai/orca/issues/6698) | issue | Vietnamese Telex IME loses characters (aterm IME tracker) | M | Reimplement against fork IME subsystem; repro with Telex + Korean first |
| [9039](https://github.com/stablyai/orca/pull/9039) | PR | Hangul syllable lost on Shift/Ctrl+Enter | M | Reimplement: port `terminal-ime-deferred-newline.ts` into fork keyboard path; reuse PR's e2e spec |
| [8335](https://github.com/stablyai/orca/issues/8335) | issue | Terminal stuck echoing mouse motion after backgrounding during external editor | M | aterm-side: resync mouse modes on reveal/reattach from aterm's authoritative mode state |
| [6106](https://github.com/stablyai/orca/issues/6106) | issue | SSH terminal loses pre-TUI scrollback after Codex tab restore | M | aterm-side: extend alt-screen snapshot shape for SSH provider; repro per issue recipe |
| [6975](https://github.com/stablyai/orca/pull/6975) | PR | Windows ConPTY OSC color reply leak | S | Reimplement in fork's daemon responder: gate OSC 10/11/12 replies behind ConPTY flag (mirror conptyDa1Override) |
| [965](https://github.com/stablyai/orca/pull/965) | PR | Webview teardown leaks background video/audio after close *(demoted from P0: media leak, not data loss)* | S | Two snippets only: stop()+about:blank in `webview-registry.ts`; close guest WebContents in `browser-manager.ts` |

### Agent status, completion & notifications
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [8442](https://github.com/stablyai/orca/pull/8442) | PR | Suppress fresh-working false completion notifications | S | Light adaptation of the gate in fork's `use-notification-dispatch.ts` (issue [#4375](https://github.com/stablyai/orca/issues/4375)) |
| [9507](https://github.com/stablyai/orca/pull/9507) | PR | Premature "Task complete" when a Codex tool outlives the 1.5s hook-done quiet window (issue #9333) | S | Cherry-pick (87+/2f) into fork's `terminal-pane/agent-completion-coordinator.ts` (verified: quiet window present, tool-running guard absent); do with #8442 |
| [9151](https://github.com/stablyai/orca/issues/9151) | issue | Remote disconnect misclassified as agent completion (renderer transport) | M | Only emit onExit on confirmed host exit; route no_connected_pty/stream-end to a disconnected state. Pair with #9263 |
| [6595](https://github.com/stablyai/orca/pull/6595) | PR | Duplicate task-complete notifications from background plugin hooks | M | Port `agent-tool-progress-hooks.ts` verbatim + small server/coordinator deltas |
| [7202](https://github.com/stablyai/orca/pull/7202) | PR | Codex stream errors leave rows stuck 'working' | M | Adapted pick: `codex-error-output-status.ts` into `src/shared/`, wire into fork coordinator |
| [7047](https://github.com/stablyai/orca/issues/7047) | issue | Agent marked 'Done' when CLI is not installed (exit 127) | M | Treat pre-recognition immediate exit / 127 as 'failed to launch' in shared status path |
| [7799](https://github.com/stablyai/orca/pull/7799) | PR | Detect agents running inside tmux | M | Add tmux client→server subtree resolution in `agent-foreground-process.ts` + relay twin (issue [#7797](https://github.com/stablyai/orca/issues/7797)) |
| [9013](https://github.com/stablyai/orca/pull/9013) | PR | Don't cache empty agent detection results (empty new-tab agent list) | S | Cherry-pick 2 renderer files (issue [#9011](https://github.com/stablyai/orca/issues/9011)) |
| [5149](https://github.com/stablyai/orca/pull/5149) | PR | Self-heal stale Windows agent detection runtime | S | Cherry-pick 6 files + tests |
| [9098](https://github.com/stablyai/orca/pull/9098) | PR | Skip sleeping-session resume for runtime-owned worktrees (duplicate TUIs) | M | Adapt-port gate + both tests; commit exists only on the PR branch, NOT in fork HEAD (issue [#8878](https://github.com/stablyai/orca/issues/8878)) |
| [8459](https://github.com/stablyai/orca/issues/8459) | issue | Resource Manager force-kills live daemon sessions as 'orphans' | S | Add confirm on bulk kill + re-verify liveness against daemon inventory pre-kill |
| [6072](https://github.com/stablyai/orca/issues/6072) | issue | Mobile shows stale agent rows after terminals close | M | Cherry-pick open upstream PR #9053 + gate hydrated 'done' hook rows with no live PTY |

### Auth & accounts
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [6864](https://github.com/stablyai/orca/pull/6864) | PR | Recover stale managed Claude accounts via cross-store reconcile | M | Cherry-pick with conflicts in `runtime-auth-service.ts`; review carefully (unreviewed upstream). Pairs with P0 #9582 |
| [8985](https://github.com/stablyai/orca/issues/8985) | issue | macOS TCC prompt re-appears every subprocess launch (regression) | S | Await `prepareMacosTccLoginShell` at daemon-adapter spawn boundary too |
| [8711](https://github.com/stablyai/orca/issues/8711) | issue | Codex status hooks dead in SSH worktrees (CODEX_HOME local-only) | M | Inject CODEX_HOME/ORCA_CODEX_HOME into SSH spawn env via relay pty-handler |
| [9155](https://github.com/stablyai/orca/issues/9155) | issue | Spawn-env hygiene: CLAUDE_CODE_CHILD_SESSION inherited → silent transcript loss | S | Scrub CC session markers in all 3 spawn-env builders; fold in NODE_ENV scrub ([#9058](https://github.com/stablyai/orca/pull/9058)/[#9057](https://github.com/stablyai/orca/issues/9057), dev-build-only) |

### Git, review & providers
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [9143](https://github.com/stablyai/orca/pull/9143) | PR | Parallel worktree checkout (perf on hottest git path) | S | Cherry-pick; verify Git <2.32 degrade per compat doc; GitCapabilityCache gate if absent |
| [4626](https://github.com/stablyai/orca/pull/4626) | PR | Worktree creation on git-crypt repos (defer checkout until keys copied) | M | Cherry-pick into `git/worktree.ts` + relay lockstep |
| [8542](https://github.com/stablyai/orca/pull/8542) | PR | Multi-remote PR fetch missing-ref detection | M | Reimplement: feed `error.stderr` into the fork's Rust classification boundary + alternate-remote fallback |
| [9570](https://github.com/stablyai/orca/pull/9570) | PR | Dedup superseded CI check runs (stale failing status) | S | Cherry-pick new `check-run-dedup.ts` + wiring |
| [9111](https://github.com/stablyai/orca/pull/9111) | PR | Load PR diffs for GitHub Enterprise remotes | S | Clean cherry-pick (`work-item-details.ts` + tests) |
| [9216](https://github.com/stablyai/orca/pull/9216) | PR | Scope issue list to resolved repo (kills global search + rate-limit burn) | S | Cherry-pick; verify resolver path (issue [#9202](https://github.com/stablyai/orca/issues/9202)) |
| [8726](https://github.com/stablyai/orca/issues/8726) | issue | Fork PR routing: resolve work items upstream-first under 'auto' | S | Cherry-pick open upstream PR #8727 (+4/−2 in `github/client.ts`); directly hits orc's own daily workflow |
| [1715](https://github.com/stablyai/orca/issues/1715) | issue | GitHub Projects uses wrong gh host in multi-host setups | S | Thread `--hostname` (derived from remote) through Projects graphql/scope probes |
| [7732](https://github.com/stablyai/orca/issues/7732) | issue | GitLab pipeline job details never load in Checks panel | M | Add job id to PrCheckDetail in Rust `gitlab_pipeline_checks.rs` + wasm regen; branch `handleLoadCheckDetails` by provider |
| [6712](https://github.com/stablyai/orca/issues/6712) | issue | Ephemeral VM workspaces fail for GitLab/Bitbucket | S | Relax github-only providerIdentity guard in `ipc/repos.ts:139` |

### SSH / relay / remote runtime
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [8618](https://github.com/stablyai/orca/pull/8618) | PR | Reap detached relay after failed reconnect (remote process leak) | M | Cherry-pick pidfile + reap-before-relaunch; minor relay.ts drift. Folds in issue [#8585](https://github.com/stablyai/orca/issues/8585) (accepted-socket-client makes grace timer a no-op) |
| [3724](https://github.com/stablyai/orca/pull/3724) | PR | Discard oversized relay frames early (up-to-4GB buffering) | S | Cherry-pick both protocol decoders + tests |
| [8608](https://github.com/stablyai/orca/issues/8608) | issue | Relay CLI bin dir not front-of-PATH → worker_done to wrong runtime | S | Strip + unconditionally prepend in all 3 `pty-shell-launch.ts` wrappers |
| [5197](https://github.com/stablyai/orca/pull/5197) | PR | SSH OpenCode commit-message generation ENOENTs | M | Adapt: wrap `agent-exec-handler.ts` spawns in user login shell on POSIX remotes |
| [6938](https://github.com/stablyai/orca/pull/6938) | PR | Proxy skill discovery + preflight to remote runtime | S | Near-clean cherry-pick (new `preflight-remote-runtime.ts` + small diffs) |
| [6032](https://github.com/stablyai/orca/issues/6032) | issue | Remote servers can lose host-scoped repo/project state | M | Cherry-pick open PR stack #6030 → #6031 (renderer store slices, regression tests) |
| [6981](https://github.com/stablyai/orca/pull/6981) | PR | Recover stale coordinator runs after crash ('already running' forever) | S | Cherry-pick; adjust to fork's Rust-backed OrchestrationDb delegate |
| [9351](https://github.com/stablyai/orca/pull/9351) | PR | `orchestration.ask` not classified long-poll — dies at the 30s socket idle wall | S | Cherry-pick (53+/2f): add the ask branch to `isLongPollRequest` in `runtime-rpc.ts` (verified missing at fork L397); breaks the fork's own skill primitive today |
| [4389](https://github.com/stablyai/orca/issues/4389) | issue | Multiple orchestrators in one workspace kill each other | M | Wait briefly for upstream (their code); else key coordinator runs by handle + targeted run-stop |
| [8539](https://github.com/stablyai/orca/issues/8539) | issue | Renderer freezes 87s on reconnect with many worktrees (sync reflow) | M | Reimplement: profile with `timeRendererStartupStep`, break the forced layout read in mount effects |
| [8134](https://github.com/stablyai/orca/pull/8134) | PR | Mobile keystrokes as fire-and-forget binary frames (host side already supports) | M | Cherry-pick the 10 `mobile/` files; host decode path already present |

### Automations
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [8229](https://github.com/stablyai/orca/pull/8229) | PR | Stop mounted worktrees fresh-spawning a shell into background run tabs | M | Cherry-pick new module + rework of `launch-agent-background-session.ts`; verify vs runtime adoption path |
| [9479](https://github.com/stablyai/orca/issues/9479) | issue | Recurring automation runs leak background tabs/PTYs | M | Ownership-scoped teardown at run completion (ref PR [#3337](https://github.com/stablyai/orca/pull/3337) for the close-hidden-tab mechanism) |

### Windows / WSL
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [7503](https://github.com/stablyai/orca/pull/7503) | PR | Honor WSL default runtime for preflight + folder picking | M | Cherry-pick 6 files; manual conflicts in `local-preflight-context.ts` |
| [6896](https://github.com/stablyai/orca/issues/6896) | issue | Worktree setup command built as cmd.exe — broken under Git Bash | M | Thread configured shell into `resolveSetupRunnerCommand`; fork already has shell-classification helpers |
| [8787](https://github.com/stablyai/orca/issues/8787) | issue | Windows wait-for-setup never starts agent (doubled quotes in cmd wrapper) | S | Rewrite `wrapCmd` in `setup-agent-sequencing.ts` for `/s /c` semantics + test |
| [9498](https://github.com/stablyai/orca/issues/9498) | issue | WSL-managed CLI fails: .NET duplicate 'PATH' vs 'Path' | S | Case-insensitive env-key dedupe in `buildWslBridgeScript()` |
| [9123](https://github.com/stablyai/orca/pull/9123) | PR | Duplicate broken orca.cmd shim shipped in app.asar | S | Manual port: add `!resources/win32/**` exclude in fork's diverged builder config |
| [8845](https://github.com/stablyai/orca/pull/8845) | PR | Resource Manager attribution broken (wmic removed from Win11) | S | Cherry-pick collector.ts typeperf/CIM fallback |
| [9372](https://github.com/stablyai/orca/pull/9372) | PR | WSL git status spawns interactive login shell per call (fixes issue [#9284](https://github.com/stablyai/orca/issues/9284)) | S | Cherry-pick (36+/2f: `git/runner.ts` + test); verified fork still passes `useWslLoginShell: Boolean(wslDistro)` on hot read paths; keep login shell for `core.sshCommand` probing |

### Misc
| # | Kind | Title | Effort | Port path |
|---|------|-------|--------|-----------|
| [9094](https://github.com/stablyai/orca/pull/9094) | PR | Racing app instances silently kill macOS update installs (fork ships own builds) | L | Cherry-pick; two new modules clean, updater wiring conflicts; OK to wait one upstream cycle |
| [5097](https://github.com/stablyai/orca/issues/5097) | issue | Default shell setting for macOS/Linux (Windows-only today) | S | Extend existing TerminalWindowsShellSection pattern; shellOverride plumbing already reaches spawn |
| [6645](https://github.com/stablyai/orca/pull/6645) | PR | Scan repo-root `skills/` for bundled skills | S | +9-line source entry in `skill-discovery-sources.ts` + test |

---

## 4. P2 — backlog

| # | Kind | Title | Effort | Note |
|---|------|-------|--------|------|
| [7269](https://github.com/stablyai/orca/pull/7269) | PR | Constant-time token comparison (3 boundaries) | S | Localhost-only surfaces; cheap hardening |
| [7659](https://github.com/stablyai/orca/pull/7659) | PR | Accept GitHub restricted-shell SSH probes | S | Issue [#6988](https://github.com/stablyai/orca/issues/6988) |
| [3807](https://github.com/stablyai/orca/pull/3807) | PR | Generate runtime TLS certs without openssl | S | ENOENT on openssl-less hosts |
| [7624](https://github.com/stablyai/orca/pull/7624) | PR | Debounce git-conflict decoration rebuilds | S | Editor perf |
| [5429](https://github.com/stablyai/orca/pull/5429) | PR | Seed find widget from selection | S | 3 hardcoded 'never' sites |
| [5134](https://github.com/stablyai/orca/pull/5134) | PR | Windows first-launch ACL grant lags machine | S | Follow-up to merged #5124 |
| [9010](https://github.com/stablyai/orca/pull/9010) | PR | WSL_UTF8 on remaining wsl.exe spawns | S | Codex WSL paths |
| [8994](https://github.com/stablyai/orca/pull/8994) | PR | Absolute reg.exe for Windows PATH probe | S | |
| [8987](https://github.com/stablyai/orca/pull/8987) | PR | Symlinked dirs in Add Project browsers | S | Two sites (runtime + ssh-browse) |
| [9583](https://github.com/stablyai/orca/pull/9583) | PR | Paste copied image files on Windows | S | CF_HDROP path; above the engine |
| [9554](https://github.com/stablyai/orca/pull/9554) | PR | Redact sensitive Pi/OMP status tool input | S | .ssh/secret-path leak into status pipeline |
| [9130](https://github.com/stablyai/orca/issues/9130) | issue | Mobile pairing on IPv6-only hosts | S | IPv4-only filter in `ipc/mobile.ts` |
| [9490](https://github.com/stablyai/orca/issues/9490) | issue | Mobile-aware terminal flush window (battery) | S | Client type already known server-side |
| [9553](https://github.com/stablyai/orca/issues/9553) | issue | Unscoped global issue search for `git:` projects | S | Sibling of #9216 |
| [9402](https://github.com/stablyai/orca/pull/9402) | PR | Duplicate GitHub review polling from sidebar cards (rate-limit burn) | S | Batch with #9216; verify coordinator ownership first |
| [9404](https://github.com/stablyai/orca/pull/9404) | PR | Transient macOS PAM probe failure permanently disables merged TCC fix (#9301 follow-up) | S | Paths map 1:1; let it settle upstream a week |
| [9411](https://github.com/stablyai/orca/pull/9411) | PR | Orchestration skill activates in non-Orca Codex sessions | S | Port the identity gate into fork's `skills/orchestration/` wording |
| [9548](https://github.com/stablyai/orca/pull/9548) | PR | Workspace shortcut order reshuffles when sidebar closes (#9497) | S | 4 renderer files; minor drift vs fork sidebar perf patches |
| [8125](https://github.com/stablyai/orca/issues/8125) | issue | Git tab missing after git init (kind fixed at add time) | S | Add re-detection/promotion |
| [7898](https://github.com/stablyai/orca/pull/7898) | PR | Source ~/.bashrc in bash wrapper (nvm/fnm) | S | Contradicts a deliberate upstream design comment — decide policy first |
| [4566](https://github.com/stablyai/orca/issues/4566) | issue | Pre-create hook for worktree creation | M | Complements #4626 (git-crypt) |
| [6258](https://github.com/stablyai/orca/issues/6258) | issue | Automatic git fetch | M | Net-new; stale ahead/behind misleads agents |
| [1479](https://github.com/stablyai/orca/pull/1479) | PR | Custom CLI agent profiles | M | Feature; fits orchestration focus |
| [1549](https://github.com/stablyai/orca/pull/1549) | PR | Opt-in workspace branch publishing | M | Feature |
| [1776](https://github.com/stablyai/orca/pull/1776) | PR | Default prompts (per-agent persistent instructions) | M | Feature |
| [1153](https://github.com/stablyai/orca/pull/1153) | PR | Auto-close worktree after PR merge *(demoted from P1: feature, not a fix)* | S | Mostly one additive controller file |
| [4384](https://github.com/stablyai/orca/issues/4384) | issue | orca:// deep links *(demoted from P1: feature, no upstream PR yet)* | M | OSC-link seam is fork-controlled |
| [4444](https://github.com/stablyai/orca/issues/4444) | issue | Codex history reverse bridge *(demoted from P1: integration parity, hardlink risk)* | M | |
| [8662](https://github.com/stablyai/orca/pull/8662) | PR | Relative times in configured UI language *(demoted from P1: approved upstream — will arrive via routine merge)* | S | Pick only if a release ships first |
| [5742](https://github.com/stablyai/orca/pull/5742) | PR | Timeouts/abort for push/pull/fetch (stuck source control) | L | Real gap; large PR — consider trimming to timeout+abort core |
| [5950](https://github.com/stablyai/orca/pull/5950) | PR | Per-hunk stage/unstage in diff viewer | L | 26-file feature; high value, big lift |
| [1693](https://github.com/stablyai/orca/issues/1693) | issue | Pre-bundle node-pty per platform for SSH relay | L | Fork carries `TODO(#1693)`; Wave-1 patch delivery is the cheap first step |
| [8652](https://github.com/stablyai/orca/issues/8652) | issue | SSH PTY tabs excluded from hidden-view parking (heap growth) | L | Needs snapshot support for remote PTYs — design work |
| [5311](https://github.com/stablyai/orca/issues/5311) | issue | WSL projects (open projects inside WSL) | L | Wait for upstream #8286 design/draft PR |
| [4280](https://github.com/stablyai/orca/issues/4280) | issue | First-class headless/server mode | L | Highest demand; reconcile with fork's web-renderer foundations; track upstream |
| [9512](https://github.com/stablyai/orca/issues/9512) | issue | Submit PR reviews from Orca | L | Feature, no upstream PR; mind GitLab parity mandate |

**Verify-first (likely already covered by fork/merges — confirm, then close):** [#4605](https://github.com/stablyai/orca/pull/4605), [#5992](https://github.com/stablyai/orca/pull/5992), [#6138](https://github.com/stablyai/orca/pull/6138), [#6234](https://github.com/stablyai/orca/issues/6234), [#6357](https://github.com/stablyai/orca/issues/6357), [#6635](https://github.com/stablyai/orca/issues/6635), [#6650](https://github.com/stablyai/orca/pull/6650), [#6880](https://github.com/stablyai/orca/issues/6880), [#6891](https://github.com/stablyai/orca/issues/6891), [#7150](https://github.com/stablyai/orca/pull/7150), [#7209](https://github.com/stablyai/orca/issues/7209), [#7410](https://github.com/stablyai/orca/issues/7410), [#7431](https://github.com/stablyai/orca/issues/7431), [#7623](https://github.com/stablyai/orca/issues/7623), [#7848](https://github.com/stablyai/orca/issues/7848), [#8180](https://github.com/stablyai/orca/issues/8180), [#9138](https://github.com/stablyai/orca/issues/9138), [#5370](https://github.com/stablyai/orca/issues/5370).

---

## 5. Deliberately skipped (notable)

- **Already in the fork via merges** (verified by commit archaeology): [#7260](https://github.com/stablyai/orca/pull/7260) (sleep-wake ACK recovery — #7214 branch absorbed), [#7722](https://github.com/stablyai/orca/pull/7722) (listFiles starvation — #7769 merged), [#7354](https://github.com/stablyai/orca/pull/7354) (submodule worktree removal — #9096), [#7707](https://github.com/stablyai/orca/pull/7707) (oversized issue bodies), [#6483](https://github.com/stablyai/orca/pull/6483) (ConPTY scrollback), [#8260](https://github.com/stablyai/orca/issues/8260) (white-screen crash — fixed 70e16cb90), [#7774](https://github.com/stablyai/orca/pull/7774), [#7323](https://github.com/stablyai/orca/issues/7323), [#8591](https://github.com/stablyai/orca/issues/8591), [#7345](https://github.com/stablyai/orca/issues/7345), [#5352](https://github.com/stablyai/orca/issues/5352), [#1283](https://github.com/stablyai/orca/pull/1283), [#5811](https://github.com/stablyai/orca/pull/5811), [#5527](https://github.com/stablyai/orca/pull/5527), [#8113](https://github.com/stablyai/orca/pull/8113).
- **Superseded by newer upstream work arriving in the current merge:** [#7615](https://github.com/stablyai/orca/pull/7615) (→ #9277 idle-daemon retirement), [#8982](https://github.com/stablyai/orca/pull/8982) (→ #9380/#9515 spinner clock), [#8383](https://github.com/stablyai/orca/pull/8383) (→ answered-wait machinery in v1.4.147), [#6860](https://github.com/stablyai/orca/pull/6860), [#7124](https://github.com/stablyai/orca/pull/7124), [#2834](https://github.com/stablyai/orca/pull/2834), [#6012](https://github.com/stablyai/orca/pull/6012) (→ P0 #6555).
- **Obsolete-by-fork (aterm/architecture):** [#2711](https://github.com/stablyai/orca/pull/2711) (CSI-u Ctrl+C leak — aterm encodes correctly), [#8563](https://github.com/stablyai/orca/issues/8563) (wheel-to-arrow — aterm gates on alt-screen), [#8038](https://github.com/stablyai/orca/issues/8038) (Hangul Enter — fork IME layer covers), [#7972](https://github.com/stablyai/orca/pull/7972) (renderer-delivery queue — pipeline rewritten), [#7299](https://github.com/stablyai/orca/pull/7299) (backpressure contracts — cat-flood campaign covers), [#2291](https://github.com/stablyai/orca/pull/2291) (remote input latency — fork fast path exists), [#4639](https://github.com/stablyai/orca/pull/4639) (parent watchdog — contradicts fork's survive-app-crash daemon design), [#8261](https://github.com/stablyai/orca/issues/8261) (auto-update silent quit — fork updater is explicit-click only), [#1604](https://github.com/stablyai/orca/pull/1604) (pane UUID keying — fork already pane-keyed).
- **High-profile feature asks with nothing portable yet:** [#1099](https://github.com/stablyai/orca/issues/1099) / [#7568](https://github.com/stablyai/orca/issues/7568) (multi-repo workspaces — folder workspaces + project groups cover most of it), [#1129](https://github.com/stablyai/orca/pull/1129) (in-repo .worktrees/ — relative workspaceDir config covers).
- **Out of repo:** [#4642](https://github.com/stablyai/orca/issues/4642) (`npx skills` is an external package).

---

## Dedupe ledger (issue ⇄ fixing PR)

Kept the PR, noted the issue: #6554←#5657, #6555←#6011 (+#6012 superseded), #8855←#8362, #9015←#9014, #9237←#9236 (issue kept as spec since PR is draft), #9013←#9011, #9216←#9202, #8442←#4375, #7786←#7777, #7799←#7797, #7659←#6988, #8618←#8585, #9098←#8878, #8229←#2989, #9158←#9492 (two issues, one fix). Kept the issue, PR as reference: #4573 (PR #4574 needs predicate rework), #9479 (PR #3337 mechanism), #9155 (folds PR #9058). Gap-fill swaps: #9499←#9361, #9372←#9284 (fixing PRs replaced the issue rows).

---

**Addendum (2026-07-20, gap-fill):** the 88 PRs in #9347–#9551 that the original chunking missed are now classified in `chunks/prs-9347plus.json` and deep-dived in `dives/dive-extra-5.json` (PR coverage 1023/1023). New entries above from that pass: P0 port path for [#9494](https://github.com/stablyai/orca/pull/9494) (folded into the #9158 row), P1 [#9351](https://github.com/stablyai/orca/pull/9351), [#9372](https://github.com/stablyai/orca/pull/9372), [#9396](https://github.com/stablyai/orca/pull/9396), [#9499](https://github.com/stablyai/orca/pull/9499), [#9507](https://github.com/stablyai/orca/pull/9507), [#9514](https://github.com/stablyai/orca/pull/9514), [#9535](https://github.com/stablyai/orca/pull/9535), and P2 [#9402](https://github.com/stablyai/orca/pull/9402), [#9404](https://github.com/stablyai/orca/pull/9404), [#9411](https://github.com/stablyai/orca/pull/9411), [#9548](https://github.com/stablyai/orca/pull/9548). Watch item: [#9522](https://github.com/stablyai/orca/pull/9522) (host-owned remote terminal lifecycle — the comprehensive answer to the P0 #8871 family; too fresh/large to port, re-evaluate next merge cycle).
