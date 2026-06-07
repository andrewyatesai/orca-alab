# Orca → Rust Migration Plan
> Generated from the `orca-functional-map` workflow. Domain groupings (proposed crates), ordered phases, critical path, and risks. Leaf pure-logic first; UI shells last.

## Domains → proposed crates

### Pure shared core  →  `orca-core`

Leaf zero-dependency crate already bootstrapped at rust/crates/orca-core (cross_platform_path, git_cquoted_path, worktree_base_ref, worktree_id). Absorbs the transport-agnostic pure logic in src/shared (path/git-state/agent-protocol normalization, validation, scheduling math) plus the pure renderer helpers in src/renderer/src/lib. TypeScript .test.ts files (106 in src/shared) translate verbatim to cargo tests for behavioral fidelity. Everything depends on this; it must stay IO-free and build offline.

Subsystems: `Orca Shared Subsystem`, `ui-lib-hooks (pure portions: agent-detection, path-utils, agent-status-types, metadata-caching)`

### Wire protocol & E2EE  →  `orca-proto`

The wire contract shared by runtime, relay, CLI, web, and mobile. Splits out of shared because it needs serde + a crypto_box/libsodium port of tweetnacl. Must remain byte-compatible with the existing TS/web/mobile clients so old peers keep working during dual-run. Houses RPC envelopes, the min-compatible-version handshake (MIN_COMPATIBLE_RUNTIME_SERVER_VERSION), pairing offers, and terminal/screencast frame formats.

Subsystems: `Orca Shared Subsystem (runtime-rpc-envelope, protocol-compat, protocol-version, pairing, e2ee-crypto, terminal/browser/screencast stream protocols)`, `ui-runtime-web (E2EE client + RPC envelope portions)`

### Renderer state model  →  `orca-state`

Zustand store (src/renderer/src/store) is the single source of truth for repos, worktrees, tabs, terminals, editor state, and integrations. Porting its state machine to a pure Rust crate (serde-persisted) is high-leverage: the Swift UI binds to it over UniFFI, and the CLI/headless consumers reuse it. Pure-tier reducer logic, no rendering.

Subsystems: `ui-store`, `ui-lib-hooks (state/caching/scroll-anchor hooks)`

### Terminal emulation  →  `orca-terminal`

