# Ported Modules Ledger

Tracks the TS→Rust port at module granularity so the migration is continuable.
Authoritative inventory of what each subsystem does lives in `functional-map.md`.

## The per-module pattern

1. Read the `src/shared/<mod>.ts` source **and** its `.test.ts`.
2. Port the logic to `rust/crates/orca-core/src/<mod>.rs`, faithful to behaviour.
3. Translate the original test cases **verbatim** into a `#[cfg(test)]` module.
4. `cargo test` + `cargo clippy` green; keep the core zero-dep, `forbid(unsafe)`,
   panic-free (so Trust can discharge panic-safety obligations).
5. Record it here; mark the owning subsystem in `functional-map.md` when fully covered.

## `orca-config` — project/config tier (14 modules, 113 tests, clippy clean)

JSON-backed config inspection on **vendored `serde_json`** (`preserve_order`,
so servers list in file order). `mcp` ports `inspectMcpConfigContent` +
`summarizeMcpServer` from `mcp-config.ts`: parse the config JSON, extract the
servers object at the candidate path, summarize each server's transport
(stdio/http/unknown) + status (enabled/disabled/invalid), masking sensitive env
via `orca-text::mcp_env`. JSON is the shared format for configs and IPC, so this
unblocks a broad class of future ports.

| Rust module | Source | Notes |
| --- | --- | --- |
| `mcp` | `mcp-config.ts` (inspect/summarize) | JSON config → server summaries; invalid-JSON handling without leaking contents |
| `setup_script_package_manager` | `setup-script-package-manager-suggestion.ts` | package.json `packageManager` + lockfile-family detection → install-command candidate; ambiguous/multi-family → none; file reads/exists injected |
| `repo_icon` | `repo-icon.ts` | repo-icon sanitize (lucide/emoji/image; reject unsafe URLs, oversized data URLs; tri-state undefined/reset/icon) + favicon/GitHub-avatar builders (hand-rolled URL parse) |
| `pi_overlay_ui_settings` | `pi-overlay-ui-settings.ts` | merge user Pi settings while force-overriding Orca-only safety (`terminal.clearOnShrink`, `hideThinkingBlock`); tolerates malformed shapes |
| `project_groups` | `project-groups.ts` | create/normalize project groups (persisted-JSON normalize, dedupe, parent-cleanup, sort), clear dead memberships, subtree-id collection, next-order; id/clock injected |
| `workspace_statuses` | `workspace-statuses.ts` (+`-defaults`/`-default-migration`) | status-column normalize (sanitize id/label/color/icon, dedupe, cap) + one-shot legacy-default-visual + reversed-order migrations; clamp board width/opacity; group-key encode/decode |
| `feature_interactions` | `feature-interactions.ts` | 37-id feature-interaction catalog + `normalizeFeatureInteractions`/`hasFeatureInteraction` over untrusted persisted JSON (drop unknown ids, reject non-finite/negative `firstInteractedAt`, integer>0 `interactionCount` else 1). Reassigned from orca-core (needs `serde_json::Value`). TS repo-writer meta-test skipped (asserts the TS app, not this logic) |

## `orca-agents` — agent-CLI tier (11 modules, 115 tests, clippy clean)

Seeds the agent-CLI domain (commit-message generation, provider specs, output
parsing). `commit_message_prompt` ports `commit-message-prompt.ts`: the base
prompt assembly + diff truncation, agent-output cleanup (fence/preamble/list-
marker stripping), a POSIX-style custom-command **tokenizer** (quotes + escapes,
no shell expansion) → spawn-ready binary/argv with `{prompt}` substitution, and
**error extraction** from noisy agent stdout/stderr (ANSI strip, last-`ERROR:`
JSON payload, wrapped `Error code:` quoted-message). Over **vendored `regex` +
`serde_json`**.

