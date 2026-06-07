# `src/shared` Rust-port backlog

**Milestone:** finish porting the entire *pure-logic* surface of `src/shared` into
the `rust/` workspace, each module carrying its `.test.ts` translated **verbatim**
and green **offline**. This is the concrete, bounded replacement for the
open-ended "convert Orca to fully native Rust" goal — it has a finite, reachable
definition of done (below) and excludes the owner-gated `aterm`/`trust` tracks.

**Status snapshot (2026-06-07).** `src/shared` has 197 source files. **78 are
already ported** (15 crates, 601 verbatim tests — see
[`ported-modules.md`](./ported-modules.md)). The **119 remaining** files were
classified by a 12-agent fan-out (read each source + its test) into:

| Category | Count | In scope? |
| --- | --- | --- |
| `pure-portable` | 60 | ✅ port + verbatim tests |
| `io-edge` | 14 | ✅ port logic with IO injected at the edge |
| `type-only` | 45 | ❌ out of scope (data co-ported only when a consumer needs it) |
| `ui-glue` | 0 | — |

**In-scope total: 74 modules**, of which **30 carry tests (~264 verbatim
cases)**. When all 74 are ported with their tests green offline, this milestone
is complete and the `src/shared` pure-logic surface is exhausted.

The backlog below is generated, not hand-curated; regenerate with the
`classify-shared-port-backlog` workflow if `src/shared` changes materially.

---

<!-- BEGIN generated backlog -->
## Milestone scope & definition of done

This milestone is **done** when every module classified `pure-portable` or `io-edge` has been ported to its target Rust crate **and** every verbatim test case that accompanies it has been translated and runs green **offline** (no network, no real clock, no ambient filesystem — IO is injected at the edge per the hazards). For `io-edge` modules, "ported" specifically means the pure decision logic is lifted into testable Rust with the Node/`fs`/`ws`/`crypto`/`child_process`/timer/clock/uuid surfaces replaced by injected closures or trait objects, mirroring how the TypeScript already (or should) parameterize them.

**In scope: 74 modules** — 60 `pure-portable` + 14 `io-edge`. Of these, **30 carry tests (~264 verbatim cases)** that must be translated and pass; the remaining 44 are ported without a pre-existing test file (parity for several is gated indirectly by a sibling test, e.g. `git-history.test.ts` covers the parser, `setup-script-imports.test.ts` covers the Codex-environment case).

**Out of scope:** **45 `type-only` modules** and **0 `ui-glue` modules** (nothing in this backlog was classified `ui-glue`; the renderer DOM-event-name and UI-copy declarations all fell under `type-only`). Type-only files carry no behavior to port. The caveat: a type-only file may still receive a **tiny const port** when a Rust consumer in this backlog needs its constants — e.g. `git-history-types` color/limit tables co-port with the graph/parser modules, `agent-hook-types`'s `ORCA_HOOK_PROTOCOL_VERSION`, `setup-script-import-providers`, `workspace-status-defaults`, `status-bar-defaults`. Those are dragged in as data, not ported as standalone deliverables, and do not count toward the 74.

## Ordered backlog

Ordering: priority high→low, then target crate grouped (alphabetical; `new:orca-telemetry` sorts first by literal name), then descending test count (ties broken alphabetically by file). All files live under `src/shared/`. Cat = pure (pure-portable) / io (io-edge). LOC `n/a` = not provided in the classification.