Replaces @xterm/* and @xterm/headless with vendored alacritty_terminal for VT/ANSI parsing, scrollback, and snapshot serialization. Self-contained and high-leverage (core UX surface). Validated by replaying captured PTY streams against xterm-serialize golden snapshots. Note: alacritty has no addon ecosystem, so search/ligatures/web-links/unicode11/serialize behavior must be reimplemented.

Subsystems: `main-daemon (xterm-headless emulation/serialize)`, `ui-terminal (xterm.js core + addons)`

### Git execution  →  `orca-git`

Low-level git/gh/glab CLI wrappers with WSL-aware spawning, status polling, diff generation, worktree management, and upstream/push-target resolution (src/main/git). IO leaf: keeps shelling out to git/gh/glab binaries initially (output-parsing parity testable against fixtures), with optional gitoxide/git2-rs for hot paths. Foundation for forge providers, providers dispatch, and runtime.

Subsystems: `main-git`

### SCM & work-item providers  →  `orca-forge`

Provider-agnostic review/issue layer over GitHub, GitLab, Bitbucket, Azure DevOps, Gitea, Jira, and Linear (src/main/{github,gitlab,bitbucket,azure-devops,gitea,jira,linear,source-control}). Keep GitHub/GitLab behind gh/glab CLIs; others via reqwest + serde_json. Honors the AGENTS.md rule to avoid GitHub-only naming for generic review concepts. Depends on orca-git; consumed by runtime + UI.

Subsystems: `main-source-control`, `main-github subsystem`, `GitLab Client Subsystem (main-gitlab)`, `main-git-providers-small`, `Linear SDK Integration (main-linear)`

### Remote transport & relay  →  `orca-ssh-transport, orca-relay`

SSH connection lifecycle, relay deployment, and JSON-RPC multiplexing (src/main/ssh, src/relay). orca-relay is a standalone Rust binary that is an excellent isolated proving ground: it can be built and dropped onto remote hosts before any UI work, exercising orca-proto, orca-git, and the PTY/fs providers over a single channel. Transport uses ssh2-rs/libssh2 plus a system-OpenSSH ProxyCommand/ProxyJump fallback.

Subsystems: `SSH (main-ssh) Remote Transport & Relay Multiplexer`, `Relay subsystem`

### PTY daemon, provider dispatch & port discovery  →  `orca-daemon, orca-pty, orca-providers, orca-ports`

Native PTY spawn/resize/kill (replacing node-pty, ConPTY on Windows), the local-vs-SSH provider dispatch for PTY/fs/git (src/main/providers), the headless session daemon binary (src/main/daemon), and dev-server URL/port discovery (src/main/ports). Depends on orca-terminal, orca-ssh-transport, and orca-git. The daemon can run headless and be driven by the CLI before the UI exists.

Subsystems: `main-daemon (PTY lifecycle)`, `main-providers: Provider Abstraction & Dispatch`, `main-ports (Advertised URL & Port Scanning)`

### Agent hooks, integrations & text generation  →  `orca-agent-hooks`

HTTP hook listener that tracks agent lifecycle (working/waiting/done) for Claude/Codex/Gemini/Cursor/Droid/etc., per-agent hook installers that POST to the loopback server (src/main/agent-hooks + per-agent dirs), and LLM-driven commit/PR/branch text generation plus the Hermes plugin. IO tier; depends on orca-core agent-protocol types and orca-ssh-transport for remote install.

Subsystems: `agent-hooks`, `main-agent-integrations`, `text-generation + hermes (agent hooks)`

### AI accounts & rate limits  →  `orca-accounts`

OAuth/account lifecycle and credential storage for Claude and Codex (Keychain on macOS via Security framework, filesystem elsewhere), host/WSL runtime switching, managed-home sync, and the centralized rate-limit poller across Claude/Codex/Gemini/OpenCode (src/main/{claude-accounts,codex-accounts,rate-limits}). Credential-format migration is sensitive: must not lock users out of stored tokens.

Subsystems: `Claude Accounts Management Subsystem`, `main-codex-accounts`, `main-rate-limits`

### Automations & usage tracking  →  `orca-automations, orca-usage`

Scheduled/manual automation execution with prechecks and process-tree management (src/main/automations), plus token/cost analytics parsing ~/.claude transcripts and OpenCode SQLite DBs (src/main/{claude-usage,codex-usage}). IO tier; SQLite reads move to rusqlite (readonly + query_only pragma). Depends on orca-accounts and orca-providers.

Subsystems: `main-automations`, `main-usage subsystem (Claude/OpenCode usage tracking + stats collection)`

### Observability & telemetry  →  `orca-observability`

Local-first NDJSON span tracer with secret redaction and optional OTLP export, consent-aware bundle upload, and the PostHog event pipeline (src/main/{observability,telemetry}). Cross-cutting: instrumentation (withGitSpan etc.) is referenced by git/runtime, so the tracing API surface should land early even if exporters follow. Uses tokio + reqwest; install_id via uuid.

Subsystems: `observability (error-tracking lane)`, `main-telemetry`

### Browser, computer-use & speech (native/ffi services)  →  `orca-browser, orca-computer-use, orca-speech`

The highest-risk native peripherals. main-browser drives embedded Chromium via CDP/screencast/grab-overlay/WebAuthn (src/main/browser) with the agent-browser binary staying native. computer-use wraps the macOS helper socket and Linux/Windows desktop scripts (src/main/computer). main-speech wraps sherpa-onnx STT (ffi via the ort/ONNX-Runtime route). Each is isolated and FFI-heavy; sequence after the core is stable.

Subsystems: `main-browser`, `ui-browser-pane (backend RPC + grab/annotation logic)`, `computer-use`, `main-speech subsystem`

### Runtime orchestration hub  →  `orca-runtime`

The central engine wiring every service together: session/workspace orchestration, RPC dispatch to 70+ methods, persistence (orchestration SQLite + Store), startup sequencing, single-instance locking, skills/keybindings/repo-discovery infra (src/main/{ipc,index.ts,daemon,memory,network,keybindings} and the misc/platform clusters). Highest integration risk; depends on essentially all lower crates. Build last among backend tiers.

Subsystems: `main-runtime`, `Orca main-root (Electron app lifecycle, service wiring)`, `main-startup`, `main-misc-infra`, `main-platform-misc`

### IPC & preload bridge  →  `orca-ipc, orca-preload-bridge`

The 500+ channel main<->renderer contract (src/main/ipc) and the typed preload surface (src/preload). In the native target this becomes the Tauri-style command surface plus the UniFFI/C-ABI boundary the Swift shell links against. Generate bindings from a single schema and golden-test them so the Rust core and any residual TS cannot drift during dual-run. Note AGENTS.md preload .ts-over-.d.ts typecheck-hole constraint when bridging.

Subsystems: `main-ipc`, `preload`

### CLI & installer  →  `orca-cli, orca-cli-installer`

Declarative command specs, arg validation, and local/remote runtime dispatch over Unix sockets / WebSocket tunnels (src/cli), plus cross-platform shell-command registration and launcher lifecycle (src/main/cli). clap for parsing, tokio for transport. Strategic: the first fully end-to-end Rust consumer of orca-runtime, shippable headless before any native UI exists.

Subsystems: `CLI Subsystem`, `main-cli-internal`

### Native macOS UI shell  →  `orca-shell (Swift/SwiftUI) + orca-renderer-bridge`

The entire React renderer (src/renderer) and Electron window/menu/dock layer (src/main/menu, window mgmt) rebuilt natively in SwiftUI on macOS, binding to orca-state and orca-runtime over UniFFI. Largest effort and last by necessity: it requires a frozen core+IPC contract. Hardest gaps are Monaco (ui-editor), ProseMirror/Tiptap rich markdown, dnd-kit tab DnD, and sidebar virtualization, none of which have native equivalents. Linux/Windows shells lag macOS.

Subsystems: `main-window`, `ui-sidebar`, `ui-terminal (view layer)`, `ui-editor subsystem`, `ui-settings`, `ui-right-sidebar`, `ui-feature-wall`, `ui-status-bar`, `ui-browser-pane (view layer)`, `ui-tabs (TabBar + TabGroup)`, `ui-automations`, `ui-onboarding`, `ui-scm-views`, `ui-misc`, `ui-runtime-web (web/mobile bridge)`

## Phases (ordered)

### Phase 1: Phase 0 — Foundation & vendoring

**Goal:** Make the rust/ workspace build offline and provable against the TS baseline.

**Rationale:** Everything downstream depends on offline reproducible builds and a way to prove behavioral fidelity. Zero product risk, pure enablement; the Cargo.toml already commits to this architecture.

**Includes:** `Expand rust/Cargo.toml workspace members`, `Vendor + strip deps under rust/vendor with .cargo/config.toml`, `Stand up the UniFFI/C-ABI scaffolding for the future Swift boundary`, `Establish the .test.ts -> cargo test golden-translation harness and CI parity gate`

### Phase 2: Phase 1 — Pure core (orca-core)

**Goal:** Port all IO-free src/shared logic with verbatim tests.

**Rationale:** Leaf crate, zero dependencies, already bootstrapped. Lowest risk and highest leverage: every other crate links it. Verbatim test translation makes regressions impossible to miss.

**Includes:** `Pure shared core`

### Phase 3: Phase 2 — Protocol, E2EE & state model

**Goal:** Freeze the wire contract and the renderer state machine in Rust.

**Rationale:** Near-pure, depends only on core + serde/crypto. Locking the protocol early lets existing TS/web/mobile clients keep talking to Rust services during dual-run, and a Rust state model decouples the eventual Swift UI from the backend.

**Includes:** `Wire protocol & E2EE`, `Renderer state model`

### Phase 4: Phase 3 — Terminal emulation

**Goal:** Swap xterm/xterm-headless for alacritty_terminal.

**Rationale:** Self-contained and central to UX. Can be validated in isolation by replaying captured PTY streams against xterm-serialize golden snapshots, with no dependency on the runtime hub.

**Includes:** `Terminal emulation`

### Phase 5: Phase 4 — Git & forge providers

**Goal:** Port git execution and the SCM/work-item provider layer.

**Rationale:** IO leaves that mostly shell out to existing git/gh/glab binaries, so output-parsing parity is fixture-testable with little blast radius. Unblocks runtime, providers, and the review UI.

**Includes:** `Git execution`, `SCM & work-item providers`

### Phase 6: Phase 5 — Transport, PTY daemon & provider dispatch

**Goal:** Stand up remote transport, the relay, native PTY, provider dispatch, and port discovery.

**Rationale:** The relay is a standalone Rust binary that proves orca-proto + git + PTY/fs providers end-to-end over SSH without touching the UI. Native PTY and local/remote dispatch are the backbone the runtime hub will consume.

**Includes:** `Remote transport & relay`, `PTY daemon, provider dispatch & port discovery`

### Phase 7: Phase 6 — Agent ecosystem

**Goal:** Port hooks, integrations, accounts, rate limits, automations, and usage.

**Rationale:** IO services that build on git/providers/transport. Mostly independent of each other, so they parallelize. Credential and SQLite migrations are contained here and testable against real on-disk fixtures.

**Includes:** `Agent hooks, integrations & text generation`, `AI accounts & rate limits`, `Automations & usage tracking`

### Phase 8: Phase 7 — Observability & native peripherals

**Goal:** Port tracing/telemetry and the FFI-heavy native services.

**Rationale:** Observability is cross-cutting but low-risk and best landed before the hub integration so spans exist when wiring. Browser/computer-use/speech are the riskiest FFI surfaces but are isolated, optional at runtime, and must not gate the core path.

**Includes:** `Observability & telemetry`, `Browser, computer-use & speech (native/ffi services)`

### Phase 9: Phase 8 — Runtime hub & IPC boundary

**Goal:** Wire every service into the orchestration engine behind the command/preload boundary.

**Rationale:** Highest integration risk; depends on all lower crates. Generated bindings + golden contract tests prevent drift between Rust and any residual TS while running hybrid.

**Includes:** `Runtime orchestration hub`, `IPC & preload bridge`

### Phase 10: Phase 9 — CLI (first end-to-end Rust consumer)

**Goal:** Ship a headless Rust CLI driving the Rust runtime.

**Rationale:** Validates the full backend stack in production before any native UI exists, and gives users a working artifact mid-migration. Flushes out runtime/IPC contract bugs cheaply.

**Includes:** `CLI & installer`

### Phase 11: Phase 10 — Native macOS UI shell (last)

**Goal:** Rebuild the React renderer in SwiftUI on the frozen core.

**Rationale:** Largest effort and highest reimplementation risk (Monaco, Tiptap, dnd-kit, virtualization have no native equivalents). Requires a stable orca-state + orca-runtime + IPC contract, so it can only begin once those are proven by the CLI and dual-run.

**Includes:** `Native macOS UI shell`

## Critical path

- Phase 0: offline-vendored workspace + UniFFI/C-ABI scaffold + TS->cargo golden-test harness (rust/Cargo.toml already commits to this)
- orca-core: pure src/shared logic ported with verbatim tests (rust/crates/orca-core)
- orca-proto: wire/RPC/E2EE contract frozen byte-compatible with existing TS/web/mobile peers
- orca-terminal: alacritty_terminal replacing xterm, validated against serialize snapshots
- orca-git + provider/transport backbone: orca-ssh-transport, orca-daemon/orca-pty native PTY, orca-providers dispatch (relay binary as isolated proving ground)
- orca-runtime hub wiring all services behind the orca-ipc / orca-preload-bridge command+UniFFI boundary
- orca-cli: first end-to-end headless consumer proving the runtime contract in production
- orca-shell (SwiftUI) binding orca-state + orca-runtime over UniFFI, replacing the React renderer last

## Risks

- alacritty_terminal has no xterm addon ecosystem: search, ligatures, web-links, unicode11, webgl, and especially addon-serialize snapshot/restore must be reimplemented and proven bit-identical to current scrollback serialization.
- node-pty -> native PTY parity is hardest on Windows (ConPTY) and WSL; resize/signal/exit-code semantics must match or terminal sessions regress.
- ssh2-rs/libssh2 feature gaps vs Node ssh2 (ProxyCommand/ProxyJump, agent forwarding, SFTP edge cases); the system-OpenSSH fallback path must cover what the library cannot.
- E2EE interop: porting tweetnacl to crypto_box/libsodium must be byte-compatible, or paired web/mobile clients break during dual-run.
- Protocol/version handshake (MIN_COMPATIBLE_RUNTIME_SERVER_VERSION) must stay wire-stable so older CLI/web/mobile peers keep working while half the stack is Rust.
- SQLite coupling: node:sqlite/better-sqlite3 -> rusqlite must replicate readonly + query_only pragmas and the exact cookie/orchestration/OpenCode schemas opened read-only.
- Credential migration: macOS Keychain (Security framework) and Electron safeStorage encryption formats must be read compatibly or users lose stored OAuth tokens.
- Embedded Chromium is the biggest UI-tier gap: CDP, screencast streaming, grab-mode overlay, downloads, and WebAuthn have no clean tauri-webview/Rust equivalent; agent-browser stays a native binary but the host integration is XL-risk.
- SwiftUI rebuild of the large React surface: Monaco (code/diff editor), ProseMirror/Tiptap (rich markdown), dnd-kit tab drag-drop, and virtualized sidebar lists have no native equivalents and represent the dominant effort and schedule risk.
- IPC surface is enormous (500+ channels, 70+ RPC methods); without generated bindings and golden contract tests, the Rust core and residual TS will drift during the long hybrid period.
- sherpa-onnx native addons -> ort/ONNX-Runtime bindings plus multi-model catalog and platform-native lib vendoring add size and FFI risk; STT is non-essential and must not gate the core path.
- Vendoring-and-stripping every dependency for offline builds while still tracking upstream security patches is an ongoing maintenance burden.
- gh/glab/git CLI version coupling: output-format changes break parsing; keep provider behavior behind explicit checks (GitLab and others, not GitHub-only) per AGENTS.md.
- Native shell is macOS-first; Linux/Windows/WSL shells will lag, risking long-lived platform divergence in a product that targets all three.
- Long-lived dual-run: running Rust services beside Electron behind feature flags risks an indefinite hybrid if the IPC/FFI boundary or migration sequencing slips.