| Rust module | Source (`src/shared/`) | Notes |
| --- | --- | --- |
| `commit_message_prompt` | `commit-message-prompt.ts` | prompt build + diff truncate, `cleanGeneratedCommitMessage`, `tokenizeCustomCommandTemplate` + `planCustomCommand`, `extractAgentErrorMessage` (JSON + `Error code:` payloads) |
| `tui_agent_selection` | `tui-agent-selection.ts` | agent auto-pick (catalog fallback order), blank preference, disabled-agent normalize/filter; agents keyed by id (catalog = auto-pick order) |
| `commit_message_models` | `commit-message-agent-spec.ts` (parser half) | model-discovery parsers: Codex JSON, one-per-line, Pi whitespace table, Cursor `id - Label`; label/thinking-level derivation, dedupe |
| `commit_message_agent_spec` | `commit-message-agent-spec.ts` (spec half) | 8-agent spec table (binary/prompt-delivery/`buildArgs`/model catalog/dynamic discovery) + lookups, `resolveCommitMessageAgentChoice` (uses `tui_agent_selection`), capability views (no spawn details), dynamic-model synth |
| `pull_request_generation` | `pull-request-generation.ts` | PR-fields prompt build (reuses `truncate_diff_for_prompt`) + fence-tolerant JSON parse with current-field fallbacks (base/title/body/draft) |
| `commit_message_generation` | `commit-message-generation.ts` | commit-draft prompt from staged context + split generated text into subject/body (reuses `clean_generated_commit_message`/`truncate_diff_for_prompt`) |
| `commit_message_plan` | `commit-message-plan.ts` | agent+prompt → spawn-ready binary/argv/stdin; custom-command path, command-override prefix, model/thinking validation, dynamic-model acceptance (composes spec lookups + tokenizer) |
| `agent_status_types` | `agent-status-types.ts` (parser half) | untrusted agent-status payload → lean `ParsedAgentStatusPayload`: state allow-list (`working`/`blocked`/`waiting`/`done`), per-field trim + line collapse (single-line vs paragraph-preserving), strict-`true` `interrupted` gated on `done`, **UTF-16-safe truncation** that drops a trailing lone high surrogate |

## `orca-net` — network tier (1 module, 6 tests, clippy clean)

