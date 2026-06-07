# User-story migration roadmap

The migration is organized around **what Orca does for its users**, not by porting
leaf modules bottom-up. Each slice below is a recognizable product capability;
"done" means that capability's pure logic runs on the Rust core with verbatim
tests green and a parity adapter (see [`tools/parity`](../../tools/parity/README.md)).

Orca = an **Agent Development Environment**: run a fleet of CLI coding agents in
parallel, each in its own git worktree. The nine stories below are ordered by
product centrality (which also roughly tracks dependency order). The `src/shared`
modules that implement each story come from the [port backlog](./shared-port-backlog.md);
"ported" lists the Rust modules already landed (see [ported-modules](./ported-modules.md)).

Legend: ✅ ported · ⬜ remaining (in-scope backlog) · 🔌 io-edge (inject IO) · ⏭️ out-of-scope/type-only.

---

## 1. Parallel isolated work (worktrees) — *foundation, ~mostly done*
*"Every task gets its own git worktree; no stashing or branch juggling."*
- ✅ `worktree_ownership`, `worktree_id`, `worktree_base_ref`, `branch_name_from_work`, `cross_platform_path`, `wsl_paths`, `workspace_cleanup`, `nested_repo_telemetry`, `native_file_drop`
- ⬜ `worktree-card-properties`, `external-worktree-visibility`, `filesystem-rename-collision` 🔌, `git-discard-path-safety` 🔌
- **Slice done when:** worktree create/own/clean + discard-safety run on Rust.

## 2. Fleet visibility (agent status) — *the dashboard; highest day-to-day value*
*"See which agents are working / blocked / waiting / done across all worktrees."*
- ✅ `agent_status_types` (this session), `agent_recognition`, `agent_tab_title`, `synthetic_agent_title`, `agent_notification_id`, `agent_hook_endpoint_file`, `agent_hook_relay`, `tab_title_resolution`, `stable_pane_id`
- ⬜ `agent-status-identity`, `agent-interrupt-intent`, `agent-feature-install-commands`, `agent-detection` (508 LOC, regex lookbehind → needs `fancy-regex` or hand-rolled boundaries), `agent-hook-listener` 🔌 (3043 LOC, 14-source normalizer — the big one)
- **Slice done when:** a payload from any of the 14 agent sources → normalized status, on Rust.

## 3. Run any CLI agent (BYO subscription) — *~mostly done*
*"Run Claude Code / Codex / Grok / … side-by-side with my own subscription."*
- ✅ `commit_message_agent_spec` + `_models`/`_prompt`/`_generation`/`_plan`, `pull_request_generation`, `tui_agent_selection`, `agent_kind`, `pi_agent_kind`
- ⬜ `tui-agent-config` (~30-agent config table + `isTuiAgent`), `tui-agent-startup` (launch-command quoting), `codex-auth-errors`
- **Slice done when:** agent catalog + launch-command planning run on Rust.

## 4. Terminal multiplexing — *core surface; pairs with the `aterm` engine*
*"Ghostty-class terminal: tabs, panes, splits."*
- ✅ `terminal_surface_id`, `terminal_tab_id`, `terminal_fonts`, `terminal_color_scheme_protocol`, `terminal_stream` (relay framing)
- ⬜ `terminal-quick-commands` (15 tests), `workspace-session-terminal-buffers` (7), `pty-session-id-format`, `terminal-ligatures`, `terminal-session-state-save-failure`
- **Owner-gated companion:** the `aterm` Rust terminal engine (empty scaffold today).
- **Slice done when:** session/scrollback/quick-command logic on Rust; engine is a separate track.

## 5. Review & ship — *richest remaining subsystem*
*"Review AI diffs, annotate, commit, open PRs — without leaving Orca."*
- ✅ `git_remote_error`, `git_cquoted_path`, `git_push_target`, `effective_upstream`, `git_upstream_status`, `github_pr_merge_methods`, `gitlab_pipeline_checks`, `hosted_review_queue`, `hosted_review_refs`, `base_ref_search_result`
- ⬜ **git-history subsystem:** `git-history-log-parser` → `git-history-graph` (6) → `git-history` 🔌 (+ co-port `git-history-types`)
- ⬜ `source-control-ai` (21 tests, 863 LOC — SC-AI settings/migration/precedence), `git-uncommitted-line-stats` 🔌 (15), `git-branch-cleanup` 🔌, `git-rebase-source` 🔌, `diff-comments-format`, `binary-buffer`, `github-project-group-sort`
- **Slice done when:** history graph + diff/review/commit-AI logic on Rust.

