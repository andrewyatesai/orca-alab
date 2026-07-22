# Upstream Triage Landscape — stablyai/orca open issues & PRs

Snapshot: 2026-07-20 · upstream head ~v1.4.147 · fork merge base: just-merged v1.4.147-era main.
Source: 777 open issues + 1023 open PRs, classified by 19 chunk agents + a gap-fill pass.
Merged classifications: `upstream-triage/triage-index.json` (1800 unique items after dedup).

> **Coverage note:** the `prs-0` chunk double-covered the #8287–#8750 range (88 duplicate
> classifications, deduped in the index), which initially left the 88 PRs in #9347–#9551
> unclassified. That gap is now filled by `chunks/prs-9347plus.json` (deep dives:
> `dives/dive-extra-5.json`), merged into the index. Coverage: **777/777 issues, 1023/1023 PRs**.

## Overall stats

### By area (issues / PRs classified)

| Area | Issues | PRs | Total |
|---|---:|---:|---:|
| ui-ux | 186 | 220 | 406 |
| agents-orchestration | 181 | 211 | 392 |
| terminal | 62 | 78 | 140 |
| git-source-control | 59 | 77 | 136 |
| other | 57 | 68 | 125 |
| mobile | 55 | 66 | 121 |
| ssh-remote | 53 | 64 | 117 |
| daemon-pty | 38 | 64 | 102 |
| review-github-gitlab | 30 | 57 | 87 |
| wsl-windows | 26 | 41 | 67 |
| updater-packaging | 16 | 17 | 33 |
| ci-infra | 3 | 25 | 28 |
| i18n | 6 | 15 | 21 |
| docs | 4 | 10 | 14 |
| telemetry | 1 | 10 | 11 |

### By category

| Category | Issues | PRs |
|---|---:|---:|
| feature | 407 (52%) | 397 (39%) |
| bug | 330 (42%) | 507 (50%) |
| perf | 15 | 69 |
| ci-infra | 3 | 24 |
| docs / question / other | 22 | 26 |

### Fork relevance

| Relevance | Issues | PRs | Total | Share |
|---|---:|---:|---:|---:|
| applies as-is | 725 | 952 | 1677 | 93.2% |
| needs aterm equivalent | 35 | 50 | 85 | 4.7% |
| already-fixed-likely (by aterm) | 11 | 12 | 23 | 1.3% |
| obsolete-by-fork | 6 | 9 | 15 | 0.8% |

## Dominant themes — issues