Seeds the network tier (proxy now; HTTP clients + rate limiting later). std-only,
zero-dependency, IO-free: it computes proxy configuration that higher tiers (PTY
env, HTTP dialers) consume. `network_proxy` ports `network-proxy.ts`, replacing
the WHATWG `URL` parse with a targeted proxy-URL parser (proxy URLs are
`scheme://[user[:pass]@]host[:port]` and the only output is `scheme://[auth@]host`,
so paths/default-port-dropping/IDNA aren't needed).

| Rust module | Source (`src/shared/`) | Notes |
| --- | --- | --- |
| `network_proxy` | `network-proxy.ts` | proxy URL normalize (protocol allowlist, host required, strips path/query/fragment) + redact creds; env precedence (`HTTPS_PROXY`→…→`http_proxy`, `NO_PROXY`/`no_proxy`); bypass-rule normalize; child-process proxy env build |

## `orca-crypto` — E2EE tier (1 module, 5 tests, clippy clean)

NaCl `box` for the encrypted remote-runtime transport, on **vendored
`crypto_box`** (X25519 + XSalsa20-Poly1305; 20-crate pure-Rust stack incl.
`curve25519-dalek` + `fiat-crypto`, built offline). `nacl_box` ports
`e2ee-crypto.ts` (which used `tweetnacl`): keypair-from-seed, shared-box
precompute (`box.before`), and seal/open with the `nonce || tag || ciphertext`
bundle. Nonces/seeds are caller-injected (the IO edge owns the OS RNG), so the
crate vendors `crypto_box` **without `getrandom`** and stays deterministic.

The TS module shipped with **no tests**; the port is gated on the **canonical
NaCl `box` test vector**, so parity is *byte-for-byte* wire-compatibility with
`tweetnacl` (the property mobile/CLI pairing actually depends on) — a stronger
guarantee than the original had.

| Rust module | Source (`src/shared/`) | Notes |
| --- | --- | --- |
| `nacl_box` | `e2ee-crypto.ts` | X25519 keypair-from-seed + shared-box precompute + seal/open; canonical NaCl `box` KAT (`tweetnacl` wire-compat), peer interop round-trip, tamper/short/bad-length rejection |

## `orca-relay` — remote/mobile transport tier (4 modules + base64, 46 tests, clippy clean)

The remote/mobile transport (replaces the `ws`-based relay). `terminal_stream`
is the binary framing it multiplexes terminal traffic over; `pairing` is the
deep-link handshake that bootstraps the session; `e2ee_channel` is the encrypted
session itself, over `orca-crypto`. A private `base64` module (standard +
url-safe) backs both protocols. JSON over **vendored `serde_json`**;
`#![forbid(unsafe_code)]`, panic-free (Trust-ready).

`e2ee_channel` is ported as a **pure reducer**: every input returns a list of
`E2eeEffect`s the transport owner executes (`SendText`/`SendBinary`/`Deliver*`/
`Ready`/`Error`), and the WebSocket, the handshake timer, and the nonce RNG are
**injected at the edge** (same boundary pattern as `orca-git`'s `GitRunner`) — so
the handshake state machine is fully unit-testable with no IO.

| Rust module | Source | Notes |
| --- | --- | --- |
| `terminal_stream` | `terminal-stream-protocol.ts` | frame encode/decode (10 opcodes: output/snapshot×3/resized/error/input/resize/subscribe/unsubscribe), text + JSON payloads; rejects bad version/opcode |
| `pairing` | `pairing.ts` | `orca://pair?code=` deep-link encode/decode + paste-pair parse; minimal `orca://` URL parse (exact host/path route) + offer schema (`v`=2, non-empty fields) replacing zod |
| `e2ee_channel` | `runtime/rpc/e2ee-channel.ts` | NaCl-box handshake state machine (hello→auth→ready) + transparent encrypt/decrypt; token-auth + nonce RNG injected; consecutive-decrypt-failure cap, handshake timeout, destroy-safety. 16 cases (the 1 cross-compat sanity case lives in `orca-crypto`'s interop test) |
| `base64` (priv) | — | standard (`+/=`) + url-safe-no-pad encode, lenient decode; shared by pairing + e2ee_channel |

## `orca-core` — done (49 modules, 271 tests, clippy clean)

| Rust module | Source (`src/shared/`) | Notes |
| --- | --- | --- |
| `cross_platform_path` | `cross-platform-path.ts` | path containment/resolution, POSIX+Windows+UNC |
| `git_cquoted_path` | `git-cquoted-path.ts` | git C-quoted path decode (octal/named escapes) |
| `worktree_id` | `worktree-id.ts` | worktree id parse + folder-instance suffix strip |
| `worktree_ownership` | `worktree-ownership.ts` | worktree ownership classify (orca-managed/unknown-legacy/external) + external-visibility policy + known-layout building; **composes `cross_platform_path` + `wsl_paths`** (Windows-casing & WSL-aware) |
| `worktree_base_ref` | `worktree-base-ref.ts` | `git worktree add` ref qualification |
| `wsl_paths` | `wsl-paths.ts` | `\\wsl.localhost\` / `\\wsl$\` UNC parsing |
| `repo_badge_color` | `repo-badge-color.ts` (+`constants.ts`) | hex colour normalise/expand/validate |
| `git_push_target` | `git-push-target-validation.ts` | remote/branch/URL safety (anti-traversal) |
| `gitlab_projects` | `gitlab-projects.ts` | GitLab recents list: most-recent-first, dedupe by host+path, cap at 10 (clock injected as ISO string) |
| `gitlab_pipeline_checks` | `gitlab-pipeline-checks.ts` | GitLab pipeline jobs → provider-neutral `PRCheckDetail` status/conclusion (manual→neutral, scheduled/waiting→queued+pending); shares the Checks panel with GitHub |
| `branch_name_from_work` | `branch-name-from-work.ts` (+`marine-creatures.ts`) | slug sanitise, creature-name detection, prompt build |
| `browser_search` | `browser-url.ts` (search heuristics) | search-vs-URL detection + per-engine search-URL building (Google/DuckDuckGo/Bing/Kagi) |
| `marine_creatures` | `marine-creatures.ts` | 552-entry name corpus (data table) |
| `native_file_drop` | `native-file-drop.ts` | OS file-drop routing by event target path (terminal/editor/composer/sidebar/file-explorer), internal-drag rejection, fail-closed explorer dir |
| `nested_repo_telemetry` | `nested-repo-telemetry.ts` | nested-repo scan/import funnel payloads: count cap+bucket, scan/import outcome classification, UUIDv4 attempt-id (random bytes injected), all-selected from raw counts |
| `tab_title_resolution` | `tab-title-resolution.ts` | tab title/label priority resolution |
| `base_ref_search_result` | `base-ref-search-result.ts` | legacy remote-ref → local branch derivation |
| `github_pr_merge_methods` | `github-pr-merge-methods.ts` | PR merge-method ordering/labelling |
| `stable_pane_id` | `stable-pane-id.ts` | UUID leaf-id validation + pane-key build/parse |
| `setup_runner_command` | `setup-runner-command.ts` | cross-platform setup-runner shell command: bash (POSIX/`/`-paths), WSL UNC→Linux-path rewrite, `cmd.exe /c` for Windows; POSIX/Windows arg quoting |
| `setup_script_telemetry` | `setup-script-telemetry.ts` | setup-script prompt funnel payloads: count→bucket (0/1/2-3/4+), import-vs-configure mode, provider-only (no raw details), action + edited-before-save |
| `feature_wall_tour_depth` | `feature-wall-tour-depth.ts` | onboarding tour depth telemetry: workflow+substep → canonical ordered depth step, furthest-step + visited/completed counts |
| `agent_kind` | `agent-kind.ts` | TuiAgent ↔ telemetry AgentKind mapping |
| `agent_hook_endpoint_file` | `agent-hook-endpoint-file.ts` | parse `endpoint.env`/`endpoint.cmd` hook handshake files (POSIX `KEY=value` + Windows `set KEY=value`), `=`-in-value preservation, required-field check |
| `agent_notification_id` | `agent-notification-id.ts` | deterministic notification dedupe id from worktree/pane/state-start (percent-encoded, truncated ts); `None` on missing field or non-finite timestamp |
| `agent_recognition` | `agent-name-token-match.ts` + `agent-process-recognition.ts` | whole-token agent-name matching (hand-rolled boundaries, no regex lookbehind) + process-name normalization/expected-match |
| `pty_env` | `pty/{terminal-color-env,wsl-orca-env,codex-home-wsl-env}.ts` | PTY env construction (NO_COLOR strip, WSLENV interop, Codex-home flavor) |
| `terminal_fonts` | `terminal-fonts.ts` | font-weight clamp + bold derivation |
| `synthetic_agent_title` | `synthetic-agent-title.ts` | agent terminal-state title synthesis |
| `open_in_applications` | `open-in-applications.ts` | "open in app" list normalise/dedup/cap |
| `protocol_compat` | `protocol-compat.ts` | runtime/mobile protocol compat verdicts |
| `protocol_version` | `protocol-version.ts` | protocol version constants + capabilities |
| `commit_message_host_key` | `commit-message-host-key.ts` | model-discovery host-key namespacing |
| `git_upstream_status` | `git-upstream-status.ts` | patch-equivalence + force-push-with-lease decision |
| `hook_command_source_policy` | `hook-command-source-policy.ts` | normalize/resolve hook source policy (local-only/run-both/shared-only); absent-vs-invalid distinction, legacy fallback to shared-only |
| `hosted_remote_url` | `git/hosted-remote-url.ts` | provider-neutral remote-URL parse (https/ssh/scp/shorthand) + GitHub/GitLab/Bitbucket file-URL build (hand-rolled percent en/decode) |
| `linear_links` | `linear-links.ts` | Linear team/settings URL builders (percent-encoded segments) + workspace url-key extraction from issue URLs (host/first-path-segment parse) |
| `hosted_review_queue` | `hosted-review-queue.ts` | provider-neutral review classification (mine/requested/agent/teammate), needs-response + ready-to-merge gates (GitHub merge-state blockers scoped to GitHub); hand-rolled UTC ISO-8601→epoch parser (no date crate) |
| `hosted_review_refs` | `hosted-review-refs.ts` | git ref → branch name: strip `refs/heads/`, `refs/remotes/<remote>/`, and `origin/`/`upstream/` (base refs) |
| `tailnet_address` | `tailnet-address.ts` | Tailscale `100.64.0.0/10` IPv4 detection (octet parse + range check) for phone pairing |
| `quick_open_filter` | `quick-open-filter.ts` | Quick Open blocklist, exclude prefixes (POSIX + Windows `path.relative`), rg/git arg builders, rg line normalisation |
| `uri_component` | (extracted from `hosted-remote-url.ts`) | `encodeURIComponent`/`decodeURIComponent` equivalents, shared by URL/id builders (malformed-escape passthrough) |
| `terminal_surface_id` | `terminal-surface-id.ts` | host `tab::leaf` ↔ `:`-safe `web-terminal-<encoded>` tab id (percent-encode the `::` separator), prefix detection |
| `terminal_tab_id` | `terminal-tab-id.ts` | tab-id validity (non-empty, no `:`) + host-tab exclusion of web-terminal surface ids |
| `task_providers` | `task-providers.ts` | provider-neutral (GitHub/GitLab/Linear/Jira) visible-list + default-source normalization, runtime-availability filtering, always ≥1 valid source |
| `task_query` | `task-query.ts` | GitHub-style task search: quote-aware tokenizer, parse→scope/state/draft/assignee/author/review/labels/free-text, serialize (round-trip), single-filter edit, `repo:` stripping for cross-repo fan-out |
| `workspace_cleanup` | `workspace-cleanup.ts` | cleanup classification (ready/review/protected) + queue/select/force-remove policy, idle/archived inactivity reasons, dismissal fingerprint |

## `orca-text` — done (regex-backed; 6 modules, 37 tests, clippy clean)

Pure logic that needs a regex engine. Depends on the **vendored** `regex`
(see "Vendoring" below). Separated from `orca-core` only to keep that crate
zero-dependency.

| Rust module | Source (`src/shared/`) | Notes |
| --- | --- | --- |
| `git_remote_error` | `git-remote-error.ts` | credential-URL scrubbing, error normalisation, `isNoUpstreamError` |
| `mcp_env` | `mcp-config.ts` (`maskMcpEnv`) | mask sensitive MCP env values by credential-ish key or token-shaped value |
| `pi_agent_kind` | `pi-agent-kind.ts` | Pi vs OMP launch-command detection; word-boundary regex (no `pip`/`mpi`/`comp` false-positives), path-aware, case-insensitive |
| `skill_metadata` | `skill-metadata.ts` | skill markdown → `{name, description}`: minimal YAML frontmatter parse (scalars/quotes/`-` lists/`\|`/`>` block scalars) with first-heading + first-paragraph fallback |
| `agent_tab_title` | `agent-tab-title.ts` | prompt → short tab title: first clause, leading-filler/markup/link/punctuation strip, `\p{L}`/`\p{N}` cleanup, capitalize, word-boundary truncate (needs `unicode-gencat`) |
| `workspace_name` | `workspace-name.ts` | git-ref-safe slugify + work-item intent name (action detection w/ `[^a-z0-9_-]` boundaries so slugs aren't mistaken for actions, compact title, Linear/Jira identity), create-name resolve |

## `orca-git` — IO tier (21 modules, 113 tests, clippy clean)

Git logic generic over a `GitRunner` boundary (`runner.rs`): real
`ProcessGitRunner` shells the user's `git` via `std::process` (Orca's current
approach; a vendored `gitoxide` backend can replace it behind the trait). Tests
run against closure / sequential mock runners — the same shape as the TS
`gitExecFileAsync` mocks. Depends on `orca-core` + `orca-text`.

| Rust module | Source | Notes |
| --- | --- | --- |
| `runner` | `git/runner.ts` (contract) | `GitRunner` trait, `GitOutput`/`GitError`, `ProcessGitRunner`, `Fn` blanket impl |
| `fetch_error_classification` | `git/fetch-error-classification.ts` | missing-remote-ref detection |
| `check_ignored_paths` | `git/check-ignored-paths.ts` | chunked `check-ignore` + exit-1 handling + dedup |
| `branch_rename` | `git/branch-rename.ts` | `branchHasUpstream`, collision-suffix resolution, `branch -m` (complete) |
| `push_target` | `git/push-target-validation.ts` | `GitPushTarget` + shape/`check-ref-format` validation |
| `effective_upstream` | `shared/git-effective-upstream.ts` | resolve `@{u}` + legacy same-name-origin fixup; ahead/behind |
| `publish_target_status` | `shared/git-publish-target-status.ts` | ahead/behind vs an explicit `remote/branch` |
| `upstream` | `git/upstream.ts` | full upstream-status engine (composes the above; full test suite) |
| `remote` | `git/remote.ts` | push / pull / fast-forward / fetch / rebase-from-base (configured + explicit targets; error normalisation) — complete |
| `rebase_source` | `shared/git-rebase-source.ts` | base-ref → remote/branch (longest-match remote) |
| `status_parse` | `git/status.ts` (parsers) | porcelain-v2 status-char, conflict-kind, branch-ahead/behind parsing |
| `status` | `git/status.ts` (getStatus core) | full porcelain-v2 parse → entries/branch/upstream/ignored; type-1/2, untracked, ignored, unmerged conflicts (fs-exists injected) |
| `worktree` | `git/worktree.ts` (parseWorktreeList) | `git worktree list --porcelain` parse (line + NUL `-z`, bare/sparse/detached, main detection) |
| `repo_clone_path` | `git/repo-clone-path.ts` (pure) | clone-destination validation (absolute + anti-traversal) + WSL comparison key; platform-parameterized |
| `branch_cleanup` | `shared/git-branch-cleanup.ts` | worktree-deletion safety: target-ref gathering, non-fatal remote refresh, unmerged-changes detection (tree-equal merge / merge-only / patch-equivalent) |

## `orca-store` — persistence tier (1 module, 4 tests, clippy clean)

Thin synchronous SQLite adapter, the native replacement for
`src/main/sqlite/sync-database.ts` (which wraps Electron's `node:sqlite`).
Backed by **vendored, bundled SQLite** — the C amalgamation compiles offline
via `cc`, no system SQLite.

| Rust module | Source | Notes |
| --- | --- | --- |
| `database` | `sqlite/sync-database.ts` | open (file/memory, read-only, `file_must_exist`), `exec`, pragma get/set, connection access |

## `orca-pty` — local PTY tier (1 module, 2 tests, clippy clean)

Native PTY spawning, the replacement for `node-pty`. Backed by **vendored
`portable-pty`**. `PtySession` mirrors the node-pty surface: spawn
`(program, args, {cwd, env, cols, rows})`, stream output via a reader, `write`,
`resize`, `process_id`, `kill`, `wait`. Tests spawn a **real PTY child** and
assert its streamed output (offline).

| Rust module | Source | Notes |
| --- | --- | --- |
| `session` | `node-pty` usage (rate-limits/runtime `pty:spawn`) | open/spawn/read/write/resize/kill/wait over `portable_pty` |

## `orca-terminal` — headless terminal engine (2 modules, 19 tests, clippy clean)

The foundation of the `@xterm/headless` replacement
(`daemon/headless-emulator.ts`): a server-side grid + cursor driven by the
**vendored `vte` ANSI parser**, tracking cwd via OSC-7, with **snapshot/restore
and resize** (the reconnect/SSH-replay role of `@xterm/addon-serialize`).
Implemented subset: print, CR/LF/BS/HT, line scroll, **bounded scrollback**
(default 5000 lines), OSC-7 cwd (percent-decoded), `TerminalSnapshot`
capture/restore, resize, and **per-cell SGR attributes**
(bold/italic/underline/inverse + a full `Color` model: 16-color, bright,
256-palette `38;5;n`, and truecolor `38;2;r;g;b` — both `;` and `:` forms), and
**mouse-reporting modes** (DECSET 9/1000/1002/1003 tracking + 1006/1016 SGR,
tracked for remote replay). The full `aterm` engine extends this further
(selection/copy, full DECSET set, hyperlinks).

| Rust module | Source | Notes |
| --- | --- | --- |
| `headless` | `daemon/headless-emulator.ts` (subset) | grid of `Cell{ch,attrs}` over `vte::Parser`+`Perform`; OSC-7 cwd; SGR attrs; snapshot/restore; resize |
| `color_scheme_protocol` | `terminal-color-scheme-protocol.ts` | DEC mode 2031 / CSI 997 color-scheme: reply-sequence build, theme/system resolution, subscribe/unsubscribe scan with cross-chunk tail carry (vendored `regex`, literal/class only) |

## `orca-ffi` — native FFI boundary (1 module, 5 tests, clippy clean)

The stable **C ABI** the thin native wrappers (SwiftUI on macOS, etc.) link
against — the keystone connecting the Rust core to platform shells. Two
surfaces: (1) headless **terminal** — create/process/row-text/cursor/resize/
size/free + **per-cell render data** (`orca_terminal_cell` → `OrcaCell{ch,
bold/italic/underline/inverse, fg/bg as default|indexed|truecolor}`); and (2)
**live session** — `orca_session_spawn`/wait/write/resize/size/cursor/row-text/
cell/free, spawning a real PTY whose output streams into the terminal. The
session FFI test spawns a shell and reads its grid through the ABI. Builds as **`staticlib` + `cdylib`** (`liborca_ffi.a` /
`liborca_ffi.dylib`) with a hand-written C header at
`rust/crates/orca-ffi/include/orca.h`. This is the one crate not under
`forbid(unsafe_code)` — `unsafe` is confined to the FFI boundary, each `unsafe
fn` documenting its contract. Tests exercise the C ABI exactly as a wrapper
would (incl. null-pointer tolerance).

| Rust module | Source | Notes |
| --- | --- | --- |
| `lib` (C ABI) | new boundary | `orca_terminal_*` + `orca_string_free` + `orca_ffi_version`; `orca.h` |

## Native shell — `native/orca-macos` (Swift + SwiftUI, builds & runs)

The thin macOS wrapper (the owner's original ask). A SwiftPM package links the
vendored Rust core through the C ABI: `OrcaTerminal` (Swift) → `COrca` (module
map over `orca.h`) → `liborca_ffi.a` → vendored `vte`. The `orca-smoke`
executable drives the core end-to-end and **passes**:

```
(cd rust && cargo build -p orca-ffi) && (cd native/orca-macos && swift run orca-smoke)
# → OK — Swift shell drove the Rust core (grid, cursor, OSC-7 cwd, resize); core v0.0.1
```

`OrcaKit` exposes typed `TerminalCell`/`CellColor` (incl. truecolor) via
`cell(row:col:)` + `size()`. The smoke verifies grid, cursor, OSC-7 cwd, resize,
and per-cell SGR/truecolor through the ABI.

**`OrcaUI` (SwiftUI) renders it.** `TerminalView` and `SessionTerminalView` draw
the Rust core's grid — monospaced cells with bold/italic/underline, inverse, and
the 16/256/truecolor palette mapped to SwiftUI `Color` — through `OrcaKit`.
Compiles against the macOS SDK.

**The full live path runs.** `OrcaKit.OrcaSession` spawns a real shell command
in a PTY via the FFI; `swift run orca-smoke` verifies output streams all the way
back to Swift:

```
SessionTerminalView (SwiftUI) → OrcaKit.OrcaSession → orca.h (C ABI)
        → liborca_ffi → orca-session → PTY + orca-terminal → vendored vte
```

**Windowed app (`OrcaApp`, `@main`) builds.** A SwiftUI `App` spawns the user's
`$SHELL` in a live PTY session, renders it via `SessionTerminalView` on a redraw
tick, and forwards key input (incl. return/tab/arrows/escape) to
`session.write`. `swift build` compiles all targets (OrcaKit, OrcaUI, OrcaApp,
OrcaSmoke) against the macOS SDK.

So a functional native terminal — windowed SwiftUI app → live PTY → Rust VT
engine — exists end-to-end. Packaging it into a signed `.app` bundle (Info.plist
+ codesign) is the remaining distribution step.

## `orca-runtime` — orchestration tier (1 module, 7 tests, clippy clean)

The multi-agent coordination store, ported from
`src/main/runtime/orchestration/db.ts` onto `orca-store`'s vendored SQLite. Full
schema (messages, tasks, dispatch_contexts, decision_gates, coordinator_runs +
indexes + CHECK constraints) verbatim. Operations: message send/inbox/mark-read,
task create/list/update/get, **dispatch contexts** (ready-gated dispatch,
one-active-per-assignee guard, failure-count carry-forward, complete), and
**decision gates** (create/list/resolve), and **coordinator runs**
(create/update/active-lookup, terminal states stamp `completed_at`). All five
schema tables now have operations. Tests run against in-memory SQLite (incl.
CHECK-constraint + state-transition enforcement).

| Rust module | Source | Notes |
| --- | --- | --- |
| `orchestration` | `runtime/orchestration/db.ts` | schema + messages + tasks + dispatch contexts + decision gates + coordinator runs (all 5 tables) |

## `orca-ssh` — SSH tier, started (1 module, 11 tests, clippy clean)

OpenSSH config parsing ported from `ssh-config-parser.ts`: `parse_ssh_config`
handles Host blocks (single + multi-pattern, wildcard/negation/pattern-only
skipping), scalar directives (hostname/port/user/identity*/proxy*), quoted
values + inline comments, `=`-form, case-insensitive keywords, `Match`
block-termination, and `~` expansion (POSIX + Windows separators, parameterized
on `home` for purity). The transport (a vendored SSH crate behind a connection
boundary, like `orca-git`'s runner) is the next step.

| Rust module | Source | Notes |
| --- | --- | --- |
| `config_parser` | `ssh/ssh-config-parser.ts` | `parse_ssh_config` → `SshConfigHost[]` (pure; `home`-parameterized) |

## `orca-session` — live terminal session (1 module, 2 tests, clippy clean)

Composes `orca-pty` + `orca-terminal`: spawns a PTY, runs a background reader
thread streaming the child's output into a shared `Mutex<HeadlessTerminal>`, and
exposes write/resize + grid access for rendering. This is the unit the UI drives
(and what the FFI/Swift app will spawn). Tests spawn a real shell command and
assert the streamed grid content.

| Rust module | Source | Notes |
| --- | --- | --- |
| `session` | runtime `pty:spawn` + headless emulator wiring | PTY spawn → reader thread → headless terminal; write/resize/grid access |

## Vendoring (done — three dependency modes proven)

All third-party crates are vendored in-tree under `rust/vendor/` (87 crates),
pinned by `rust/Cargo.lock`, with `rust/.cargo/config.toml` redirecting
crates.io → `vendor/` and `[net] offline = true`. **Builds are offline by
construction** across all three dependency modes:

1. **pure-Rust** — `regex` (+`regex-automata`, `regex-syntax`, `aho-corasick`, `memchr`); `vte` (+`utf8parse`, `arrayvec`); `serde_json` (+`serde`, `itoa`, `ryu`, `indexmap`); `crypto_box` (+`curve25519-dalek`, `crypto_secretbox`, `salsa20`, `poly1305`, `aead`, `fiat-crypto`, `subtle`, `zeroize` — 20 crates, the NaCl-box E2EE stack, no `getrandom`).
2. **native C via `cc`** — `rusqlite` + `libsqlite3-sys` `bundled` (SQLite C amalgamation compiled in-tree, no system lib).
3. **native syscalls** — `portable-pty` (+`nix`, `libc`, `filedescriptor`; `winapi` for the Windows target).

Stripping = minimal feature sets (`default-features = false` + only what's
used): `regex` keeps `std, perf, unicode-case, unicode-perl`; `rusqlite` keeps
only `bundled`; `portable-pty` drops `ssh`/serde; `crypto_box` keeps only
`alloc, salsa20` (drops `getrandom`/`std`/`serde`). (Cross-platform vendoring
includes Windows-only crates like `winapi`; physical pruning of unused-target
source is a later refinement.)

## Next-up queue

- **orca-text (regex tier):** `text-search.ts` (rg/git `--json` parsing),
  `agent-tab-title.ts` (add the `unicode-gencat` feature for `\p{L}`/`\p{N}`).
- **orca-core (pure tier):** `color-validation.ts`,
  `workspace-space-compaction.ts`, `composer-branch-selection.ts`.
  (`project-groups.ts` + `workspace-statuses.ts` chain landed in `orca-config`.)
  (`gitlab-pipeline-checks.ts`, `gitlab-projects.ts`, `hosted-review-queue.ts`,
  `linear-links.ts`, `task-providers.ts`, `terminal-tab-id.ts` +
  `terminal-surface-id.ts` landed; `pi-agent-kind.ts` landed in `orca-text`.
  `git-history-boundary-rows.ts` deferred — untested UI graph-model logic.)
- **orca-git (in progress):** remote ops complete; `hosted-remote-url.ts` landed
  in `orca-core` (hand-rolled URL parse/build). Next: `repo-clone-path.ts`, then
  the larger `status.ts` / `worktree.ts` / `repo.ts`.
- **orca-crypto (started):** NaCl `box` done (tweetnacl-wire-compatible).
  Vendored Curve25519/XSalsa20-Poly1305 stack now unblocks the relay's
  encrypted session and the SSH transport's key handling.
- **orca-relay (started):** terminal binary-stream framing + pairing handshake
  + E2EE channel state machine done (over `orca-crypto`). Next: the multiplex
  registry and wiring the channel reducer to a concrete WebSocket transport.
- **orca-pty (started):** add the IO-mixed `shell-startup-env.ts` +
  `windows-environment-path.ts` over injectable file/exec readers.
- **orca-net (started):** proxy settings done (std-only). Next: HTTP client +
  rate limiting (will vendor a stripped HTTP/TLS stack).
- **orca-agents:** commit-message generation ported **end-to-end** (spec table +
  model parsers + prompt + generation + plan + PR generation + tui-agent
  selection); plus `agent_status_types` (untrusted status-payload
  parse/normalize). Next agent-domain candidates: `tui-agent-config` catalog
  (fuller `is_tui_agent`), the agent-status **rendering/derivation** half
  (label/icon/state-machine consumers of `ParsedAgentStatusPayload`), or agent
  spawning/execution (IO tier).
- **Large subsystems (multi-turn):** `keybindings.ts` (1579 LOC — a ~600-line
  cross-platform definitions table + match/normalize engine, 22 tests) then
  `window-shortcut-policy.ts` (26 tests) on top; the per-provider review
  adapters `hosted-review-github.ts`/`hosted-review-gitlab.ts` (pair with the
  landed `hosted_review_queue` classifier).
- **IO tier (next crates):** `orca-store` schema/migrations (port
  `runtime/orchestration/db.ts`), `orca-ssh` (vendor an ssh crate). Each adds a
  vendored, stripped dependency.

## Regex tier (now unblocked — `regex` is vendored)

`orca-core` stays zero-dependency; modules needing a real regex engine live in
`orca-text`:

- ✅ `git-remote-error.ts` → `orca-text::git_remote_error` (done).
- `text-search.ts` — rg/git-grep `--json` parsing + submatch regex construction.
- ✅ `agent-tab-title.ts` → `orca-text::agent_tab_title` (done; enabled the
  `unicode-gencat` regex feature for `\p{L}`/`\p{N}`).