## 6. Task / issue integration — *partly done*
*"Link GitHub / Linear / Jira / GitLab work items to worktrees."*
- ✅ `task_providers`, `task_query`, `linear_links`, `gitlab_projects`
- ⬜ `work-items`, `github-project-group-sort`; most provider shapes are ⏭️ type-only.
- **Slice done when:** provider work-item fetch-limits + grouping/sort on Rust.

## 7. Remote & mobile — *transport-heavy; mostly io-edge*
*"Run agents over SSH; monitor/steer from my phone over an E2EE channel."*
- ✅ `pairing`, `nacl_box` (NaCl box E2EE), `tailnet_address`, `terminal_stream`
- ⬜ `runtime-rpc-envelope`, `runtime-rpc-call-queue` (4), `runtime-rpc-feature-interaction-source`, `remote-runtime-request-frames`, `remote-workspace-session-projection` (3), `ssh-pty-id`, `runtime-environments`, `runtime-bootstrap`, `runtime-client-events`
- ⬜ 🔌 `remote-runtime-client` (5), `remote-runtime-request-connection`, `remote-runtime-request-websocket`, `secure-file` (3), `runtime-environment-store`
- **Slice done when:** RPC framing/queue + session projection on Rust (clients stay io-edge).

## 8. Automations — *self-contained, needs a date-time dep*
*"Run agent workflows on a schedule or trigger."*
- ⬜ `automation-schedules` (19 tests; RRULE + cron — will vendor `chrono`/`time`), `automation-precheck`
- **Slice done when:** schedule parse/validate/next-occurrence on Rust.

## 9. Browser / design mode — *self-contained protocols*
*"Embedded browser; annotate, grab elements, screencast."*
- ⬜ `browser-screencast-protocol` (7, binary framing), `browser-grab-types` (9), `browser-viewport-presets`, `browser-annotation-viewport-bridge`
- **Slice done when:** screencast framing + grab budgets/redaction on Rust.

## Cross-cutting platform/infra (serves every story)
- ✅ `mcp`, `repo_icon`, `project_groups`, `workspace_statuses`, `network_proxy`, `protocol_compat`, `quick_open_filter`, `skill_metadata`, `feature_wall_tour_depth`, `hook_command_source_policy`, `uri_component`, `setup_runner_command`, `setup_script_*`
- ⬜ `workspace-session-schema`, `setup-script-imports`, `app-icon`, `telemetry-events` (32; new `orca-telemetry` crate), onboarding (`feature-interactions` ✅, `feature-tips`, `contextual-tours`, `feature-education-telemetry`, `feature-wall-*`), `file-uri-path`, `string-utils`, `mobile-markdown-document`, misc.

---

## Recommended execution order

Logic-layer slices first (Rust core), then the UX replacement (native UI over that core).

1. **Finish Story 2 (fleet visibility)** — highest day-to-day value; the agent-status surface is what users stare at. Tackle `agent-status-identity` → `agent-detection` → `agent-hook-listener` (the io-edge normalizer).
2. **Story 5 (review & ship)** — git-history subsystem, then `source-control-ai`. Core to the "ship in-app" promise.
3. **Story 4 (terminal)** logic + seed the `aterm` engine track (owner-gated).
4. **Story 3 finish** (`tui-agent-config`/`-startup`) — small, completes "run any agent".
5. **Story 7 (remote/mobile)** pure RPC framing/queue; **Story 8/9** as self-contained wins.
6. **Then the real Electron replacement:** native per-platform UI shells (SwiftUI on macOS) over the Rust core — the largest lift, started only as a terminal vertical slice today.

Each increment: port → verbatim tests green offline → parity adapter (`tools/parity`) →
update [ported-modules](./ported-modules.md) + [backlog](./shared-port-backlog.md) progress.