| Order | File | Crate | Cat | Tests | LOC | Deps | What | Hazards |
|---|---|---|---|---|---|---|---|---|
| 1 | telemetry-events.ts | new:orca-telemetry | pure | 32 | 1458 | serde_json | Zod-first telemetry event registry: per-event schemas, shared enums, refinements, runtime predicates | ~50 schemas; `.strict()`→`deny_unknown_fields`; superRefine cross-field rules; `.max`/uuid caps; compile-time roster checks non-portable; validates `serde_json::Value` |
| 2 | terminal-quick-commands.ts | orca-agents | pure | 15 | 179 | — | Validate/normalize quick commands (scope, dedup, length caps); format input; flatten multi-line | UTF-16 `.slice` caps diverge from Rust char/byte; line-break split trivial; keep `TUI_AGENT_CONFIG`/`isTuiAgent` in sync |
| 3 | tui-agent-startup.ts | orca-agents | pure | 11 | 184 | — | Build launch-command plans w/ posix/powershell/cmd quoting + draft-launch (prefill flag/env) | Shell-quoting parity (char ops, no regex); re-exports `isShellProcess`; needs `TUI_AGENT_CONFIG` |
| 4 | workspace-session-schema.ts | orca-config | pure | 12 | 278 | serde_json | Zod schema+parser validating persisted workspace-session JSON; ok/error result | Recursive `z.lazy` union; zod→serde parity; tolerant `.catch(undefined)`; NaN/Inf/neg preprocessing; stay tolerant of extra fields |
| 5 | setup-script-imports.ts | orca-config | pure | 8 | 292 | regex, serde_json | Detect/normalize setup/teardown scripts from Superset/Conductor/cmux JSON (+delegate Codex/pkg-mgr) | JSON.parse + shape coercion; `\bsetup\b` boundary regex; depends on codex + package-manager; exact unsupported-field paths |
| 6 | setup-script-import-codex-environment.ts | orca-config | pure | 0 | 153 | regex, serde_json | Hand-rolled minimal TOML parser extracting Codex env setup/cleanup scripts | Section/assignment regex + JSON.parse unescape; multiline triple-quote / escape-count parity; readFile injected |
| 7 | automation-schedules.ts | orca-core | pure | 19 | 593 | — | RRULE + 5-field cron parse/validate/classify w/ next/prev occurrence + friendly labels | Local-TZ Date + Intl labels need chrono/time + locale; ~4.7M occurrence scans; leap-day edges; cron split trivial |
| 8 | browser-grab-types.ts | orca-core | pure | 9 | 259 | — | Browser-grab types + payload budgets, safe-attr allowlist, redaction patterns, style-key list, aria predicate | Small const tables (Set/array/16-key style list); parity is the value; one `startsWith`; no regex/IO |
| 9 | feature-tips.ts | orca-core | pure | 8 | 107 | — | Feature-tip catalog + completion/ordering; unseen tips, 'new' first | Depends `feature-interactions.hasFeatureInteraction`; insertion-order Set dedup; 'new' priority first |
| 10 | contextual-tours.ts | orca-core | pure | 7 | n/a | — | Tour definitions + id validation/dedup helpers | Large UI-coupled const table (DOM selectors/copy); core logic is set-dedup + type-guard, pure |
| 11 | workspace-session-terminal-buffers.ts | orca-core | pure | 7 | 108 | — | Decide which worktrees keep scrollback (SSH/unhydrated vs local); prune/cap buffers | UTF-16 `slice(-LIMIT)` cap; missing connectionId ⇒ treat as SSH; copy-on-write session sharing |
| 12 | feature-interactions.ts | orca-core | pure | 4 | 262 | — | Interaction id catalog + validate/normalize persisted records (drop unknown, coerce counts) | 37-member id union; `isFinite`/`isInteger`/`>0` NaN/Inf rejection; fs+regex meta-test non-portable |
| 13 | feature-education-telemetry.ts | orca-core | pure | 3 | 60 | — | Telemetry source/outcome tables + membership-fallback normalizers | Clamp unknown to 'unknown'; test asserts parity with `CONTEXTUAL_TOUR_IDS` |
| 14 | remote-workspace-session-projection.ts | orca-core | pure | 3 | 205 | — | Project WorkspaceSessionState to/from remote, remap worktreeId<->path | Heavy Record transforms; injected predicates are pure; needs `splitWorktreeId` + session/tab types first |
| 15 | source-control-ai.ts | orca-git | pure | 21 | 863 | serde_json | SC-AI settings: defaults, legacy migration/merge, normalization, host-scoped model, per-op precedence | Normalize over `serde_json::Value`; JS proto-pollution guards are no-ops in Rust; stringify-equality→`PartialEq`; deep optional-record merges |
| 16 | git-history-graph.ts | orca-git | pure | 6 | 216 | — | Build swimlane graph view models (lanes, color rotation, ref sort/color) | Imports `addIncomingOutgoingChangesHistoryItems` + boundary ids (port together); pure `Map<string,colorId>`; intricate lane alloc |
| 17 | git-history-log-parser.ts | orca-git | pure | 0 | 151 | regex | Parse git-log records + ref decorations into GitHistoryItem/Ref | Anchored hash char-class; refs/remotes HEAD non-capturing group; `\x1f` decoration + `\0` record split; parity via `git-history.test.ts` |
| 18 | browser-screencast-protocol.ts | orca-relay | pure | 7 | 143 | serde_json | Binary wire protocol for screencast frames (16-byte header + JSON meta + image bytes) | LE header + reserved-byte check byte-exact; serde_json metadata round-trip w/ finite filter; zero-copy decode→borrowed `&[u8]` |
| 19 | agent-hook-relay.ts | orca-relay | pure | 5 | 115 | — | Relay->Orca envelope type, source union, JSON-RPC/method constants, `isRemoteAgentHooksEnabled` flag | Mostly types/constants; flag reads env—inject map; 14-variant source union; envelope wants serde derive |
| 20 | runtime-rpc-call-queue.ts | orca-relay | pure | 4 | 156 | — | Per-selector concurrency-limited RPC queue (fg/bg priority, compaction) + bg-method classifier | Promise concurrency→Rust futures/semaphore; `run()` closure injected; ordering/compaction is parity-critical |
| 21 | crash-reporting.ts | orca-text | pure | 6 | n/a | regex | Crash-report sanitization (secret/path redaction), allowlist, breadcrumb cap, text format/truncate | PATH_PATTERNS negative lookahead—needs fancy-regex/rewrite; replace-with-callback; UTF-16 caps (240/4000/64000) vs Rust bytes |
| 22 | workspace-session-browser-history.ts | orca-text | pure | 1 | 66 | — | Normalize/dedupe/recency-sort/cap browser URL history; prune from session | WHATWG URL normalize (needs url crate); stable recency sort; dedup by normalized URL; Kagi-token redaction |
| 23 | agent-hook-listener.ts | orca-agents | io | 40 | 3043 | serde_json, regex | Transport-agnostic hook listener: body parse, 14-source event normalization, prompt/tool caches, transcript scan, endpoint-file write | Inject byte-stream reader, seek/read closure (UTF-8 boundary carry), fs+platform+uuid for endpoint write, sha256, homedir; 14 sources × dozens of events; many regex |
| 24 | agent-detection.ts | orca-agents | pure | 0 | 508 | regex | Detect agent identity/status from OSC titles; normalize churn, clear stale, track transitions, id shells | regex LOOKBEHIND `(?<![\w./\\-])`—no Rust support, needs fancy-regex/hand boundary; OSC ctrl-char matchAll; braille spinner per-code-point; `i` flag |
| 25 | agent-feature-install-commands.ts | orca-agents | pure | 0 | 29 | — | Skill name/repo constants + build `npx skills add --global` command | none |
| 26 | agent-interrupt-intent.ts | orca-agents | pure | 0 | 19 | — | Interrupt-intent union/types + settle-delay const + type-guard | none (one equality-check guard) |
| 27 | agent-status-identity.ts | orca-agents | pure | 0 | 78 | — | Resolve which identity a pane keeps on inherited child hook (staleness windows); suppress inherited 'done' | none — staleness math takes `now` as param, pure |
| 28 | codex-auth-errors.ts | orca-agents | pure | 0 | n/a | regex | Detect Codex auth-failure msgs via regex; extract offending line from ANSI-stripped output | `i` flag + ANSI char-27 (all Rust-regex ok); `slice(0,4000)`/`trim` UTF-16 vs Rust byte semantics |
| 29 | tui-agent-config.ts | orca-agents | pure | 0 | 318 | — | Per-agent TUI config table + `isTuiAgent` guard + detect-command list | `Record<TuiAgent,Config>` must track ~30-member `TuiAgent` union; optional discriminant fields |
| 30 | secure-file.ts | orca-config | io | 3 | 175 | serde_json | Atomic secure file/JSON writer hardening perms (chmod 0600/0700 POSIX, Windows ACL via PowerShell) | Inject fs write/mkdir/rename/rm/chmod, execFileSync (whoami+powershell), randomBytes; win32 ACL branch + embedded PS script; SID cache; trivial CSV regex |
| 31 | runtime-environment-store.ts | orca-config | io | 1 | 166 | serde_json | CRUD store for runtime envs in orca-environments.json (add/remove/resolve/mark-used, dedup, sort) | Inject existsSync/readFileSync/writeSecureJsonFile/chmod, randomUUID, Date.now; zod parse parity; secure-file hardening |
| 32 | app-icon.ts | orca-config | pure | 0 | 15 | — | App-icon option table + default + `normalizeAppIconId` | none |
| 33 | auto-rename-branch-from-work-settings.ts | orca-config | pure | 0 | 21 | — | Normalize auto-rename-branch-from-work setting (default on unless guarded opt-out) | Imports `GlobalSettings` type; pure nullish/boolean derivation |
| 34 | e2e-config.ts | orca-config | pure | 0 | n/a | — | Derive E2E config (enabled/headless/exposeStore/userDataDir) via trim/bool coercion | none |
| 35 | runtime-environments.ts | orca-config | pure | 0 | 99 | serde_json | Zod schemas + redact / createFromPairingOffer / getPreferredPairingOffer | Zod parity (min(1), finite, literal kinds)→serde+validation; redact deviceToken/publicKeyB64; no IO |
| 36 | workspace-status-default-migration.ts | orca-config | pure | 0 | 82 | serde_json | Detect whether persisted status matches known legacy/default sets (migration/repair) | Untyped `serde_json::Value`; exact key-count (===4) + field-equality vs const tables; multiple ordering variants |
| 37 | automation-precheck.ts | orca-core | pure | 0 | 47 | — | Validate/clamp/format automation precheck command config + pass/fail result | none (numeric clamp, trim, label format; types only) |
| 38 | browser-annotation-viewport-bridge.ts | orca-core | pure | 0 | 257 | — | Types/constants + IPC payload validators (token + marker geometry) + injected overlay-script generator | Token regex→ASCII+length check; port validators only, keep ~170-line injection script as literal template |
| 39 | browser-viewport-presets.ts | orca-core | pure | 0 | 92 | — | Chrome-DevTools viewport preset table + id lookup + preset->override mapping | none (const table + find-by-id + projection) |
| 40 | diff-comments-format.ts | orca-core | pure | 0 | n/a | — | Format diff/review comments into quote-safe text block w/ escaping | Ordered escaping (backslash first); single-char replaces→`str::replace`; `DiffComment` type only |
| 41 | external-worktree-visibility.ts | orca-core | pure | 0 | 44 | — | Derive parent path of external worktree (trailing slash, UNC, Windows drive roots) | Trivial Windows drive/UNC char checks; depends `normalizeRuntimePathSeparators` (already in orca-core) |
| 42 | feature-wall-setup-steps.ts | orca-core | pure | 0 | 123 | — | Setup-step catalog + section partition + first-incomplete derivation | Section partition (parallel-work vs setup); first-incomplete depends on step order + section precedence |
| 43 | feature-wall-tiles.ts | orca-core | pure | 0 | 200 | — | Feature-wall tile catalog (media vs agent-status-mockup) + media-tile guard | Discriminated union on `kind` (media / agent-status-mockup); mostly static content + one guard |
| 44 | feature-wall-workflows.ts | orca-core | pure | 0 | 85 | — | Feature-wall workflow catalog + media-tile-by-id lookup | Depends `feature-wall-tiles`; Map lookup by media tile id, null on miss |
| 45 | filesystem-rename-collision.ts | orca-core | io | 0 | 51 | — | Validate no-clobber rename dest, allow case-only same-parent on case-insensitive FS via dev/ino | Inject lstat; caseFold NFC.normalize+toLowerCase needs unicode-normalization + Unicode lowercase (outside allowed deps); dev/ino identity |
| 46 | github-project-group-sort.ts | orca-core | pure | 0 | 235 | — | Deterministic grouping + multi-key sort of ProjectV2 rows + `isIterationCurrent` | localeCompare ordering ≠ Rust default; 7-kind union; Date parse + Date.now time-dependent; MAX_SAFE_INTEGER sentinel |
| 47 | linear-issue-read-limits.ts | orca-core | pure | 0 | 6 | — | Linear read-limit constants + `clampLinearIssueListLimit` | none |
| 48 | mobile-markdown-document.ts | orca-core | pure | 0 | 67 | — | Mobile markdown read/save types + FNV-1a-64 content hash + UTF-8 byte-length | FNV must iterate UTF-16 code units (`encode_utf16`), NOT chars/bytes; utf8ByteLength matches TextEncoder incl. lone-surrogate replacement |
| 49 | pty-session-id-format.ts | orca-core | pure | 0 | 41 | — | Separators + `parsePtySessionId` recovering worktreeId from minted PTY session id | ASCII separators ⇒ Rust rfind/find/slicing matches; keep strict non-empty-halves checks |
| 50 | repo-kind.ts | orca-core | pure | 0 | 17 | — | Derive repo kind ('git'/'folder') + display label | none |
| 51 | review-steps.ts | orca-core | pure | 0 | 37 | — | Review-tile copy table (notes/pr-view/ship) + accessor | none — static UI copy |
| 52 | runtime-bootstrap.ts | orca-core | pure | 0 | 49 | — | Runtime metadata types + `findTransport` (legacy fallback) + `getRuntimeMetadataPath` | `path.join` separators; legacy singular-field fallback cast; transport-kind union; no IO |
| 53 | runtime-client-events.ts | orca-core | pure | 0 | 48 | — | Runtime client event-stream unions + `toRuntimeActivateWorktreeEvent` builder | Event-type union; conditional optional fields→`Option` (trivial) |
| 54 | terminal-session-state-save-failure.ts | orca-core | pure | 0 | 15 | — | Error code/message constants + message builder + substring matcher | none |
| 55 | work-items.ts | orca-core | pure | 0 | 37 | — | Fetch-limit constants + SSH-remote-required error matcher + sort-by-updatedAt | `new Date(x).getTime()` ISO parse parity; narrows unknown error shape |
| 56 | worktree-card-properties.ts | orca-core | pure | 0 | 41 | — | Normalize worktree-card property list (force fixed, dedupe, canonical order) | none |
| 57 | git-uncommitted-line-stats.ts | orca-git | io | 15 | 193 | regex | Parse numstat (tab/NUL, rename norm) + count untracked additions w/ stat cache | Inject fs lstat/readFile + bounded concurrency; module LRU keyed size/mtime/ctime; brace-rename regex; byte newline count + `isBinaryBuffer`; `decodeGitCQuotedPath` dep |
| 58 | git-history.ts | orca-git | io | 6 | 231 | — | Orchestrate git history load via injected executor (limit clamp, ref resolve, merge-base, topo log, parse) | Inject `GitHistoryExecutor` (rev-parse/symbolic-ref/for-each-ref/merge-base/log); `\0` split; `clampHistoryLimit` pure; Promise.all ordering; re-exports parser/types |
| 59 | git-branch-cleanup.ts | orca-git | io | 2 | 144 | — | Derive cleanup target refs + decide branch fully-merged (merge-tree/rev-list/cherry) via injected exec | Inject `GitBranchCleanupExec`; `\r?\n` split trivial; rev-list count parse |
| 60 | git-discard-path-safety.ts | orca-git | io | 2 | 126 | — | Validate untracked-discard targets stay inside worktree (realpath, symlink-aware) before injected remove | Inject lstat+realpath (not yet injected); symlink-leaf vs symlinked-parent; cross-platform relative/resolve/sep; `removePath` already injected |
| 61 | binary-buffer.ts | orca-git | pure | 0 | 12 | — | Detect binary content via NUL byte in first 8KiB | Node Buffer→`&[u8]`; trivial NUL scan |
| 62 | git-effective-upstream.ts | orca-git | io | 0 | 212 | — | Resolve effective upstream (configured vs same-name origin) + ahead/behind via injected runner | Inject `GitCommandRunner` + `isNoUpstreamError`; `EffectiveGitUpstream` union; `--left-right` count parse; `splitRemoteBranchName` pure |
| 63 | git-publish-target-status.ts | orca-git | io | 0 | 72 | regex | Format publish-target display/ref names + ahead/behind vs remote-tracking ref via injected runner | `i` word-boundary regex `(?:exited with|exit code) 1\b`; inject `GitCommandRunner`; inspect code/stderr; rev-list count parse |
| 64 | git-rebase-source.ts | orca-git | io | 0 | 50 | — | Resolve remote rebase source (normalize base, list remotes, longest-prefix split, check-ref-format) | Inject runGit closure (remote/check-ref-format); `\r?\n` split trivial; throws on invalid/leading-dash refs |
| 65 | remote-runtime-client.ts | orca-relay | io | 5 | 623 | serde_json | One-shot + streaming E2EE WebSocket clients: handshake SM, encrypt/decrypt framing, keepalive, RPC validation | Inject ws socket, randomUUID, e2ee encrypt/decrypt, timers, Buffer; `RuntimeRpcEnvelope` (zod)→serde_json; port handshake/validation pure |
| 66 | remote-runtime-request-connection.ts | orca-relay | io | 1 | 297 | serde_json | Reusable cached E2EE WS multiplexing one-shot RPCs (ready-waiter queue, pending map, idle timer, handshake SM) | Inject ws + crypto + idle/request timers + id-gen; stateful lifecycle (closed/awaiting/ready) + pending-map bookkeeping; delegates framing |
| 67 | remote-runtime-request-websocket.ts | orca-relay | io | 1 | 120 | serde_json | `openRemoteRuntimeWebSocket`: build ws from pairing offer, derive shared key, wire listeners + cleanup | Inject ws ctor + e2ee key derive + hello JSON; attach/detach listener lifecycle + late-error swallow is the tested behavior |
| 68 | remote-runtime-request-frames.ts | orca-relay | pure | 0 | 98 | serde_json | Pure framing helpers: error ctors + parse ready/authenticated/RPC-response frames into tagged union | `ParsedRemoteRuntimeFrame` union + `RuntimeRpcEnvelope` zod→serde; exact error codes/msgs; keepalive-vs-envelope precedence; no IO |
| 69 | runtime-rpc-envelope.ts | orca-relay | pure | 0 | 73 | serde_json | Zod schema validating RPC frame envelope (Success/Failure/Keepalive) + `isKeepaliveFrame` guard | zod→`serde_json::Value`; discrimination (ok:true/false + keepalive); tolerant null/optional `_meta` |
| 70 | runtime-rpc-feature-interaction-source.ts | orca-relay | pure | 0 | 25 | serde_json | Tag/inspect RPC params with browser-pane interaction-source marker key | Arbitrary `serde_json::Value`; object-vs-array-vs-null guard + object-spread merge |
| 71 | ssh-pty-id.ts | orca-ssh | pure | 0 | 54 | — | Encode/parse/decode SSH PTY ids embedding connection id (ssh:<enc>@@<relayPtyId>) | encodeURIComponent parity (percent-encoding crate + custom AsciiSet); decode throw-on-malformed→`None` |
| 72 | terminal-ligatures.ts | orca-terminal | pure | 0 | 61 | — | Detect ligature-capable fonts (substring match) + resolve auto/on/off mode | `toLowerCase` Unicode parity; comma-split font-stack + quote strip (trivial) |
| 73 | file-uri-path.ts | orca-text | pure | 0 | 76 | regex | Bidirectional path<->file:// URI conversion (UNC, Windows drive, query/fragment) | WHATWG URL (no url crate—custom parse); encode/decodeURIComponent UTF-8 percent + lone-surrogate; UNC optional capture groups |
| 74 | string-utils.ts | orca-text | pure | 0 | 12 | — | `escapeRegex`: escape regex metacharacters for RegExp constructor | Replicate exact escaped set `.*+?^${}()|[]\`; `regex::escape` differs—needs manual char replace |

## Out of scope

45 `type-only` modules (and 0 `ui-glue`). All are declaration/constant files with no portable behavior; ported only as data when a Rust consumer above needs the constants.

- **Provider/domain type declarations** — `automations-types`, `git-status-types`, `git-history-types`, `github-project-types`, `gitlab-types`, `hosted-review`, `jira-types`, `github-auth-types`, `skills`, `speech-types`, `rate-limit-types`, `workspace-ports`, `workspace-space-types`, `remote-workspace-types`, `source-control-ai-types`, `runtime-types`, `runtime-access-grants`: big interfaces / discriminated + string-literal unions, no functions. (`git-history-types` color/limit constants will co-port with the graph/parser deliverables.)
- **Usage-analytics shapes** — `claude-usage-types`, `codex-usage-types`, `opencode-usage-types`: scan-state/summary/daily/breakdown row types only.
- **Permission / status / install state types** — `cli-install-types`, `computer-use-permissions-types`, `developer-permissions-types`, `telemetry-consent-types`, `shell-open-types`, `gh-star-source`: id/status string unions and result records, no logic.
- **Constants-only modules** — `agent-hook-types` (`ORCA_HOOK_PROTOCOL_VERSION`), `feature-wall-telemetry`, `terminal-scrollback-limits`, `orca-attribution`, `pane-key`, `status-bar-defaults`, `windows-terminal-shell`, `workspace-source`, `workspace-status-defaults`, `setup-script-import-providers`: single consts/arrays; tiny const ports only if a consumer needs them.
- **Renderer UI-copy tables and DOM/event-name declarations** — `agents-orchestration-steps`, `workbench-steps`, `editor-save-events`, `updater-renderer-events`, `rich-markdown-context-menu`, `browser-guest-events`, `app-identity`: static display copy, custom-event names, and callback-bearing detail types (the closest thing to "ui-glue" here); no behavior to port.
- **Central type hub** — `types.ts` (~240 interfaces incl. the `TuiAgent` union, 3 `PET_SIZE` consts): huge fan-in surface; port fragments incrementally as dependent modules demand, never wholesale.
- **SSH type shapes** — `ssh-types`: connection/target/lease/port-forward aliases + grace-period consts; its 4 tests are compile-time type-shape assertions with no runtime behavior, so it stays out of scope.

## Suggested execution order

Eight cohesive waves over the 22 high-priority modules. Each wave names its co-ported medium / type-only dependencies so a subsystem lands intact rather than in alphabetical fragments.

1. **Git-history subsystem (orca-git).** `git-history-log-parser` → `git-history-graph` → (io-edge) `git-history`. Co-port `git-history-types` (lane colors, default/max limits, color ids) and the out-of-batch `git-history-boundary-rows` (`addIncomingOutgoingChangesHistoryItems` + boundary IDs) that the graph imports. Parser parity is gated by `git-history.test.ts`, so do all three together.
2. **Source-control AI (orca-git).** `source-control-ai` plus its `source-control-ai-types` structs. Self-contained 21-test, 863-LOC normalize/merge/precedence engine; nail the `serde_json::Value` normalization and deep optional-record merge parity.
3. **Feature-education & onboarding (orca-core).** Strict dependency chain: `feature-interactions` → `feature-tips` (uses `hasFeatureInteraction`) → `contextual-tours` → `feature-education-telemetry` (test asserts its id table stays aligned with `CONTEXTUAL_TOUR_IDS`). Port in this order so each consumer's dependency already exists.
4. **Workspace-session core (orca-config + orca-core + orca-text).** `workspace-session-schema` (read-boundary validator) first, then `workspace-session-terminal-buffers`, `remote-workspace-session-projection`, and `workspace-session-browser-history`. Needs the `WorkspaceSessionState`/`TerminalTab` type fragments and `splitWorktreeId` ported up front; browser-history pulls in a URL-normalization helper (no `url` crate in the allowed set).
5. **Setup-script imports (orca-config).** `setup-script-import-codex-environment` (hand-rolled TOML) first, then the `setup-script-imports` aggregator that delegates to it plus the package-manager module. Co-port the `setup-script-import-providers` const. Parity for the Codex path is gated by `setup-script-imports.test.ts`.
6. **Runtime-RPC & relay transport (orca-relay).** `runtime-rpc-call-queue` (scheduling/compaction, Promise→futures/semaphore), `browser-screencast-protocol` (byte-exact binary framing), and `agent-hook-relay` (envelope + env-flag parser). These seed the relay crate's wire-protocol layer that the medium io-edge `remote-runtime-*` clients (orders 65–70) build on next.
7. **Agents / TUI quick-commands (orca-agents).** `terminal-quick-commands` and `tui-agent-startup`. Both depend on `tui-agent-config` (`TUI_AGENT_CONFIG`/`isTuiAgent`) and `tui-agent-startup` re-exports `isShellProcess` from `agent-detection`, so pull those two medium modules into the same wave.
8. **Standalone heavy hitters.** Independent large modules with no shared subsystem: `telemetry-events` (new `orca-telemetry` crate, ~50 Zod schemas + superRefine), `automation-schedules` (cron/RRULE, needs a date-time lib), and `browser-grab-types` (const-table parity). Schedule these in parallel with the others since they share no dependencies.
<!-- END generated backlog -->

---

## Progress

Tick modules off here as they land (keep in sync with `ported-modules.md`).

- **In scope:** 74 modules (30 tested, ~264 cases).
- **Ported so far this milestone:** 21 / 74 (workspace **738 tests**; 81 modules parity-verified).
  - ✅ `feature-interactions` → `orca-config::feature_interactions`.
  - ✅ **Review & ship (git-history):** `git-history-types`, `git-history-log-parser`, `git-history-graph`, `git-history-boundary-rows`, `git-history` (io-edge, injected executor) → `orca-git`.
  - ✅ **Review & ship (SC-AI):** `source-control-ai` (+types) → `orca-git` (21 tests).
  - ✅ **Run any agent:** `tui-agent-config`, `tui-agent-startup`, `terminal-quick-commands` → `orca-agents`.
  - ✅ **Session:** `workspace-session-schema`, `workspace-session-terminal-buffers` → `orca-config`.
  - ✅ **Browser:** `browser-grab-types`, `browser-viewport-presets` → `orca-core`; `browser-screencast-protocol` → `orca-relay`.
  - ✅ **Onboarding:** `feature-tips`, `contextual-tours`, `feature-education-telemetry` → `orca-config`.
  - ✅ **Setup:** `setup-script-import-codex-environment`, `setup-script-imports` → `orca-config`.
  - Each carries verbatim tests + (where a clean postcondition exists) a Trust contract + a parity adapter.
- **Next up:** fleet-visibility remainder (`agent-status-identity`, `agent-detection`, `agent-hook-listener`), `automation-schedules` (needs a vendored date-time crate), and the remaining browser/runtime-rpc slices.