- **Agent-orchestration correctness is the #1 pain** (181 issues): false completion/interrupt
  inference, status attribution broken by Claude Code's shared daemon, silent dispatch drops,
  provider auth churn (Claude OAuth clobbering #9582, CODEX_HOME redirection wars #5370/#8612,
  daily managed-Claude re-auth #6234), rate-limit/account switching asks.
- **UI/UX workspace organization** (186): multi-window (#9588/#6074), multi-repo workspaces
  (#1099, #7568, #7900), tab groups, sidebar/board layout, command palette, LSP/editor asks.
- **Session/PTY lifecycle rot**: P0 ghost-tab respawn on remote hosts (#9352 + root-cause twin
  #9585), orphaned PTYs after macOS logout (#7936), stale daemon generations across updates,
  auto-updater killing all PTYs, node-pty FD_CLOEXEC fd leak in the remote relay.
- **SSH/remote resilience** (53): 95s connects on high-RTT links, reconnect PTY leaks,
  keyboard-interactive MFA (#8622), sleep-recovery failures, disconnect misread as completion.
- **Mobile beta bug cluster** (55): pairing loss, CJK/Japanese IME broken (#7427), dropped
  keystrokes on Android (#7094), host rename/remove freezes (filed 3x).
- Report quality is unusually high: many July issues arrive pre-root-caused with file/line
  pointers, making them cheap cherry-picks once upstream lands fixes.

## Dominant themes — PRs

- **Agent/provider plumbing dominates** (189): Codex multi-account stacks, Grok/Cursor/Command
  Code rate-limit handling, new-agent catalog additions, agent-status truthfulness fixes.
- **Systematic perf campaigns**: nwparker's perf-7…perf-21 series (PTY hot paths, renderer
  indexing, memory bounding) and brennanb2025's Windows-crash-hardening + backlog-feature sweep.
- **A rich vein of small, root-caused bug fixes** in daemon/SSH/git/review layers the fork
  shares unchanged — the best porting material (see below).
- **Review state is useless as a signal**: only 2 of 1023 PRs have any reviewDecision
  (1 APPROVED — #8662 i18n relative times; 1 CHANGES_REQUESTED — #6181). Ranking must lean on
  size, evidence quality, recency, and author track record.

### Noise / staleness in the PR queue

| Noise class | Count | Notes |
|---|---:|---|
| Draft PRs | 77 (7.5%) | incl. 4 do-not-merge mobile-relay review slices, docker-isolation drafts |
| Bot `pr-bug-scan` fix-PRs | 58 (5.7%) | app/buf0-bot; most depend on unmerged parent branches — skip, but 2–3 point at real defects (#7482, #7364) |
| Giant feature drafts | ~5 | #8337 official custom agents (73k lines), #8549 + #7506 plugin systems, #8287 WSL-native projects, #7717 per-worktree services — track, don't port |
| Genuinely stale (untouched since May) | 45 (4.4%) | the Apr–May community backlog tail (#500–#1776) |

Overall the queue is fresh: 70% of PRs and 69% of issues were touched in July; 92% of PRs were
created in the last two months. Top authors: brennanb2025 (147), nwparker (76), buf0-bot (58),
Jinwoo-H (52), gatsby74 (51), bbingz (43).

## Obsolescence for the aterm fork

The xterm.js→aterm engine swap obsoletes remarkably little: **15 items (0.9%)** — all
xterm/WebGL internals (glyph-atlas recovery #6071, WebGL flicker #6597/#6491, hidden-terminal
WebGL bleed #8151, xterm XTVERSION leak #8340, software-renderer gating #7206, addon
containment #7004) plus two non-fork items (Homebrew signing #9282, a promo post).
Another **23 items are likely already fixed** by aterm's drain/perf work (Windows freeze and
long-session slowdown reports, paste hangs, garbled-repaint bugs #9115/#5345).

**84 items (35 issues, 49 PRs) need an aterm-side equivalent** — concentrated in the terminal
area (80 of its 138 items). Recurring aterm work themes: IME/composition (Hangul Enter guard
#8038, kitty Option-compose #9109), scrollback semantics (cold-restore replay #7364, remote
reattach #9562, scroll parking #8968), links (lifecycle, context menus #9279, clickable paths
#5024), input encoding (CSI-u/Kitty keyboard, option-as-alt #8733), clipboard (OSC52 #6186),
inline images (#7775), smooth scrolling (#9339/#7425), font fallback and ambiguous-width glyphs.

Everything else — daemon/PTY, agents, git, review, SSH/WSL, mobile, UI — **applies as-is (93%)**.

## Notable high-demand items

| Item | Signal | Ask |
|---|---|---|
| #1099 | 31 reactions (top overall) | Multi-repo workspaces / cross-repo diffs |
| #7568 | 16 reactions, P1, mockup | Multi-repo project groups with PR-aware workspaces |
| #4280 / #4501 | 15 / 4 reactions, P1 | First-class headless server mode |
| #961 / #3035 | 11 / 3 reactions | LSP support (go-to-references, codebase search) |
| #5311 | 10 thumbs, P1 | WSL projects (upstream PR #8287 in flight) |
| #4521 | 8 reactions | Git Graph view (PR exists in brennanb2025's sweep) |
| #1082 | 7 thumbs | Jujutsu workspaces |
| #6074 / #9588 | 3 thumbs + fresh dup | Multiple windows |
| #7754 | 5 thumbs | Embedded VSCode editor |
| #9352 | P0, video repro | Remote host respawns manually-closed terminal tabs |

## Best small-port candidates (root-caused, fork-shared code)

- #9015 — 11x SSH warm-connect perf win; #8855 — node-pty FD_CLOEXEC leak fix
- #7260 — sleep-wake blank terminals; #7456 — legacy-daemon dead input; #7722 — relay starvation
- #7948 — SSH relay recursive-watch freeze; #7774 — ELECTRON_RUN_AS_NODE env leak
- #7898 — bashrc env inheritance; #7972 — PTY delivery-queue memory creep
- #7747 — Monaco EditContext typing loss; #8662 — i18n relative-time locale (only APPROVED PR)
- #8726/#8843 — fork-repo Tasks/PR routing (matches this fork's own daily workflow)
