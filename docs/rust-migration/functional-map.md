# Orca Functional Map
> Generated from the `orca-functional-map` workflow (51 subsystems mapped). Source of truth for the TS→Rust port: each subsystem lists what it does, its public/IPC surface, real external dependencies, persistence, cross-platform concerns, and a Rust-portability assessment.
## Contents
- **backend** — main-runtime, main-ipc, main-browser, main-daemon, main-github subsystem, SSH (main-ssh) Remote Transport & Relay Multiplexer, agent-hooks, main-git, main-providers: Provider Abstraction & Dispatch, Claude Accounts Management Subsystem, main-codex-accounts, main-rate-limits, GitLab Client Subsystem (main-gitlab), main-window, computer-use, observability (error-tracking lane), Linear SDK Integration (main-linear), main-automations, main-startup, text-generation + hermes (agent hooks), main-cli-internal, main-telemetry, main-ports (Advertised URL & Port Scanning), main-usage subsystem (Claude/OpenCode usage tracking + stats collection), main-speech subsystem, main-source-control, main-git-providers-small, main-misc-infra, main-agent-integrations, main-platform-misc, Orca main-root (Electron app lifecycle, service wiring, IPC handlers)
- **renderer** — ui-sidebar, ui-terminal, ui-editor subsystem, ui-settings, ui-right-sidebar, ui-feature-wall, ui-status-bar, ui-browser-pane, ui-tabs (TabBar + TabGroup), ui-automations, ui-onboarding (Orca Electron IDE), ui-scm-views, ui-misc (assorted UI components), ui-store, ui-lib-hooks (Orca Renderer), ui-runtime-web
- **cli** — CLI Subsystem
- **relay** — Relay subsystem
- **preload** — preload
- **shared** — Orca Shared Subsystem


## backend

### `Claude Accounts Management Subsystem`

Manages OAuth account lifecycle for Claude Code including authentication (login/reauthentication), credential storage (Keychain on macOS, filesystem on other platforms), multi-account selection with runtime-specific switching (host/WSL), and token refresh tracking. Also manages Claude agent hooks installation for post-event callbacks in settings.json.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca_claude_accounts`  
_Core auth logic is IO-heavy (filesystem, keychain, subprocess) with some platform-specific binaries (macOS security, Windows wsl.exe, Claude CLI). The OAuth flow, credential storage, account selection, and token tracking are largely pure business logic. Hook installation is pure JSON manipulation. Main complexity: (1) macOS Keychain via security command needs FFI or subprocess wrapper; (2) WSL path handling and wsl.exe integration (Windows-only); (3) Claude CLI invocation (subprocess with env var patching); (4) Atomic file writes with ownership validation. Credential parsing/identity resolution are pure JSON. Suggested split: pure Rust core (account model, selection logic, OAuth parsing, token comparison) in a library crate, platform-specific bindings (keychain, wsl, fs ops) in a sister platform crate, with Swift/native bridge for Keychain on macOS if FFI proves unwieldy. Hook installation can be a standalone JSON utility. Token refresh readback and PTY gating are application-layer concerns (state machine) best kept in Rust alongside the core service._

**Capabilities**
- OAuth login flow via Claude CLI (claude auth login --claudeai)
- Credential capture and parsing from Claude config directory
- Keychain storage on macOS using security command
- Filesystem-based credential storage on non-macOS platforms
- Multi-account management with per-account OAuth metadata (organizationUuid, organizationName)
- Host runtime account selection
- WSL runtime account selection with per-distro account tracking
- Account reauthentication with token refresh readback
- Account removal with safe cleanup and rollback on failure
- Runtime auth materialization (sync selected account credentials to ~/.claude)
- Token refresh tracking and comparison
- System default snapshot management for account switching
- Live PTY gate to prevent account switching while Claude processes run
- Claude agent hook installation (install/remove/status) for local and remote SSH
- Hook payload delivery via HTTP POST with ORCA_* environment variables

**Public API / IPC / RPC**
- ipcMain.handle('claudeAccounts:list') -> ClaudeRateLimitAccountsState
- ipcMain.handle('claudeAccounts:add', target?: ClaudeAccountAddTarget) -> ClaudeRateLimitAccountsState
- ipcMain.handle('claudeAccounts:reauthenticate', { accountId }) -> ClaudeRateLimitAccountsState
- ipcMain.handle('claudeAccounts:remove', { accountId }) -> ClaudeRateLimitAccountsState
- ipcMain.handle('claudeAccounts:select', { accountId, runtime?, wslDistro? }) -> ClaudeRateLimitAccountsState
- ClaudeAccountService.listAccounts()
- ClaudeAccountService.addAccount(target?)
- ClaudeAccountService.reauthenticateAccount(accountId)
- ClaudeAccountService.removeAccount(accountId)
- ClaudeAccountService.selectAccount(accountId)
- ClaudeAccountService.selectAccountForTarget(accountId, target)
- ClaudeRuntimeAuthService.prepareForClaudeLaunch(target?)
- ClaudeRuntimeAuthService.prepareForRateLimitFetch(target?)
- ClaudeRuntimeAuthService.syncForCurrentSelection(target?)
- ClaudeRuntimeAuthService.getRuntimeConfigDir()
- ClaudeHookService.getStatus()
- ClaudeHookService.install()
- ClaudeHookService.installRemote(sftp, remoteHome)
- ClaudeHookService.remove()

**External dependencies**
- Electron app and ipcMain
- node:child_process (execFile, execFileSync, spawn)
- node:fs (readFileSync, writeFileSync, mkdirSync, rmSync, chmodSync, lstatSync, realpathSync)
- node:os (homedir, tmpdir)
- node:path (join, resolve, relative, dirname, sep)
- node:crypto (randomUUID, createHash)
- node:events (EventEmitter)
- macOS security command (via execFile) for Keychain access
- WSL wsl.exe binary (Windows only)
- Claude CLI (claude auth login, claude auth status)
- ssh2 (SFTPWrapper type for remote hook installation)

**Persistence**
- ~/.claude/ (default Claude config directory with .credentials.json and .claude.json)
- ~/.local/share/orca/claude-accounts/{accountId}/auth/ (WSL managed auth on Linux)
- \\\\?\\unc\\path\\to\\wsl\\distro\\root\\.local\\share\\orca\\claude-accounts\\{accountId}\\auth\\ (WSL managed auth viewed from Windows)
- macOS Keychain service 'Claude Code-credentials' (legacy active credentials)
- macOS Keychain service 'Claude Code-credentials-{sha256_suffix}' (scoped active credentials per config dir)
- macOS Keychain service 'Orca Claude Code Managed Credentials' with account=accountId (managed account credentials)
- Electron app userData/claude-accounts/{accountId}/auth/.orca-managed-claude-auth (account ownership marker)
- Electron app userData/claude-accounts/{accountId}/auth/oauth-account.json (stored OAuth metadata)
- Electron app userData/claude-accounts/{accountId}/auth/.credentials.json (stored credentials on non-macOS)
- GlobalSettings.claudeManagedAccounts[] (account list)
- GlobalSettings.activeClaudeManagedAccountId (host runtime active account)
- GlobalSettings.activeClaudeManagedAccountIdsByRuntime (per-runtime account selection)
- ~/.claude/settings.json (Claude agent hook configuration)
- ~/.openclaude/settings.json (OpenClaude hook configuration)
- Remote $HOME/.claude/settings.json (SSH remote hook config)

**Cross-platform concerns**
- macOS: Keychain for credential storage via security command, scoped by SHA256 hash of config dir
- Windows: WSL integration with wsl.exe binary for running distro commands, temporary config dirs in WSL, Windows-to-WSL path conversion (UNC paths)
- Linux/macOS: Filesystem-based credential storage in .local/share/orca/claude-accounts
- All platforms: Managed auth directory ownership validation with symlink checks and canonical path resolution
- All platforms: Claude CLI invocation with CLAUDE_CONFIG_DIR env var override
- SSH remote: POSIX shell hook scripts (.sh), curl for HTTP POST callbacks, SFTP for file delivery

### `GitLab Client Subsystem (main-gitlab)`

Provides GitLab API integration for merge request, issue, and work item management. Acts as a thin wrapper around the glab CLI, handling authentication, project resolution, rate limiting, error classification, and data transformation for both gitlab.com and self-hosted GitLab instances.

**Rust portability:** tier=`io` · effort=`M` · target=`gitlab-rs + http/reqwest for REST client, or thin wrapper over glab via std::process::Command; suggest building on top of gitoxide pattern used in git subsystem`  
_Moderate complexity: REST API interaction is straightforward but self-hosted instance support (dynamic host discovery via glab auth status, hostname targeting) adds routing complexity. Error classification mirrors GitHub side (permission, not_found, rate_limited, etc.). Rate limit caching with TTL can be implemented with Arc<Mutex<>>. Project ref cache and in-flight deduplication patterns are standard Rust concurrency. Main effort is mapping glab CLI subprocess invocations to native GitLab REST client (reqwest + serde) and handling auth (gitlab_token env var, or glab token lookup). SSH provider dispatch layer (for remote repos, template file reading) requires coordination with the platform subsystem. Consider initial Rust port reusing glab subprocess (simple drop-in) with follow-up native REST client migration._

**Capabilities**
- List/fetch merge requests with state filtering (opened/merged/closed/all)
- List/fetch issues with state and assignee filtering
- Combined work-item listing (MRs + issues) with unified sorting
- Create merge requests from local branches to specified base refs
- Create/update/comment on issues
- Merge MR with method selection (merge/squash/rebase)
- Close/reopen MRs and issues
- Update MR title, description, labels, and reviewers
- Add top-level and inline review comments on MRs
- Resolve/unresolve discussion threads
- Fetch MR detail including approval state, reviewers, and changed files
- Fetch pipeline job details and CI logs
- Retry failed pipeline jobs
- Fetch user's cross-project GitLab todos
- List project labels and assignable users
- Diagnose GitLab authentication status
- Resolve project references from git remotes (origin/upstream)
- Get rate limit status with per-host caching
- Parse GitLab URLs for MR/issue picker (paste-URL flow)
- Handle self-hosted GitLab instances via glab auth status

**Public API / IPC / RPC**
- getAuthenticatedViewer()
- diagnoseAuth()
- getRateLimit(options?)
- getProjectSlug(repoPath, connectionId?)
- getMergeRequest(repoPath, iid, connectionId?)
- getMergeRequestForBranch(repoPath, branch, linkedMRIid?, connectionId?)
- listMergeRequests(repoPath, state, page, perPage, preference?, query?, connectionId?)
- getWorkItemByProjectRef(repoPath, projectRef, iid, type, connectionId?)
- listWorkItems(repoPath, state, page, perPage, preference?, query?, connectionId?)
- listTodos(repoPath, connectionId?)
- closeMR(repoPath, iid, preference?, connectionId?, projectRef?)
- reopenMR(repoPath, iid, preference?, connectionId?, projectRef?)
- mergeMR(repoPath, iid, method?, preference?, connectionId?, projectRef?)
- addMRComment(repoPath, iid, body, preference?, connectionId?, projectRef?)
- addMRInlineComment(repoPath, iid, input, preference?, connectionId?, projectRef?)
- resolveMRDiscussion(repoPath, iid, discussionId, resolved, preference?, connectionId?, projectRef?)
- updateMRReviewers(repoPath, iid, reviewerIds, preference?, connectionId?, projectRef?)
- getJobTrace(repoPath, jobId, preference?, connectionId?, projectRef?)
- retryJob(repoPath, jobId, preference?, connectionId?, projectRef?)
- updateMR(repoPath, iid, updates, preference?, connectionId?, projectRef?)
- getIssue(repoPath, issueNumber, connectionId?)
- listIssues(repoPath, limit?, preference?, state?, assignee?, connectionId?)
- createIssue(repoPath, title, body, preference?, connectionId?)
- updateIssue(repoPath, issueNumber, updates, preference?, connectionId?, projectRefOverride?)
- addIssueComment(repoPath, issueNumber, body, preference?, connectionId?, projectRefOverride?)
- listLabels(repoPath, preference?, connectionId?)
- listAssignableUsers(repoPath, preference?, connectionId?)
- getWorkItemDetails(repoPath, iid, type, preference?, connectionId?, projectRefOverride?)
- IPC handlers: gitlab:viewer, gitlab:diagnoseAuth, gitlab:rateLimit, gitlab:projectSlug, gitlab:mrForBranch, gitlab:mr, gitlab:listMRs, gitlab:issue, gitlab:listIssues, gitlab:createIssue, gitlab:updateIssue, gitlab:addIssueComment, gitlab:listLabels, gitlab:listAssignableUsers, gitlab:listWorkItems, gitlab:workItemDetails, gitlab:closeMR, gitlab:reopenMR, gitlab:mergeMR, gitlab:updateMR, gitlab:updateMRReviewers, gitlab:addMRComment, gitlab:addMRInlineComment, gitlab:resolveMRDiscussion, gitlab:jobTrace, gitlab:retryJob, gitlab:todos, gitlab:workItemByPath

**External dependencies**
- glab CLI binary (spawned via child_process.execFile)
- git binary (via gitExecFileAsync from src/main/git/runner)
- GitLab REST API (network service via glab)
- SSH provider for remote repos (via getSshGitProvider)
- SSH filesystem provider (via getSshFilesystemProvider for template reading)

**Persistence**
- Process-level cache: projectRefCache (Map<string, ProjectRef | null>, max 512 entries)
- Process-level cache: gitLabRateLimitCache (Map<string, GitLabRateLimitSnapshot>, max 64 entries with 30s TTL)
- Process-level cache: knownHostsCache (glab auth status parsed hosts)
- In-flight deduplication: projectRefInFlight (Map<string, Promise<ProjectRef | null>>)
- Electron Store via Store interface: gitlabProjects { pinned: [], recent: [] } for project recents

**Cross-platform concerns**
- WSL support via wslDistro parameter passed to glabExecFileAsync
- Cross-platform glab spawning via resolveCommand from git/runner
- Self-hosted GitLab hostname targeting via --hostname flag to glab
- SSH-backed remote repos via connectionId (mobile via SSH tunnel)
- Fallback to local cwd-based glab inference when no connectionId

### `Linear SDK Integration (main-linear)`

Manages Linear API integration for Orca, providing authentication, workspace management, issue and project tracking across multiple workspaces. Handles encrypted token storage, concurrent API rate-limiting (max 4 parallel calls), and graceful degradation with workspace-level error reporting.

**Rust portability:** tier=`io` · effort=`L` · target=`linear-sdk-rust (or custom GraphQL client with async http, JSON serde)`  
_Core logic is pure data transformation and API orchestration; main IO dependencies are HTTP requests to Linear API (GraphQL) and filesystem reads/writes for credential storage. Electron safeStorage will need platform-specific native bindings (macOS Keychain, Windows DPAPI, Linux secret-tool). Concurrency limiter is a simple semaphore (easily ported). In-flight deduplication and request coalescing are pure async patterns. Would benefit from a Rust GraphQL client library and secure credential storage crate (keyring or similar). Pagination and multi-workspace aggregation logic is algorithm-heavy and pure, no platform dependencies._

**Capabilities**
- Multi-workspace credential management with encrypted token storage via Electron safeStorage
- Legacy workspace token migration to new workspace-based storage
- User authentication and connection validation against Linear API
- List, fetch, search, create, and update Linear issues with support for parent/child relationships
- Issue comments: fetch and add comments with user attribution
- Project querying: list, fetch projects with detailed metadata (milestones, resources, team members)
- Custom view support (issue and project models) with associated issue/project listing
- Team management: list teams, fetch team workflow states, labels, and members
- Concurrent API call rate limiting (max 4 parallel requests)
- Multi-workspace aggregation with sorted result merging (e.g., 'all workspaces' mode)
- Request deduplication via in-flight promise coalescing for identical queries
- Auto-clear tokens on 401 authentication errors with workspace-level error reporting
- Pagination with GraphQL cursor-based pagination for large result sets (max 50 items per Linear API page)

**Public API / IPC / RPC**
- connect(apiKey): Promise<{ok: true; viewer; workspace} | {ok: false; error}>
- disconnect(workspaceId?): void
- selectWorkspace(workspaceId): LinearConnectionStatus
- getStatus(): LinearConnectionStatus
- testConnection(workspaceId?): Promise<{ok: true; viewer; workspace} | {ok: false; error}>
- getIssue(id, workspaceId?): Promise<LinearIssue | null>
- searchIssues(query, limit?, workspaceId?): Promise<LinearIssue[]>
- listIssues(filter?, limit?, workspaceId?): Promise<LinearCollectionResult<LinearIssue>>
- createIssue(teamId, title, description?, workspaceId?, options?): Promise<{ok: true; id; identifier; title; url} | {ok: false; error}>
- updateIssue(id, updates, workspaceId?): Promise<{ok: true} | {ok: false; error}>
- addIssueComment(issueId, body, workspaceId?): Promise<{ok: true; id} | {ok: false; error}>
- getIssueComments(issueId, workspaceId?): Promise<LinearComment[]>
- listTeams(workspaceId?): Promise<LinearTeam[]>
- getTeamStates(teamId, workspaceId?): Promise<LinearWorkflowState[]>
- getTeamLabels(teamId, workspaceId?): Promise<LinearLabel[]>
- getTeamMembers(teamId, workspaceId?): Promise<LinearMember[]>
- listProjects(query?, limit?, workspaceId?, force?): Promise<LinearCollectionResult<LinearProjectSummary>>
- getProject(id, workspaceId, force?): Promise<LinearProjectDetail | null>
- listProjectIssues(projectId, limit?, workspaceId, force?): Promise<LinearCollectionResult<LinearIssue>>
- listCustomViews(model, limit?, workspaceId?, force?): Promise<LinearCollectionResult<LinearCustomViewSummary>>
- getCustomView(viewId, model, workspaceId, force?): Promise<LinearCustomViewSummary | null>
- listCustomViewIssues(viewId, limit?, workspaceId, force?): Promise<LinearCollectionResult<LinearIssue>>
- listCustomViewProjects(viewId, limit?, workspaceId, force?): Promise<LinearCollectionResult<LinearProjectSummary>>
- IPC channels: linear:connect, linear:disconnect, linear:selectWorkspace, linear:status, linear:testConnection, linear:searchIssues, linear:listIssues, linear:createIssue, linear:getIssue, linear:updateIssue, linear:addIssueComment, linear:issueComments, linear:listTeams, linear:listProjects, linear:getProject, linear:listProjectIssues, linear:listCustomViews, linear:getCustomView, linear:listCustomViewIssues, linear:listCustomViewProjects, linear:teamStates, linear:teamLabels, linear:teamMembers

**External dependencies**
- @linear/sdk npm package (LinearClient, AuthenticationLinearError, Issue, IssueSearchResult types)
- electron safeStorage API (Electron native: encryptString, decryptString, isEncryptionAvailable)
- Node.js fs module (readFileSync, writeFileSync, unlinkSync, existsSync, mkdirSync)
- Node.js path module (join, homedir)
- Node.js os module (homedir)

**Persistence**
- ~/.orca/linear-token.enc: Legacy encrypted Linear API token (safeStorage)
- ~/.orca/linear-viewer.json: Cached legacy viewer metadata (displayName, email, organizationName, organizationId, organizationUrlKey)
- ~/.orca/linear-workspaces.json: Active workspace metadata file (version, activeWorkspaceId, selectedWorkspaceId, workspaces array)
- ~/.orca/linear-tokens/: Directory containing per-workspace encrypted API tokens (base64url-encoded workspace ID as filename)
- In-memory token cache: Map<workspaceId, decryptedToken>
- In-memory viewer cache: LinearViewer | null (legacy)
- In-memory workspace file cache: LinearWorkspaceFile
- In-flight request deduplication: Map<cacheKey, Promise<T>> for projects, custom views

**Cross-platform concerns**
- macOS: Electron safeStorage uses Keychain for encryption
- Linux: Electron safeStorage uses secret-tool/pass for encryption (if available), falls back to plaintext warning
- Windows: Electron safeStorage uses DPAPI for encryption
- SSH: No direct support; tokens stored as plaintext with console warning when safeStorage unavailable
- File paths use Node.js path.join() for cross-platform compatibility
- Homedir discovery via os.homedir() (respects $HOME on Unix, %USERPROFILE% on Windows)

### `Orca main-root (Electron app lifecycle, service wiring, IPC handlers)`

Electron main process entry point and service orchestration: manages app lifecycle, persists state (repos, worktrees, settings, automations), wires and lifecycle-manages core services (store, runtime, PTY daemon, accounts, rate limits, telemetry), registers IPC handlers exposing ~40+ subsystems to renderer, handles window creation/focus, synthetic agent title spinners, crash breadcrumbs, updates, and agent-status hook notifications.

**Rust portability:** tier=`mixed` · effort=`XL` · target=`orca-main with Tauri or custom Electron-like wrapper + isolated service crates (orca-store, orca-runtime, orca-accounts, orca-rate-limits, orca-automations, orca-telemetry)`  
_index.ts is 1500+ lines weaving app lifecycle, window management (Electron-specific), IPC handler registration, service singletons, and complex startup/shutdown sequences. Rust port requires: (1) native window/app-lifecycle layer (Tauri, cosmic-edit, or thin Swift wrapper on macOS), (2) separate crates for stateful services (Store, Runtime, Accounts, RateLimits), (3) IPC re-architecting—Electron's ipcMain.handle() channel-based dispatch becomes explicit request-response enums or JSON-RPC over websocket, (4) memory-mapped or sled KV for persistence replacing orca-data.json (Electron.safeStorage becomes platform OS keychain via secret-service/Keychain APIs), (5) async runtime (tokio) for all I/O including PTY daemon fork comms, (6) macOS Cocoa for app lifecycle/window/dock, (7) Linux systemd and Windows Power Management P-Invoke bindings via winapi/windows crates, (8) git2-rs for worktree ops (replacing child_process calls), (9) ssh2-rs for SSH provider dispatch, (10) full test harness to cover the ~100 service interaction states currently held in module-level variables_

**Capabilities**
- App lifecycle (ready, quit, activate, app-quit guards)
- Data persistence via encrypted Store (orca-data.json with 5-rolling-backup rollback), workspace sessions, repos, worktrees, automation history, onboarding state, settings
- Global settings management (workspace dirs, SSH targets, keybindings, notifications, appearance, feature flags)
- Runtime metadata hydration and persistence (orca-runtime.json, runtime ID, commit-message resolvers)
- Service initialization and wiring (OrcaRuntimeService, PTY daemon, ClaudeAccounts, CodexAccounts, RateLimitService, StatsCollector, AutomationService, AgentAwakeService, KeybindingService, CrashReportStore)
- IPC handler registration (40+ handler modules: app, cli, filesystem, github, gitlab, linear, jira, settings, runtime, automations, keybindings, telemetry, diagnostics, browser, speech, etc.)
- Main window creation/management with WebGL GPU features, theme sync, devTools, reload guards
- Synthetic agent-status title spinners (decorative working labels for agents via OSC 0 sequences, terminal idle/permission title detection)
- Agent status hook server integration (listens to agent hook events, drives synthetic titles, broadcasts to renderer via agentStatus:set/clear)
- Crash reporting breadcrumb pipeline (coalesced agent-state breadcrumbs, process-gone crash classification and deduplication, renderer/child crash detection)
- Update checking and installation (electron-updater wrapper, prerelease feeds, nudge prompts, macOS Squirrel installer)
- System sleep assertion (keep computer awake during agent runs: powerSaveBlocker, macOS pmset, Linux systemd-logind)
- Single-instance lock enforcement (prevents concurrent launches, routes second instance to existing window)
- Proxy settings application (Electron NetworkDelegate)
- Dev-mode utilities (parent process disconnect/watchdog coupling, shell PATH hydration, startup diagnostics)
- Hook file parsing and runner script generation (orca.yaml, setup/archive hooks, issue commands, cross-platform bash/cmd/pwsh runners)
- Browser session initialization for webviews and runtime profiles
- Keybindings service initialization and menu integration

**Public API / IPC / RPC**
- ipcMain handler channels: app:* (open, isPackaged, platform, isDev), runtime:* (list-repos, repo-add, repo-open, worktree-create, worktree-remove), settings:* (get, update, themes, notifications), session:* (save, load, restore), automations:* (create, delete, run, dispatch), github:* (authorize, get-repos), gitlab:* (authorize), claude-accounts:* (add, remove), codex-accounts:* (add, remove), filesystem:* (read, write, stat, move), rate-limits:* (get, check), keybindings:* (get, update), browser:* (navigate, create-session), speech:*, ui:*, telemetry:*, crash-reporting:*
- Event emitters from renderer: ui:openSettings, ui:openFeatureTour, ui:openSetupGuide, ui:openCrashReport, terminal:zoom, pty:data (synthetic title writes)
- Agent hook event listener (paneKey, tabId, worktreeId, connectionId, payload with agentType/state, receivedAt, stateStartedAt)
- Store public methods: getRepo(), getSettings(), updateSettings(), getWorktreeMeta(), setWorktreeMeta(), getWorktreeIdForTab(), flush()
- OrcaRuntimeService public methods: getRuntimeId(), getCommitMessageAgentEnvironmentResolvers(), notifyBranchRenamed(), getAgentStatusTerminalHandleForPaneKey(), getAgentBrowserBridge(), getAgentStatusOrchestrationContextForPaneKey()
- OrcaRuntimeRpcServer.getWebSocketEndpoint(), createPairingOffer()
- BrowserManager.setSettingsResolver()

**External dependencies**
- electron (app, BrowserWindow, nativeTheme, safeStorage, powerMonitor, powerSaveBlocker)
- electron-updater (auto-updater)
- @electron-toolkit/utils (electronApp, is)
- node-pty (or daemon fork: pty-subprocess)
- yaml parser
- child_process (exec, execFile for hooks and git)
- @parcel/watcher (filesystem watcher)
- qrcode (terminal QR rendering for pairing)
- Electron observability/tracer (custom span creation)
- git CLI (gitExecFileSync for worktree ops)
- system APIs: PowerMonitor (resume), PMSet (macOS), systemd-logind (Linux)
- electron-reloader (dev mode)
- ssh2 library (indirectly via SSH providers)
- better-sqlite3 (indirectly via external providers)
- WSL CLI (wsl.exe for hook execution on WSL worktrees)

**Persistence**
- orca-data.json: full persisted state (repos, worktrees, workspace session, settings, automations, onboarding, UI state, notifications, keybindings, feature interactions, project groups, SSH targets, telemetry consent)
- 5 rolling backups (.bak.1 through .bak.5) refreshed every 60min
- orca-runtime.json: runtime ID, live pane-status cache (reloaded at startup)
- terminal-history/ and terminal-history-wsl/: per-worktree per-shell bash/zsh history files
- Crash report store (local trace lane breadcrumbs and crash details)
- Stats collector (performance metrics)
- Claude/Codex usage stores (token/call tracking)
- Agent hook server state (pane-key aliases, migration-unsupported PTY entries, legacy numeric pane-key bridges)
- Browser session storage (cookies, cache)
- Terminal scrollback snapshots (refs in layout, actual blobs persisted separately)

**Cross-platform concerns**
- macOS: Squirrel.Mac installer, pmset sleep assertions, native theme integration, macOS app dock badge updates, Cmd+Q quit handling
- Linux: systemd-logind lid-switch sleep assertions, /proc-based process monitoring
- Windows: DACL ACL management for userData folder (Chromium resets it), wsl.exe hook execution, PowerShell/cmd runner scripts, git-bash detection and PATH hydration, ComSpec shell resolution
- WSL: dual-path normalization (Windows UNC <-> Linux), wsl.exe bridge for hook execution, separate terminal-history roots per distro, SSH remote PTY provider dispatch
- SSH: relay-routed PTY streams, remote filesystem walks, remote worktree enumeration, remote branch renames (via agent environment resolvers)
- All: XDG_CONFIG_HOME fallback for Linux, ~ expansion, symlink resolution for workspace dirs, locale-aware path comparisons

### `SSH (main-ssh) Remote Transport & Relay Multiplexer`

Provides SSH connection lifecycle management, remote relay deployment, JSON-RPC multiplexing over SSH, and cross-platform remote execution (ProxyCommand/ProxyJump/system-ssh fallback). Manages connections to remote hosts, coordinates Orca relay installation/launch, and multiplexes concurrent PTY/filesystem/git operations over single SSH channels.

**Rust portability:** tier=`io` · effort=`XL` · target=`orca-ssh-transport (new workspace crate); dependencies: ssh2-rs (or fork/native libssh2), tokio, bytes, tracing, thiserror, serde_json, tempfile (for relay staging), flate2 (tar.gz compression), which (bin search) + system OpenSSH binary invoke via std::process`  
_Core JSON-RPC framing is pure (FrameDecoder state machine); SSH connection is IO-heavy (ssh2-rs or native libssh2 binding needed). Credential prompt callback requires IPC bridge to UI. Relay deployment is sequential (upload, npm install, execute) — each step blocks. System SSH fallback requires child_process spawn; tar/npm invocation. Port forwarding uses net::TcpListener + forwarding channels. Multiplexer keepalive/timeout timers replicate to tokio::time. Biggest portability gap: ssh2 Rust crate maturity (ssh2-rs is actively maintained but less complete than Node ssh2 library). Credential prompting and UI callbacks must be async-compatible. ProxyCommand socket piping may need careful Duplex simulation in Rust (likely io::DuplexStream or custom Duplex)._

**Capabilities**
- SSH config parsing and token resolution via ssh -G
- OpenSSH authentication (keys, agent sockets, passphrases, passwords)
- Private key encryption detection and passphrase prompting
- Connection retry with transient/auth error classification
- ProxyCommand/ProxyJump spawning and Duplex socket bridging
- System ssh fallback (FIDO2, ControlMaster, ProxyUseFdpass)
- Relay deployment: version detection, SFTP upload, npm install coordination
- 13-byte framed JSON-RPC protocol (VS Code PersistentProtocol format)
- Request/response multiplexing with keepalive and timeout management
- Relay sentinel detection and version mismatch error handling
- SFTP file operations (upload directory, write file, symlink rejection)
- SSH port forwarding (TCP local→remote mapping)
- Remote port scanning via relay /proc/*/fd walking
- Credentials caching across reconnection cycles
- Reconnection backoff with grace-period PTY reattachment
- SSH agent identity filtering (identitiesOnly support)
- System process spawning for remote command execution

**Public API / IPC / RPC**
- class SshConnection
- class SshConnectionManager
- class SshChannelMultiplexer
- class SshRelaySession
- class SshPortForwardManager
- class PortScanner
- class FrameDecoder
- deployAndLaunchRelay(conn, onProgress?, graceTimeSeconds?, relayInstanceId?)
- waitForSentinel(channel)
- execCommand(conn, command)
- uploadDirectory(sftp, localDir, remoteDir, rootRealPath?, options?)
- uploadFile(sftp, localPath, remotePath, options?)
- sftpPathExists(sftp, remotePath)
- parseSshConfig(content)
- resolveWithSshG(configHost)
- buildConnectConfig(target, resolved, options?)
- findSystemSsh()
- spawnSystemSsh(target)
- spawnSystemSshCommand(target, command)
- uploadDirectoryViaSystemSsh(target, localDir, remoteDir, options?)
- writeFileViaSystemSsh(target, remotePath, contents, options?)
- resolvePrivateKey(target, resolved)
- resolveAgentSocket(target, resolved)
- runRemoteOrcaCli(runtime, request)
- encodeFrame(type, id, ack, payload)
- encodeJsonRpcFrame(msg, id, ack)
- parseJsonRpcMessage(payload)
- parseUnameToRelayPlatform(os, arch)

**External dependencies**
- ssh2 (Client, utils.parseKey, BaseAgent, ClientChannel, SFTPWrapper, ConnectConfig)
- child_process (spawn, ChildProcess)
- net (createServer, Socket)
- stream (Duplex, pipeline)
- fs (readFileSync, writeFileSync, lstat, readdir, open)
- path (join, relative, sep, isAbsolute, homedir)
- os (homedir)
- electron (app)
- node:crypto (indirectly via ssh2 key parsing)
- system binaries (ssh, tar, npm, node)

**Persistence**
- SSH connection state (SshConnectionState: targetId, status, error, reconnectAttempt)
- Relay session state (RelaySessionState: idle/deploying/ready/reconnecting/disposed)
- Credential cache (passphrase, password per connection)
- Port forward entries (id, localPort, remoteHost, remotePort, label)
- Relay version lock file remote side (.orca-remote/INSTALL_LOCK)
- Relay instance socket path (.orca-remote/orca-relay-{instanceId}.sock)

**Cross-platform concerns**
- macOS: /opt/homebrew/bin/ssh, /usr/local/bin/ssh, system SSH with FIDO2 key support
- Linux: /usr/bin/ssh, system SSH
- Windows: C:\Windows\System32\OpenSSH\ssh.exe, OpenSSH agent pipe (\\\\.\\pipe\\openssh-ssh-agent)
- SSH_AUTH_SOCK environment variable for Unix domain sockets vs Windows named pipes
- ProxyUseFdpass support (macOS/Linux OpenSSH only)
- Relay platform detection: linux-x64, linux-arm64, darwin-x64, darwin-arm64

### `agent-hooks`

Manages the agent status lifecycle via HTTP-based hook listeners that receive lifecycle events from agent processes (Claude, Codex, Gemini, Cursor, Droid, etc.). Central subsystem for tracking agent state transitions (working/waiting/done), terminal output, tool use, permissions, and interrupts across local and SSH-tunneled agents. Supports managed hook installation/removal into agent config files.

**Rust portability:** tier=`io` · effort=`L` · target=`orca_agent_hooks`  
_Core subsystem is pure event cache + JSON I/O with optional HTTP loopback. Major components: (1) HTTP server + bearer token auth can use tokio + hyper or axum (minimal deps vs current Node http module), (2) on-disk JSON cache (last-status.json) is straightforward serde_json + atomic fs operations, (3) hook script generation and installer is text manipulation + fs write, easily portable, (4) SFTP support already uses ssh2 crate in Rust ecosystem, (5) telemetry.track() calls can be replaced with event struct emission to main backend, (6) IPC fanout can become Tauri commands. State machines (status normalization, identity resolution) are logic-only. Risk: assistant message retry scheduling is a per-pane timer cascade — needs careful tokio::spawn or async task management. Dependency on telemetry and runtime enrichment can be abstracted to traits. Major lifting: first-work branch rename orchestrates git CLI, text generation agent, branch rename — relatively isolated module but requires git/ssh context (can stay Electron-based or port selectively). Effort is L because no UI rebuild, no native deps (ssh2 already in Rust), primary work is architectural layering._

**Capabilities**
- Local HTTP loopback server on random port for receiving agent status POST events
- Bearer token authentication for hook requests
- Persistent on-disk cache of last agent status per pane (survives app restart)
- Legacy numeric-to-stable pane key aliasing during pane ID migration
- Agent status state machine normalization and identity tracking
- Telemetry emission for prompt-sent and install failures
- TTL-based expiration of cached statuses (7 days)
- Managed hook script generation and installation for 12+ agent types
- Remote SFTP-backed hook installer for SSH targets
- Automatic branch renaming on first agent work based on generated summaries
- Migration state tracking for unsupported PTY configurations
- Assistant message retry scheduling for delayed transcript flush
- Status change listener subscription for dashboard reflow
- Pane key alias persistence to localStorage
- Migration safe-guard for PTY teardown and crash recovery

**Public API / IPC / RPC**
- AgentHookServer.start(options)
- AgentHookServer.stop()
- AgentHookServer.setListener(callback)
- AgentHookServer.setPaneStatusClearListener(callback)
- AgentHookServer.subscribeStatusChanges(listener) -> unsubscribe
- AgentHookServer.getStatusSnapshot() -> AgentStatusIpcPayload[]
- AgentHookServer.inferInterrupt(request) -> boolean
- AgentHookServer.ingestRemote(envelope, connectionId)
- AgentHookServer.registerPaneKeyAlias(legacyKey, stableKey, ptyId)
- AgentHookServer.clearPaneKeyAliasesForPty(ptyId, options)
- AgentHookServer.dropStatusEntry(paneKey)
- AgentHookServer.clearPaneState(paneKey)
- AgentHookServer.buildPtyEnv() -> env vars
- agentHookServer singleton export
- IPC: agentStatus:getSnapshot (invokable)
- IPC: agentStatus:inferInterrupt (invokable)
- IPC: agentStatus:drop (fire-and-forget)
- IPC: agentStatus:getMigrationUnsupportedSnapshot (invokable)
- IPC: agentHooks:{agent}Status (12 per-agent handlers for status)
- installManagedAgentHooks()
- removeManagedAgentHooks()
- getManagedAgentHookStatuses()
- applyAgentStatusHooksEnabled(enabled)
- maybeAutoRenameBranchOnFirstWork(event, deps)
- installRemoteManagedAgentHooks(sftp, remoteHome)
- setMigrationUnsupportedPtyListener()
- getMigrationUnsupportedPtySnapshot()
- setMigrationUnsupportedPty()
- clearMigrationUnsupportedPty()

**External dependencies**
- http (Node.js core)
- crypto (Node.js core)
- fs (Node.js core)
- path (Node.js core)
- os (Node.js core)
- ssh2 (SFTPWrapper for remote installations)
- electron (ipcMain for IPC handlers)
- ../telemetry/client track() for event emission

**Persistence**
- last-status.json: JSON file in userData/agent-hooks/ with version 2 schema, map of paneKey->EnrichedAgentHookEventPayload, TTL-filtered on load (7 days), atomic write via tmp+rename
- endpoint file: written to userData/agent-hooks/orca_agent_hook_endpoint.json (or namespaced in dev), contains port/token/env/version for hook scripts to discover
- .bak backup: rotating single backup of each agent config during hook install
- legacy pane key aliases: Map<string, PaneKeyAliasEntry> persisted via setPaneKeyAliasPersistenceListener callback
- migration unsupported PTY state: in-memory Map<string, MigrationUnsupportedPtyEntry> with snapshot persistence via listener

**Cross-platform concerns**
- Windows: grantDirAcl fallback when EPERM on script write, ACL retry in writeScriptWithAclRetry
- Windows: special PowerShell UTF-8 encoding handling for hook POST bodies (CJK prompt support)
- Windows: buildWindowsAgentHookPostCommand() generates UTF-8-aware Invoke-WebRequest
- POSIX: wrapPosixHookCommand() with [ -x ] guard to silently skip missing/non-executable scripts
- POSIX: chmod 0o755 for managed scripts
- SSH: SFTP-only remote install (no Windows SSH support in v1)
- macOS/Linux/Windows: different hook script paths (~/.orca/agent-hooks/ vs userData-specific), endpoint file location handling

### `computer-use`

Provides native computer automation on macOS (via helper app socket) and Linux/Windows (via scripted desktop automation). Manages app discovery, window listing, UI element interaction (click, drag, scroll, type, hotkey), screenshot capture, and permission handling for accessibility control.

**Rust portability:** tier=`mixed` · effort=`L` · target=`computer-use crate wrapping os-specific providers (orc-macos-computer-use, orc-desktop-script)`  
_Platform abstraction layer is solid: generic provider interface (capabilities, listApps, listWindows, snapshot, action) with macOS native provider and desktop script provider as swappable backends. Rust port can replicate this: trait-based provider dispatch. FFI required for macOS only (native helper app via socket; osascript via shell is replaced with Swift NSWorkspace + Accessibility APIs). Desktop script providers (Python/PS1 on Linux/Windows) can be ported to Rust binaries. IPC complexity (socket framing, JSON line protocol, request buffering, timeout handling, cleanup) is pure Rust-friendly. Permission handling depends on native platform APIs (TCC on macOS, registry on Windows, dbus/systemd on Linux). Snapshot caching is generic. Main challenge: native macOS helper app remains Objective-C/Swift, Rust talks to it via socket—no change needed unless full rewrite planned._

**Capabilities**
- Fork and manage native helper subprocess via stdio/socket IPC
- macOS app enumeration via osascript JXA
- macOS native provider protocol handshake and capability detection
- Snapshot UI state (accessibility tree, element metadata, screenshots)
- UI interaction actions: click, right-click, scroll, drag, typeText, pressKey, hotkey, pasteText, setValue
- Cross-platform desktop script invocation (Python on Linux, PowerShell on Windows)
- Snapshot caching with LRU eviction (32 max cached entries)
- Socket-based request-response protocol with JSON line format
- Graceful provider shutdown with pending request cleanup
- Permission setup and status probing for Accessibility and Screenshot capture on macOS
- Keyboard chord parsing with platform-aware modifier normalization

**Public API / IPC / RPC**
- callComputerSidecarListApps()
- callComputerSidecarCapabilities()
- callComputerSidecarListWindows(params)
- callComputerSidecarSnapshot(params)
- callComputerSidecarAction(method, params)
- shouldUseComputerSidecar()
- resetComputerSidecarForTest()
- shouldUseMacOSNativeProvider()
- shouldUseDesktopScriptProvider()
- openComputerUsePermissions(permissionId?)
- getComputerUsePermissionStatus()
- resetComputerUsePermissions()
- listMacOSApps()
- parseKey(input)
- notifyPermissionRequired(instructions)
- MacOSNativeProviderClient.capabilities(), listApps(), listWindows(params), snapshot(params), action(method, params), shutdown()
- DesktopScriptProviderClient.capabilities(), listApps(), listWindows(params), snapshot(params), action(method, params)

**External dependencies**
- child_process (fork, spawn, execFile, execFileSync, spawnSync)
- fs/fs.promises (mkdtemp, mkdtempSync, writeFile, writeFileSync, readFile, chmodSync, rmSync, rm, existsSync)
- path (join, resolve)
- os (tmpdir, release)
- net (Socket, createConnection)
- crypto (randomUUID)
- electron (app, shell, Notification)
- osascript (shelled out via execFile)
- /usr/bin/open (shelled out for permission dialogs)
- /usr/bin/pkill (shelled out to kill helpers)
- desktop-script provider (Python executable on Linux, PowerShell on Windows)

**Persistence**
- Temp directories: mkdtemp for socket paths, operation requests, status probes (all cleaned up via rmSync/rm)
- Socket path and token files in /tmp/orca-computer-use-* (ephemeral per session)
- No permanent on-disk state

**Cross-platform concerns**
- macOS: native helper app (Orca Computer Use.app), osascript JXA, TCC/accessibility permissions, LaunchServices, socket-based transport
- Linux: Python desktop automation script (runtime.py), execFile via subprocess
- Windows: PowerShell desktop automation script (runtime.ps1), execFile via subprocess
- Darwin version check (macOS 14+) for native provider availability
- Platform-specific permission handling (unsupported on non-macOS)
- ELECTRON_RUN_AS_NODE and asar handling for packaged sidecar paths

### `main-agent-integrations`

Per-agent CLI integration adapters that manage hook installation/removal and status tracking for multiple external developer agents (Claude, GitHub Copilot, Cursor, Gemini, Droid, Grok, Antigravity, Amp, Command-Code, OpenCode, OMP, OpenClaude). Each adapter generates platform-specific hook scripts that POST session events to the unified agent-hooks HTTP server via loopback, enabling the Orca dashboard to show real-time status (working/idle/waiting) across all agents.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca-agent-hooks (new crate), leveraging tokio for async SFTP, ssh2-rs bindings, and walkdir for safe overlay mirroring`  
_Most of the subsystem is pure I/O (JSON parsing, file mirroring, script generation) — no DOM/React. Moderate complexity: platform-specific script generation for Windows (PowerShell/cmd.exe) and POSIX (sh/curl), SFTP remote operations, endpoint-file caching logic. Plugin source code is string literals emitted into files, so keeping them as strings in Rust is fine (no template evaluation needed). Hook script generation is deterministic and testable. Symlink/junction handling requires platform-aware fs calls. SSH support requires ssh2-rs for SFTP. Hook installation state tracking is straightforward enum matching. The Pi/OMP overlay source generation and endpoint-file caching are pure functions that map well to Rust. Key risk: precise Windows PowerShell encoding/quoting semantics (UTF-8 byte sequences, ConvertTo-Json depth parameter, -LiteralPath escaping) must match Electron behavior exactly, so use property-based testing against existing app to validate. Minor risk: Antigravity's per-event wrapper script generation order and Windows cmd escaping. Consider leveraging existing ssh2-rs and tokio-native async SFTP instead of Node's ssh2 binding for better thread-safety._

**Capabilities**
- Install/remove/query status of managed hook definitions in agent config files (hooks.json, settings.json)
- Generate platform-specific hook scripts (PowerShell .cmd on Windows, POSIX .sh on Unix) that wrap curl/POST requests to http://127.0.0.1:PORT/hook/{agent-name}
- Manage overlay directories for OpenCode and Pi/OMP that inject Orca-owned plugins alongside user extensions without clobbering them
- Support SSH remote agent hook installation via SFTP wrapper functions (readHooksJsonRemote, writeHooksJsonRemote)
- Read endpoint files (ORCA_AGENT_HOOK_ENDPOINT) that contain live server port/token, enabling hook scripts to survive Orca restarts
- Cache endpoint file contents by mtime+size+inode to avoid re-parsing on every streaming event
- Forward agent-specific events (SessionBusy, SessionIdle, PermissionRequest, MessagePart, MessageEnd, ToolExecution, etc.) to /hook/{agent-name} POST endpoint with paneKey/tabId/worktreeId metadata
- Validate hook script paths against managed-command filename patterns to detect stale vs. current installations
- Sweep managed entries from no-longer-subscribed agent events to prevent accumulation of defunct hooks on app upgrades
- Differentiate tool-use and permission-request schema variations across agents (Claude nested hooks[], Cursor top-level command, Antigravity bundle-scoped PreToolUse, Gemini single event chain)
- Support Antigravity's Windows-wrapper pattern (event-specific .cmd files wrapping a shared core)
- Support OpenCode plugin-source generation with environment variable parsing, child-session filtering (via client.session.list() lookup), and message-role caching
- Support Pi/OMP extension source generation with agent kind detection (runtime executable name checking for omp vs. pi), prefill extension for draft submission, and titlebar-spinner animation
- Mirror user config files/extensions into overlay directories while preserving symlinks for live edits on POSIX
- Generate settings.json overlays with UI-only safeguards (mergePiOverlayUiSettings)
- Hash PTY IDs to stable 32-char hex filenames to decouple overlay directory names from filesystem paths
- Output hook definitions in agent-specific schema shapes (e.g. Droid's definition.hooks[{type,command}] vs. top-level command)

**Public API / IPC / RPC**
- OpenCodeHookService.buildPtyEnv(ptyId, existingConfigDir): Record<string, string>
- OpenCodeHookService.clearPty(ptyId): void
- CopilotHookService.getStatus(): AgentHookInstallStatus
- CopilotHookService.install(): AgentHookInstallStatus
- CopilotHookService.installRemote(sftp, remoteHome): Promise<AgentHookInstallStatus>
- CopilotHookService.remove(): AgentHookInstallStatus
- CursorHookService.{getStatus,install,installRemote,remove}(): AgentHookInstallStatus
- DroidHookService.{getStatus,install,remove}(): AgentHookInstallStatus
- GeminiHookService.{getStatus,install,installRemote,remove}(): AgentHookInstallStatus
- AntigravityHookService.{getStatus,install,installRemote,remove}(): AgentHookInstallStatus
- AmpHookService.{getStatus,install,installRemote,remove}(): AgentHookInstallStatus
- CommandCodeHookService.{getStatus,install,installRemote,remove}(): AgentHookInstallStatus
- GrokHookService.{getStatus,install,installRemote,remove}(): AgentHookInstallStatus
- PiTitlebarExtensionService.buildPtyEnv(ptyId, existingAgentDir, kind): Record<string, string>
- PiTitlebarExtensionService.clearPty(ptyId): void
- openCodeHookService (singleton export)
- piTitlebarExtensionService (singleton export)
- copilotHookService (singleton export)
- cursorHookService (singleton export)
- droidHookService (singleton export)
- geminiHookService (singleton export)
- antigravityHookService (singleton export)
- ampHookService (singleton export)
- commandCodeHookService (singleton export)
- grokHookService (singleton export)
- openClaudeHookService (singleton export, delegates to ClaudeHookService)
- ORCA_PI_PREFILL_ENV_VAR export: environment variable name for Pi prefill
- ORCA_OMP_PREFILL_ENV_VAR export: environment variable name for OMP prefill
- isSafeDescendCandidate export from Pi service (re-export for test contract)
- AgentHookInstallStatus type: {agent, state, configPath, managedHooksPresent, detail}
- AGENT_HOOK_TARGETS constant: ['claude', 'openclaude', 'codex', 'gemini', 'antigravity', 'amp', 'cursor', 'droid', 'command-code', 'grok', 'copilot', 'hermes']

**External dependencies**
- ssh2 (SFTPWrapper type for SSH SFTP operations)
- curl (shelled via child_process in generated POSIX hook scripts)
- powershell.exe (shelled via cmd.exe on Windows in generated hook scripts)
- Electron app.getPath('userData') (for overlay/config directory paths)
- Node.js fs module: readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync, statSync, unlinkSync, realpathSync, lstatSync, symlinkSync/junction
- Node.js path module: join, basename, dirname, homedir
- Node.js crypto: createHash('sha256')
- Node.js process.env (endpoint file path, Orca hook coordinates)
- Node.js child_process via http.post (generated script posts via curl/powershell)

**Persistence**
- Agent config files: ~/.claude/hooks.json, ~/.copilot/hooks/orca.json, ~/.cursor/hooks.json, ~/.gemini/config/hooks.json (.gemini/settings.json), ~/.factory/settings.json (Droid/Factory)
- Managed hook scripts: ~/.orca/agent-hooks/{agent-name}-hook.{sh,cmd,ps1} (shared across PTYs)
- Overlay directories: ORCA user-data/{opencode-config-overlays,pi-agent-overlays,omp-agent-overlays}/{hash-of-source-path}/
- Overlay manifests: {overlay-dir}/.orca-opencode-overlay-manifest.json (top-level and plugin entry tracking)
- Pi/OMP settings: {overlay-dir}/settings.json (UI safeguards merged from source)
- Endpoint files: temporary env-var path ORCA_AGENT_HOOK_ENDPOINT (written by main server at startup, read by scripts to poll live port/token)
- Windows wrapper scripts: Antigravity-specific event wrappers like antigravity-pre-invocation.cmd

**Cross-platform concerns**
- Windows hook execution: PowerShell (-ExecutionPolicy Bypass, UTF-8 encoding setup, ConvertFrom-Json, Invoke-WebRequest)
- Windows hook syntax generation: powershell.exe -NoProfile vs. cmd.exe-native @echo off / set / call sequences
- POSIX hook execution: sh -c with curl, env-var sourcing via . (dot), conditional fd checks
- Path handling: Junction vs. symlink behavior on Windows (only junctions can traverse symlinks), realpathSync resolution
- Endpoint file parsing: dual-mode KEY=VALUE (POSIX) and set KEY=VALUE (Windows cmd)
- SSH remote installs: always POSIX .sh scripts regardless of local OS (ssh2 SFTP)
- Worktree path handling: filesystem-path-containing worktreeIds hashed to stable 32-char names to avoid path-separator issues
- Temp directory cleanup: Windows locking by antivirus/indexers (EPERM/EBUSY) requires best-effort teardown

### `main-automations`

Implements scheduled and manual automation execution with precheck validation, state tracking, and integration with external cron schedulers (Hermes, OpenClaw). Handles dispatching automations to the renderer, collecting usage metrics from Claude/Codex providers, and managing local/SSH execution targets.

**Rust portability:** tier=`io` · effort=`M` · target=`orca_automations in orca_backend workspace; depends on tokio for async execution, sqlx for SQLite reads, ssh2-rs for SSH, and serde for serialization`  
_The subsystem is primarily I/O bound (filesystem, subprocess, SSH, SQLite) with moderately complex business logic for scheduling (rrule parsing via external shared code), state merging (output + session logs), and timeouts. Async Rust with tokio is suitable. Child process spawning (local precheck) maps to std::process or tokio::process. SSH channel execution (ssh2::ClientChannel) exists in Rust. Hermes output markdown parsing and session log merging is pure logic. Main complexity is precise precheck timeout enforcement across platforms and graceful process tree cleanup (equivalent to taskkill -t behavior on Windows). SQLite reads can use rusqlite or sqlx in read-only mode. External CLI calls (hermes/openclaw) should spawn rather than shell out for security._

**Capabilities**
- Schedule and execute automations at specified intervals with rrule-based cron expressions
- Dispatch automations to renderer with real-time execution status updates
- Run precheck validation commands (local and SSH) before automation execution with timeout enforcement
- Discover and list external automation managers (Hermes on local/SSH, OpenClaw on local/SSH)
- Create, update, pause, resume, run, and delete external cron jobs via CLI (hermes/openclaw commands)
- Paginate and display Hermes cron run history from markdown output files and SQLite session databases
- Merge Hermes output markdown logs with session transcripts for complete run history
- Collect usage metrics (token counts, cost) from Claude and Codex automation runs within time windows
- Handle missed run grace periods and prevent duplicate runs within missed grace windows
- Support multiple execution targets (local, SSH) with remote relay fallback for external job management
- Snapshot workspace display names for historical automation run records
- Detect and handle unavailable windows (no renderer, SSH disconnected, interactive auth needed)

**Public API / IPC / RPC**
- AutomationService.start()
- AutomationService.stop()
- AutomationService.setWebContents(webContents: WebContents | null)
- AutomationService.setRendererReady()
- AutomationService.runNow(automationId: string): Promise<AutomationRun>
- AutomationService.runPrecheck(automationId: string, runId: string): Promise<AutomationPrecheckResult | null>
- AutomationService.markDispatchResult(result: AutomationDispatchResult): Promise<AutomationRun>
- listExternalAutomationManagers(store: Store): Promise<ExternalAutomationManager[]>
- listExternalAutomationRuns(input: ExternalAutomationRunsInput): Promise<ExternalAutomationRunsPage>
- createExternalAutomation(input: ExternalAutomationCreateInput): Promise<void>
- updateExternalAutomation(input: ExternalAutomationUpdateInput): Promise<void>
- runExternalAutomationAction(input: ExternalAutomationActionInput): Promise<void>
- runAutomationPrecheck(args: { precheck: AutomationPrecheck; target: AutomationPrecheckExecutionTarget }): Promise<AutomationPrecheckResult>
- readHermesCronOutputRunsPage(jobId: string, { page, pageSize }): Promise<HermesCronOutputRunsPage>
- readHermesCronOutputRuns(jobId: string): Promise<unknown[]>
- clearHermesCronOutputRunCountCache(jobId?: string): void
- IPC: automations:list
- IPC: automations:listRuns
- IPC: automations:listExternalManagers
- IPC: automations:listExternalRuns
- IPC: automations:create
- IPC: automations:update
- IPC: automations:delete
- IPC: automations:runNow
- IPC: automations:runPrecheck
- IPC: automations:createExternal
- IPC: automations:updateExternal
- IPC: automations:runExternalAction
- IPC: automations:markDispatchResult
- IPC: automations:snapshotWorkspaceName
- IPC: automations:rendererReady
- IPC broadcast (send): automations:dispatchRequested

**External dependencies**
- child_process (execFile, spawn)
- fs (existsSync, readdir, readFile, open, stat, realpath)
- fs/promises (readdir, readFile, open, stat, realpath)
- path (join, isAbsolute, relative, resolve)
- os (homedir)
- node:sqlite (DatabaseSync via sync-database adapter)
- electron (WebContents, ipcMain)
- ssh2 (ClientChannel type)
- process.env (HERMES_HOME, PATH detection via process.platform)
- process.kill (SIGTERM, SIGKILL for process tree management)

**Persistence**
- SQLite (read-only): ~/.hermes/state.db for Hermes cron session transcripts and messages
- Filesystem (read-only): ~/.hermes/cron/output/*.md for Hermes run output markdown files
- Filesystem (read-only): ~/.openclaw/cron/jobs.json for OpenClaw job configuration
- Filesystem (read-only): ~/.hermes/cron/jobs.json for Hermes job configuration
- Store abstraction (Backend persistence): Automation definitions, AutomationRun records, workspace snapshots

**Cross-platform concerns**
- Process tree management: taskkill on Windows, kill/process.kill on Unix-like systems
- PATH lookup: 'where' command on Windows, 'which' on Unix-like systems
- Shell invocation: detached process groups on non-Windows, foreground on Windows (via spawn options)
- SSH: ssh2 library handles platform-agnostic remote execution
- File paths: normalizes using path.join, path.isAbsolute, path.resolve with platform-aware sep

### `main-browser`

Manages embedded Chromium browser instances for agent-driven automation and user browsing. Handles guest session lifecycle, security policies, devtools/CDP integration, download management, media access, WebAuthn, cookie import, and real-time screencast streaming with grab-mode overlay for element selection.

**Rust portability:** tier=`mixed` · effort=`XL` · target=`orca-browser (wrapper around chromium-embedded | tauri-webview-api; agent-browser CLI remains native binary; SQLite cookie management via rusqlite; system media access via platform crates)`  
_Core challenge is Chromium embedding. Orca currently uses Electron (Node.js + Chromium). Pure Rust rewrite would require one of: (1) tauri webview (loses direct Chromium/CDP control), (2) embed chromium-gn or use playwright-rs (high complexity, protocol fidelity risk), (3) call out to system browser via WebDriver (loses local control). Screencast, viewport emulation, CDP introspection, and anti-detection script injection are tightly coupled to Chromium internals. The agent-browser native binary (Rust, handles browser commands via CDP) can be vendored/reused. Session partition isolation, user-agent cleanup, and cookie import logic are portable. Grab overlay, shortcut forwarding, media access delegation, and downloadhandling are Electron-specific integration that would need refactoring for Swift or alternative browser embedding. Recommend: keep agent-browser binary stable; extract session/permission/download logic to shared Rust lib; implement minimal Swift/Cocoa webview bridge (loses full automation fidelity); or use tauri as compatibility layer (limited to web APIs, not CDP)._

**Capabilities**
- Browser guest registration, attachment, and lifecycle management
- Security policy enforcement (permissions, downloads, navigation guards, anti-detection)
- Session profile creation with isolated partitions and cookie management
- CDP debugger lifecycle and command forwarding via WebSocket proxy
- DOM snapshot building with accessibility tree walking and ref-mapping for element targeting
- Element interaction (click, drag, hover, fill, select, scroll, keyboard input, focus)
- Viewport and device emulation (dimensions, touch, UA, client hints)
- Network interception pattern setup (via agent-browser CLI)
- Screenshot and full-page capture with viewport synchronization
- PDF export
- Screencast streaming with frame encoding and backpressure handling
- Context menu and grab-mode overlay injection
- Shortcut forwarding from guest to renderer
- Media access delegation to system (camera/microphone via macOS TCC)
- WebAuthn handler registration per-session
- Cookie import from external browsers (Chrome, Safari, etc.)
- Download request queuing with timeout enforcement and destination management
- Stale webContents detection and cleanup on renderer process swaps
- Agent-browser native binary lifecycle management and CDP session pooling

**Public API / IPC / RPC**
- BrowserManager.registerGuest()
- BrowserManager.unregisterGuest()
- BrowserManager.getGuestWebContentsId()
- BrowserManager.getWorktreeIdForTab()
- BrowserManager.setViewportOverride()
- BrowserManager.setAnnotationViewportBridge()
- BrowserManager.acquireAutomationVisibility()
- BrowserManager.ensureWebviewVisible()
- BrowserManager.openDevTools()
- BrowserManager.hasActiveGrabOp()
- BrowserManager.setGrabMode()
- BrowserManager.awaitGrabSelection()
- BrowserManager.extractHoverPayload()
- BrowserManager.captureSelectionScreenshot()
- BrowserManager.cancelGrabOp()
- BrowserManager.handleGuestWillDownload()
- BrowserManager.acceptDownload()
- BrowserManager.cancelDownload()
- BrowserManager.getDownloadPrompt()
- BrowserManager.notifyPermissionDenied()
- AgentBrowserBridge.snapshot()
- AgentBrowserBridge.click()
- AgentBrowserBridge.goto()
- AgentBrowserBridge.fill()
- AgentBrowserBridge.select()
- AgentBrowserBridge.scroll()
- AgentBrowserBridge.screenshot()
- AgentBrowserBridge.fullPageScreenshot()
- AgentBrowserBridge.tabList()
- AgentBrowserBridge.tabSwitch()
- AgentBrowserBridge.setViewport()
- AgentBrowserBridge.interceptEnable()
- CdpBridge.snapshot()
- CdpBridge.click()
- CdpBridge.goto()
- CdpBridge.fill()
- CdpBridge.select()
- CdpBridge.scroll()
- CdpBridge.screenshot()
- CdpBridge.tabList()
- CdpBridge.tabSwitch()
- BrowserSessionRegistry.createProfile()
- BrowserSessionRegistry.deleteProfile()
- BrowserSessionRegistry.getProfile()
- BrowserSessionRegistry.listProfiles()
- BrowserSessionRegistry.clearDefaultSessionCookies()
- BrowserSessionRegistry.applyPendingCookieImport()
- startBrowserScreencast()
- CdpWsProxy.start()
- CdpWsProxy.stop()

**External dependencies**
- electron (webContents, session, shell, screen, dialog, app, systemPreferences)
- node:child_process (execFile, execFileSync, ChildProcess)
- node:fs (readFileSync, writeFileSync, copyFileSync, unlinkSync, mkdirSync, renameSync, existsSync, accessSync, chmodSync)
- node:crypto (randomUUID, createDecipheriv, pbkdf2Sync)
- node:path (join)
- node:os (platform, arch, tmpdir)
- node:events (EventEmitter)
- node:sqlite (DatabaseSync)
- node:buffer (Buffer)
- node:http (createServer, Server, IncomingMessage, ServerResponse)
- ws (WebSocket, WebSocketServer)
- agent-browser (native binary via execFile, discovered at resources/agent-browser-{platform}-{arch})
- better-sqlite3 (via DatabaseSync for SQLite cookie queries)

**Persistence**
- Electron session partitions (per-profile SQLite cookie DB at Partitions/{partition}/Cookies)
- browser-session-meta.json (session profiles, user agents, pending cookie imports metadata)
- Session-scoped interception patterns (in-memory, restored from metadata on process swap)
- On-disk staging for cookie import (temp DB copy before live DB swap)

**Cross-platform concerns**
- Platform-specific agent-browser binary resolution (macOS/Linux/Windows, ARM64/x86_64)
- Chrome DevTools Protocol version detection from bundled Chromium
- macOS system media access via systemPreferences TCC integration
- Windows and Unix file permission handling for agent-browser binary chmod
- Multiplatform keyboard modifier mapping (Cmd on macOS, Ctrl on Linux/Windows)

### `main-cli-internal`

Manages cross-platform shell command registration for the Orca CLI across macOS, Linux (AppImage), Windows, and WSL, including symlink/wrapper installation, PATH configuration, and launcher lifecycle (install/remove/status).

**Rust portability:** tier=`platform` · effort=`L` · target=`orca-cli-installer (new crate); vendorable: regex for marker parsing, walkdir for asset bundling validation`  
_Core logic (state machine, path logic, script generation) is pure Rust-compatible. Platform-specific challenges: (1) macOS privilege escalation requires swift-bridge to Security.framework or osascript via Process (doable), (2) Windows registry PATH access requires winreg crate or WinAPI FFI (both available), (3) PowerShell invocation via process spawn (straightforward), (4) WSL interop via cmd/powershell process execution (straightforward), (5) filesystem APIs map 1:1 to std::fs + tokio::fs. Symlink/hardlink handling works on all three platforms in std::fs. Estimated 800-1200 lines of Rust + 200-400 lines of Swift bridge code for macOS privilege escalation. No native addon dependencies. Script templating can use simple string interpolation or a lightweight template crate (minijinja)._

**Capabilities**
- CliInstaller: platform-aware command installation (symlink macOS/Linux, wrapper Windows), launcher resolution from bundled assets or development modes, PATH manipulation (Electron environment variable handling), privilege escalation on macOS via osascript
- WslCliInstaller: manages WSL (Windows Subsystem for Linux) CLI registration by creating bash launcher scripts and PowerShell bridge stubs within WSL distros, detects Windows interop availability, manages legacy command migration
- Script builders: generates platform-specific launcher scripts (bash for Unix/WSL, Windows batch/PowerShell), AppImage wrapper construction, WSL bridge construction with safe atomic file replacement
- Status detection: inspects existing shell commands to determine installation state (installed/not_installed/stale/conflict), tracks current launcher target, validates managed vs user-owned commands, checks PATH configuration per platform
- PATH registry management: Windows user-scoped PATH reading/writing via PowerShell, Unix PATH string parsing and comparison with platform normalization, symlink/hardlink detection

**Public API / IPC / RPC**
- CliInstaller class: constructor(options?), getStatus(), install(), remove()
- WslCliInstaller class: constructor(options?), getStatus(), install(), remove()
- getBundledLauncherPath(platform, resourcesPath)
- buildAppImageCliWrapper(appImagePath)
- buildWslLauncher(windowsLauncherPath, bridgePath?)
- buildWslBridgeScript()
- buildSafeReplaceGuard(path, managedMarker)
- buildSafeRemoveCommand(commandPath)
- getBridgePathFromCommandPath(commandPath)
- parseManagedLauncherTarget(content)
- getWslLauncherMarker(), getWslBridgeMarker()
- getPosixDirname(path)
- quoteShell(value)
- IPC channels (from src/main/ipc/cli.ts): cli:getInstallStatus, cli:install, cli:remove, cli:getWslInstallStatus, cli:installWsl, cli:removeWsl

**External dependencies**
- electron (app.isPackaged, app.getPath)
- node:child_process (execFile with timeout handling)
- node:fs (sync/async filesystem: existsSync, lstat, mkdir, readFile, readlink, symlink, unlink, writeFile)
- node:os (homedir)
- node:path (path manipulation and resolution)
- node:util (promisify)
- process.env (PATH, NODE_OPTIONS, APPIMAGE, ORCA_CLI_INSTALL_PATH)
- osascript binary (macOS privilege escalation)
- powershell.exe (Windows PATH registry read/write)
- wsl.exe (WSL command execution)

**Persistence**
- No database tables. File-based: shell command files at ~/usr/local/bin (macOS), ~/.local/bin (Linux), %LOCALAPPDATA%/Programs/Orca (Windows), WSL distro ~/.local/bin; launcher assets under resources/bin (packaged) or userData/cli/bin (dev); Windows user-scoped PATH via registry (via PowerShell SetEnvironmentVariable); development launchers cached in userData/cli/bin

**Cross-platform concerns**
- macOS: /usr/local/bin vs ~/.local/bin (arm64 Homebrew moved), osascript elevation for privileged filesystem access, .app bundle launcher paths
- Linux: ~/.local/bin (XDG-standard), /usr/bin/orca conflict (GNOME Orca), AppImage FUSE mount ephemeral paths with stable outer wrapper
- Windows: %LOCALAPPDATA%/Programs/Orca, user-scoped PATH registry (no elevation), batch/PowerShell wrapper forwarding, legacy WSL support
- WSL: Windows interop detection (powershell.exe, wslpath), base64-encoded bash command forwarding via wsl.exe, bridge PowerShell script in WSL home

### `main-codex-accounts`

Manages multi-account lifecycle for Codex CLI, including authentication, account switching across host/WSL runtimes, credential isolation, and managed-home directory creation/synchronization. Integrates Codex hook installation for agent status reporting and syncs configuration between system and managed runtime homes.

**Rust portability:** tier=`mixed` · effort=`XL` · target=`orca-codex-accounts`  
_Combines pure state logic (account selection, JWT parsing, trust key/hash computation) with significant IO (fs operations, process spawning, WSL/SSH integration, symlink management), platform-specific logic (Windows ACL, WSL shell commands, version manager directories per OS), and a Codex CLI dependency. Pure components (runtime-selection, token parsing, trust logic) are straightforward. IO-heavy components (managed home creation/sync, hook installation, resource linking) require careful abstraction of fs operations and process spawning. WSL complexity is high: requires wsl.exe integration on Windows, bash command encoding, UNC path translation. SSH/SFTP support (installRemote) needs an async, non-blocking approach incompatible with Electron's main-process sync patterns. Recommend splitting into: (1) pure core (account state, selection, validation), (2) managed-home IO module with trait-based fs abstraction, (3) wsl-integration module, (4) hook-service (pure trust logic + fs-based persist), (5) runtime-home-sync (Electron-specific auth materialization). Main challenge: maintaining account-mutation serialization in async Rust without locking up the event loop. Consider redesigning account lifecycle around async channels rather than Electron-specific Promise queues._

**Capabilities**
- Multi-account login/logout via codex login subprocess
- Account switching between host and per-distro WSL runtimes
- Managed CODEX_HOME directory creation and ownership validation (with .orca-managed-home markers)
- OAuth token parsing and account identity extraction from auth.json
- Config.toml mirroring from system ~/.codex to managed runtimes
- Codex hook installation/removal with trust-key computation and TOML-based trust persistence
- Rate-limit quota refresh on account switches
- System-managed hooks deduplication and plugin-placeholder filtering
- Session bridge syncing (system sessions to managed home)
- Resource symlink/copy management (skills, plugins, themes, prompts) with fallback copy markers
- WSL-specific: path translation, bash command encoding, distro home resolution, managed account recovery
- Windows ACL repair for permission-denied writes
- Remote SSH hook installation with SFTP
- Legacy hook cleanup (pre-managed-runtime hook removal)

**Public API / IPC / RPC**
- CodexAccountService.listAccounts() -> CodexRateLimitAccountsState
- CodexAccountService.addAccount(target?: CodexAccountAddTarget) -> Promise<CodexRateLimitAccountsState>
- CodexAccountService.reauthenticateAccount(accountId: string) -> Promise<CodexRateLimitAccountsState>
- CodexAccountService.removeAccount(accountId: string) -> Promise<CodexRateLimitAccountsState>
- CodexAccountService.selectAccount(accountId: string | null) -> Promise<CodexRateLimitAccountsState>
- CodexAccountService.selectAccountForTarget(accountId: string | null, target?: CodexAccountSelectionTarget) -> Promise<CodexRateLimitAccountsState>
- CodexRuntimeHomeService.syncForCurrentSelection(target?: CodexAccountSelectionTarget) -> void
- CodexRuntimeHomeService.clearLastWrittenAuthJson(accountId: string) -> void
- CodexHookService.getStatus() -> AgentHookInstallStatus
- CodexHookService.install() -> AgentHookInstallStatus
- CodexHookService.installRemote(sftp: SFTPWrapper, remoteHome: string) -> Promise<AgentHookInstallStatus>
- CodexHookService.refreshRuntimeUserHooks() -> AgentHookInstallStatus
- CodexHookService.remove() -> AgentHookInstallStatus
- resolveCodexCommand(options?: ResolveCommandOptions) -> string
- getOrcaManagedCodexHomePath() -> string
- getSystemCodexHomePath() -> string
- syncSystemCodexResourcesIntoManagedHome() -> void
- syncSystemConfigIntoManagedCodexHome() -> void

**External dependencies**
- electron (app.getPath, ipcMain)
- node:fs (readFileSync, writeFileSync, mkdirSync, rmSync, symlinkSync, copyFileSync, etc.)
- node:path (join, resolve, dirname, relative, sep)
- node:os (homedir)
- node:crypto (randomUUID)
- node:child_process (spawn, execFileSync)
- ssh2 (SFTPWrapper type only - used in remote hook install)
- Store (Electron persistence service)
- RateLimitService (quota tracking service)
- wsl utilities (parseWslUncPath, toWindowsWslPath, getWslHome, buildEncodedWslBashCommand)
- win32-utils (getSpawnArgsForWindows, grantDirAcl, isPermissionError)

**Persistence**
- ~/.codex/auth.json (system-level Codex credentials, read and mirrored)
- ~/.codex/config.toml (system Codex config with hooks/trust entries)
- ~/.codex/hooks.json (system user hooks)
- Orca userData/codex-accounts/<accountId>/home/.orca-managed-home (managed account marker file)
- Orca userData/codex-accounts/<accountId>/home/config.toml (mirrored config per account)
- Orca userData/codex-accounts/<accountId>/home/hooks.json (runtime hooks with Orca-managed entries)
- Orca userData/codex-accounts/<accountId>/home/auth.json (per-account OAuth credentials)
- Orca userData/codex-runtime-home/home/ (shared Orca-managed Codex home for resource symlinks)
- Orca userData/codex-runtime-home/.orca-resource-copies/<entryName>.json (markers for fallback copies)
- WSL ~/.local/share/orca/codex-accounts/<accountId>/home/ (WSL-specific managed homes with Linux paths)
- GlobalSettings.codexManagedAccounts (account list with email, runtime type, distro)
- GlobalSettings.activeCodexManagedAccountId (host-only active account)
- GlobalSettings.activeCodexManagedAccountIdsByRuntime (per-runtime/per-distro selection)

**Cross-platform concerns**
- Windows: WSL distro detection/mounting via wsl.exe commands, UNC path translation, batch script detection for Codex CLI entry points, ACL-based permission repair
- macOS: userData under ~/Library/Application Support/orca, realpath symlink resolution, nvm/version-manager bin directory probing
- Linux: userData under ~/.config/orca, XDG_CONFIG_HOME support, pnpm/yarn/mise shim directories
- SSH: POSIX hook scripts (.sh) for remote hosts, SFTP for file transfer, no GUI app context
- WSL: Shell-in-shell login via wsl.exe --exec, bash -ic for initialization, .local/share isolated accounts per distro

### `main-daemon`

Headless background daemon managing terminal PTY sessions, xterm terminal state, session lifecycle (create/attach/resize/write/kill), and IPC communication with renderer. Spawns and orchestrates node-pty subprocesses, emulates ANSI/escape-sequence parsing via xterm-headless, and maintains session snapshots for reconnection/recovery.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca-daemon (new Rust binary crate in workspace, vendoring node-pty → native PTY binding + alacritty_terminal for headless emulation)`  
_Core logic (socket server, RPC routing, session management, state machines, snapshot capture) is pure-I/O Rust (tokio sockets, serde NDJSON). PTY spawning requires FFI binding or Rust rewrite (vte crate for terminal parsing, alacritty_terminal for state, libc/rustix for fork/execve). Unix domain socket protocol is portable; Windows socket paths → platform-specific (AF_UNIX on modern Windows via WSL, or named pipes fallback). macOS DNS resolver health → libc/nix system call. Shell path resolution and env var setup are pure logic. OSC-7 cwd parsing is pure string logic. Snapshot serialization (ANSI replay) is pure. Effort XL if maintaining node-pty FFI; effort L if binding alacritty_terminal or writing minimal vte + libc PTY spawner._

**Capabilities**
- Spawns and manages PTY subprocess handles with node-pty (shell session creation)
- Unix domain socket IPC server (NDJSON protocol, two-socket pattern: control + stream)
- Token-based authentication handshake on socket connection
- RPC routing: createOrAttach, write, resize, kill, signal, getCwd, getForegroundProcess, clearScrollback, listSessions, getSnapshot, ping, systemResolverHealth, shutdown
- Headless xterm-based ANSI/escape-sequence parsing and state tracking (no renderer replies)
- Terminal snapshot capture (ANSI serialization, scrollback, cwd tracking via OSC-7, terminal modes)
- Session state machine (created/spawning/running/exiting/exited)
- Shell-ready detection (SHELL_READY_MARKER with 15s timeout, startup command queueing)
- Session attachment/detachment (multiple clients per session, token-based)
- Output stream batching to renderer (8ms intervals, interactive fast-path for keystroke echo)
- PTY lifecycle management (kill with 5s timeout, force-kill, graceful shutdown with final checkpoints)
- Process cwd resolution fallback (OSC-7 or /proc/<pid>/cwd on Linux, lsof on macOS)
- Foreground process detection (live process name from node-pty)
- WSL shell integration (distro selection, shell path resolution, WSL env setup)
- Windows PowerShell version resolution (powershell.exe vs pwsh.exe)
- Windows Git Bash support
- Killed-session tombstones (1000-item cap to prevent reattach races)
- macOS system DNS resolver health reporting
- Session history persistence (checkpoint to JSON + ANSI scrollback)
- Graceful daemon shutdown with session checkpointing before kill
- Uncaught exception handling (PTY native errors suppressed, logic errors fatal)

**Public API / IPC / RPC**
- DaemonServer.start(): Promise<void>
- DaemonServer.shutdown(): Promise<void>
- RPC message type: 'createOrAttach' (payload: sessionId, cols, rows, cwd, env, command, shellOverride, etc.)
- RPC message type: 'write' (payload: sessionId, data)
- RPC message type: 'resize' (payload: sessionId, cols, rows)
- RPC message type: 'kill' (payload: sessionId)
- RPC message type: 'signal' (payload: sessionId, signal)
- RPC message type: 'listSessions' (no payload)
- RPC message type: 'getCwd' (payload: sessionId)
- RPC message type: 'getForegroundProcess' (payload: sessionId)
- RPC message type: 'clearScrollback' (payload: sessionId)
- RPC message type: 'getSnapshot' (payload: sessionId)
- RPC message type: 'ping' (no payload)
- RPC message type: 'systemResolverHealth' (no payload)
- RPC message type: 'shutdown' (payload: killSessions boolean)
- Socket event: 'data' (NDJSON stream)
- Socket event: 'exit' (payload: code)
- PROTOCOL_VERSION: 11
- NOTIFY_PREFIX: 'notify_' (fire-and-forget RPCs)
- DaemonClient.ensureConnected(): Promise<void>
- DaemonClient.request<T>(type, payload): Promise<T>
- DaemonClient.notify(type, payload): void
- DaemonClient.onEvent(listener): () => void (unsubscribe)
- DaemonClient.onDisconnected(listener): () => void (unsubscribe)
- TerminalHost.createOrAttach(opts): Promise<CreateOrAttachResult>
- TerminalHost.write(sessionId, data): void
- TerminalHost.resize(sessionId, cols, rows): void
- TerminalHost.kill(sessionId): void
- TerminalHost.signal(sessionId, sig): void
- TerminalHost.getCwd(sessionId): Promise<string | null>
- TerminalHost.getForegroundProcess(sessionId): string | null
- TerminalHost.clearScrollback(sessionId): void
- TerminalHost.getSnapshot(sessionId): TerminalSnapshot | null
- TerminalHost.listSessions(): SessionInfo[]
- Session.write(data): void
- Session.resize(cols, rows): void
- Session.kill(): void
- Session.signal(sig): void
- Session.getSnapshot(): TerminalSnapshot | null
- HeadlessEmulator.write(data): Promise<void>
- HeadlessEmulator.resize(cols, rows): void
- HeadlessEmulator.getSnapshot(opts): TerminalSnapshot

**External dependencies**
- node-pty (PTY spawning and lifecycle)
- @xterm/headless (terminal emulation, ANSI parsing, snapshot serialization)
- @xterm/addon-serialize (xterm state serialization)
- child_process.fork (daemon process launching)
- child_process.execFileSync (macOS DNS resolver health check)
- net (Unix domain socket server and client)
- crypto.randomUUID (token generation, client IDs)
- fs (socket/token file I/O, directory/checkpoint persistence)
- path (path manipulation, Windows path normalization)
- perf_hooks (interactive output latency tracking)

**Persistence**
- Unix domain socket file (socketPath parameter, deleted on shutdown)
- Token file (tokenPath parameter, PROTOCOL_VERSION-stamped UUID)
- Session history checkpoint JSON (sessionId-based directory, meta.json, checkpoint.json)
- Scrollback buffer (terminal state in checkpoints, up to 5000 lines default)
- Session metadata (cwd, cols, rows, startedAt, endedAt, exitCode)

**Cross-platform concerns**
- macOS: lsof fallback for process cwd resolution, system DNS resolver health via execFileSync
- Linux: /proc/<pid>/cwd fallback for process cwd resolution
- Windows: PowerShell version resolution (powershell.exe vs pwsh.exe), Git Bash detection, WSL distro selection, COMSPEC shell resolution, HOMEPATH+HOMEDRIVE home dir construction
- WSL: distro context tracking (getWslContextFromSessionId), CODEX_HOME cross-platform detection, env translation (addWslEnvKeys)
- Signal handling: SIGTERM/SIGINT cleanup on daemon entry, SIGHUP during subprocess exit (pty-subprocess.ts neutralizes pid reuse hazard)

### `main-git`

Centralized git repository operations via command execution. Provides git/gh/glab CLI wrappers with transparent WSL support on Windows, status polling, diff generation, worktree management, and upstream/push-target resolution. Abstracts platform differences so all consumers (IPC handlers, SSH relay, tests) get consistent behavior.

**Rust portability:** tier=`io` · effort=`L` · target=`orca-git (new crate in workspace); vendor git2-rs for repo operations, implement gh/glab via shelled-out binaries or GitHub/GitLab APIs (optional: octocat, gitlab crates)`  
_Moderate effort. Core git operations (status, diff, log, worktree) map cleanly to git2-rs or git subprocess calls. WSL path translation and gh/glab CLI wrappers are platform-specific I/O. Terminal-side gh/glab CLIs remain shelled-out for auth/token handling and GitLab API parity — no need to reimplement CLI argument parsing. Diff line-stats parsing is pure logic (already in shared/). Binary blob reads (git show) are file I/O. Worktree management is subprocess-heavy (git worktree add/remove/list/prune). Main complexity: preserve WSL distro-discovery heuristics, taskkill fallback on Windows, process tree signals, timeout semantics, and idempotent-write guards for gh retries. Reference existing git2-rs for repo queries; hand off complex workflows (rebase, merge, cherry-pick) to subprocess git. Branch cleanup shared logic already ported._

**Capabilities**
- Git command execution (async, sync, spawned) with auto-detected WSL routing
- GitHub/GitLab CLI execution with transient-error retry (5xx/429/socket-reset)
- Git status polling (staged/unstaged/untracked/conflict detection)
- Worktree listing, creation (regular/sparse), removal, and branch cleanup
- Branch comparison against base (commit count, changed files, line stats)
- Commit diff/blame history extraction
- Upstream status resolution (effective vs configured)
- Push/pull/fetch/rebase-from-base operations
- Ref search and base-ref auto-detection (origin/HEAD fallback via probes)
- Binary blob reading and diff building (text/image/PDF preview support)
- Conflict detection (merge/rebase/cherry-pick) and resolution
- File staging/unstaging and discard (tracked/untracked with safety checks)
- Commit creation with message
- Branch rename with collision detection and upstream guards
- Remote URL parsing and hosted-file-link building (GitHub/GitLab/Bitbucket)
- Git username resolution (github.user config, gh auth, computed from remote)
- Sparse checkout detection
- Gitignore pattern checking
- Clone-path validation and conflict detection

**Public API / IPC / RPC**
- gitExecFileAsync(args, {cwd, encoding?, maxBuffer?, timeout?, env?})
- gitExecFileSync(args, {cwd, encoding?, stdio?})
- gitExecFileAsyncBuffer(args, {cwd, maxBuffer?})
- gitSpawn(args, {cwd, ...spawnOptions})
- ghExecFileAsync(args, {cwd?, wslDistro?, idempotent?, ...options})
- glabExecFileAsync(args, {cwd?, wslDistro?, idempotent?, ...options})
- commandExecFileAsync(command, args, {cwd?, encoding?, maxBuffer?, timeout?, env?, signal?})
- wslAwareSpawn(command, args, {cwd?, ...spawnOptions})
- gitOptionalLocksDisabledEnv(env?)
- getStatus(worktreePath, {includeIgnored?})
- getDiff(worktreePath, filePath, staged, compareAgainstHead?)
- getBranchCompare(worktreePath, baseRef)
- getBranchDiff(worktreePath, {headOid, mergeBase, filePath, oldPath?})
- getCommitCompare(worktreePath, commitId)
- getCommitDiff(worktreePath, {commitOid, parentOid?, filePath, oldPath?})
- getHistory(worktreePath, options?)
- listWorktrees(repoPath)
- addWorktree(repoPath, branch, baseRef, options?)
- addSparseWorktree(repoPath, branch, baseRef, sparsePaths)
- removeWorktree(repoPath, worktreePath, options?)
- assertWorktreeCleanForRemoval(repoPath, worktreePath)
- forceDeleteLocalBranch(repoPath, branch, options?)
- stageFile(worktreePath, filePath)
- unstageFile(worktreePath, filePath)
- bulkStageFiles(worktreePath, filePaths)
- bulkUnstageFiles(worktreePath, filePaths)
- discardChanges(worktreePath, filePath)
- bulkDiscardChanges(worktreePath, filePaths)
- commitChanges(worktreePath, message)
- abortMerge(worktreePath)
- abortRebase(worktreePath)
- detectConflictOperation(worktreePath)
- resolveGitDir(worktreePath)
- getStagedCommitContext(worktreePath)
- getUpstreamStatus(worktreePath, pushTarget?)
- gitPush(worktreePath, publish?, pushTarget?, {forceWithLease?})
- gitPull(worktreePath, pushTarget?)
- gitFastForward(worktreePath, pushTarget?)
- gitFetch(worktreePath, pushTarget?)
- gitPullRebaseFromBase(worktreePath, baseRef)
- getDefaultBaseRef(path)
- getBaseRefDefault(path)
- getDefaultRemote(path)
- searchBaseRefs(path, query, limit?)
- searchBaseRefDetails(path, query, limit?)
- getRemoteDrift(repoPath, localRef, remoteRef)
- getRecentDriftSubjects(repoPath, localRef, remoteRef, limit)
- getRemoteCount(path)
- getBranchConflictKind(path, branchName, allowedBaseRef?)
- getRemoteUrl(path)
- getRemoteFileUrl(repoPath, relativePath, line)
- getRepoName(path)
- getGitUsername(path)
- isGitRepo(path)
- checkIgnoredPaths(worktreePath, relativePaths)
- parseWorktreeList(output, {nulDelimited?})
- validateGitPushTarget(worktreePath, pushTarget)
- branchHasUpstream(exec)
- resolveUniqueBranchName(exec, leaf, compute, currentBranch, maxAttempts?)
- renameCurrentBranch(exec, newBranch)
- claimCloneTarget(clonePath)
- cleanupClaimedCloneTarget(clonePath, success)
- deriveValidatedClonePath({url, destination})
- extractExecError(err)
- isTransientGhError(stderr)
- parseRetryAfterMs(stderr)
- isMissingRemoteRefGitError(error)
- hasWorktreeBaseCommitRef(worktreePath, branch)
- resolveDefaultBaseRefViaExec(exec)
- buildSearchBaseRefsArgv(normalizedQuery, limit, {excludeRemoteHead?})
- parseAndFilterSearchRefs(stdout, limit)
- parseAndFilterSearchRefDetails(stdout, limit, remotes?)
- normalizeRefSearchQuery(query)
- parseHostedRemote(remoteUrl)
- buildHostedRemoteFileUrl(remoteUrl, relativePath, branch, line)
- normalizeGitUsername(value)
- getSshGitUsername(path)
- clearEffectiveUpstreamStatusCacheForTests()
- isWithinWorktree(pathApi, resolvedWorktree, resolvedTarget)

**External dependencies**
- child_process (execFile, execFileSync, spawn)
- fs (existsSync, statSync, readFile, writeFile, stat, rm)
- path (join, resolve, relative, basename, extname, posix, win32)
- crypto (randomUUID)
- ../wsl module (parseWslPath, toWindowsWslPath, toLinuxPath, getDefaultWslDistro)
- ../win32-utils (getSpawnArgsForWindows, isWindowsBatchScript, resolveWindowsCommand)
- ../observability/instrumentation (withGitSpan)
- git binary (system PATH)
- gh binary (GitHub CLI, system PATH, fallback to WSL on Windows)
- glab binary (GitLab CLI, system PATH, fallback to WSL on Windows)

**Persistence**
- Git config via 'git config': branch.<name>.remote, branch.<name>.merge, github.user, user.username, branch.<name>.base (worktree creation lineage)
- Git refs: HEAD, symbolic-refs, .git/MERGE_HEAD, .git/CHERRY_PICK_HEAD, .git/rebase-merge/, .git/rebase-apply/ (conflict operation detection)
- .git/sparse-checkout file (sparse checkout status)
- .git/worktree directory (worktree metadata)
- File-based clone-path tokens in temp directory (prevent collisions during concurrent clone operations)

**Cross-platform concerns**
- Windows: WSL path translation (UNC \\wsl.localhost\... to /mnt/c/... and native Linux paths), taskkill for process tree cleanup on Windows
- Windows: .cmd shim retry fallback (batch script detection and re-exec)
- Windows: shell selection (cmd.exe vs /bin/bash) for gh auth status parsing
- macOS/Linux: native git/gh/glab binary execution
- All: CRLF handling in git output parsing (split by /\r?\n/ regex)
- All: Path separator normalization (posix vs win32 path APIs for worktree equality checks)
- Windows: Special handling of git show output translation from WSL Linux paths to Windows UNC
- SSH: WSL distro fallback for global gh calls (rate_limit, auth) when gh.exe missing on WSL-only Windows

### `main-git-providers-small`

Integrations for smaller issue tracking and SCM platforms (Jira, Azure DevOps, Gitea, Bitbucket) that provide REST API clients for pull request metadata, issue operations, and repository resolution, enabling cross-platform CI/CD and review workflow unification.

**Rust portability:** tier=`io` · effort=`M` · target=`orca-providers (new; contains provider clients and repo parsers) + reqwest (HTTP, timeout, auth headers) + serde_json (JSON parsing) + regex (URL parsing/validation)`  
_REST API clients are pure HTTP logic with JSON mapping. Medium effort: (1) replicate repo URL parsers for each platform (regex/URL parsing, mostly algorithmic), (2) translate Electron safeStorage to platform keyring (use keyring crate on macOS, secret-service on Linux; Windows handled by std env vars or os-level APIs), (3) HTTP request abstraction with retry/timeout/auth headers (reqwest handles this well), (4) concurrency queue for Jira (tokio channels). No platform-specific UI. File IO for caching repo refs and tokens is standard fs. Azure DevOps and Gitea parsers are complex due to multi-host and subpath support but fully algorithmic. Bitbucket is simplest (always api.bitbucket.org). Jira token lifecycle requires careful secure handling across platforms but is decoupled from the rest. Risk: keyring platform variation, but fallback to plaintext in tests is acceptable._

**Capabilities**
- Fetch pull requests by number from remote APIs
- Fetch pull requests for a branch (with fallback to linked PR number)
- Derive PR state (open/closed/merged/draft) and build status from provider-specific schemas
- Parse repository references from git remote URLs (HTTP/HTTPS/SSH)
- Maintain LRU repo reference cache (512 entries max per provider)
- Validate provider authentication status via API connectivity tests
- For Jira: list issues by filter (assigned/reported/done), search issues via JQL, fetch individual issues, create new issues with custom fields, update issue status and fields, add/read comments, list projects/issue types/transitions/priorities/assignable users, manage multi-site connections with encryption
- Resolve merge state (MERGEABLE/CONFLICTING/UNKNOWN)
- Normalize API base URLs per provider
- Handle concurrent request limiting (Jira: max 4 concurrent requests with queue)
- Read and encrypt provider tokens (Jira uses Electron safeStorage for encryption at rest)

**Public API / IPC / RPC**
- azure-devops: getAzureDevOpsAuthStatus(), getAzureDevOpsPullRequest(repoPath, prNumber, connectionId?), getAzureDevOpsPullRequestForBranch(repoPath, branch, linkedPRNumber?, connectionId?), getAzureDevOpsRepoSlug(repoPath, connectionId?), normalizeAzureDevOpsApiBaseUrl(value)
- gitea: getGiteaAuthStatus(), getGiteaPullRequest(repoPath, prNumber, connectionId?), getGiteaPullRequestForBranch(repoPath, branch, linkedPRNumber?, connectionId?), getGiteaRepoSlug(repoPath, connectionId?), normalizeGiteaApiBaseUrl(value)
- bitbucket: getBitbucketAuthStatus(), getBitbucketPullRequest(repoPath, prNumber, connectionId?), getBitbucketPullRequestForBranch(repoPath, branch, linkedPRNumber?, connectionId?), getBitbucketRepoSlug(repoPath, connectionId?)
- jira: getStatus(), connect(args), disconnect(siteId?), selectSite(siteId), testConnection(siteId?), clearToken(siteId), listIssues(filter?, limit?, siteId?), searchIssues(jql, limit?, siteId?), getIssue(key, siteId?), createIssue(args), updateIssue(args), addIssueComment(args), getIssueComments(key, siteId?), listProjects(siteId?), listIssueTypes(projectId, siteId?), listCreateFields(projectId, issueTypeId?, siteId?), listPriorities(siteId?), listAssignableUsers(projectKey, siteId?), listTransitions(issueKey, siteId?)

**External dependencies**
- node:crypto (hash generation)
- node:fs (token persistence)
- node:path (file paths)
- node:os (homedir)
- electron.safeStorage (encrypted token storage)
- fetch (HTTP requests with AbortController timeout)
- git child process execution (via gitExecFileAsync from ../git/runner)
- SSH git provider (via getSshGitProvider from ../providers/ssh-git-dispatch)

**Persistence**
- ~/.orca/jira-sites.json (JSON config: active site, selected site, site list)
- ~/.orca/jira-tokens/ (encrypted API tokens per site)
- In-memory LRU cache: azure-devops repo refs (512 max), gitea repo refs (512 max), bitbucket repo refs (512 max)
- In-memory token cache for Jira (accessed multiple times per request)

**Cross-platform concerns**
- Token encryption via Electron safeStorage (platform-specific secure storage)
- SSH support for git operations (connectionId parameter enables SSH tunneling to remote repos)
- Home directory resolution via os.homedir() (macOS/Linux/Windows)
- Timeout handling for network requests (5s default, configurable)

### `main-github subsystem`

GitHub API client and work-item management for Orca. Orchestrates all GitHub interactions including PR/issue listing, detail fetching, check running, mergeability detection, conflict analysis, code review operations, and GitHub Projects V2 integration. Implements rate-limit awareness, error classification, and caching strategies across multiple API endpoints.

**Rust portability:** tier=`io` · effort=`L` · target=`orca_github_client (new workspace crate) + external deps: reqwest/hyper (HTTP), serde_json (JSON parsing), tokio (async runtime), structopt/clap (CLI arg parsing for parity with gh shelling). Vendorable: gh CLI remains external binary (desktop ships it). Alternatively, octocat-rs (partial GitHub API wrapper) + graphql-client for stronger types, but integration surface (error classification, rate-limit envelopes, project-view slug discovery) requires careful mapping.`  
_Large rewrite: 3600+ lines of sophisticated error classification, caching, rate-limit coordination, GraphQL introspection, and WorkItem/ProjectV2 model mapping. Dependencies: requires spawning git and gh binaries (or reimplementing GitHub API client from scratch ~2000 lines for REST+GraphQL coverage). Rate-limit awareness and PR refresh coordinator are novel async state machines. Main challenge is preserving: (1) error classification semantics (regexes on gh stderr must become HTTP status + response code interpretation), (2) concurrency limiter (4 max parallel gh processes — becomes connection pool bounded by semaphore), (3) caching TTLs and LRU eviction, (4) ProjectV2 GraphQL introspection discovery and slug validation, (5) cross-repository PR fork workflows. Recommend: split into two phases: Phase 1 = REST API client + work-item listing (core 800 lines), Phase 2 = GraphQL (Projects, auth diagnostics, rate-limit probing). The gh binary dependency can remain external (FFI call) initially, migrating to native client in a third phase if needed._

**Capabilities**
- List and search issues/PRs with pagination and filtering (state, draft, labels, assignee, author, review-requested, reviewed-by)
- Fetch individual PR/issue details including body, comments, participants, assignees, check status, merge state
- Manage PR state transitions (open/draft/closed) and auto-merge configuration
- Request and remove PR reviewers with user filtering
- Merge PRs with configurable merge method (squash/rebase/merge) and queue detection
- Retrieve and parse PR checks (status, conclusion, log tails) with caching
- Merge-conflict detection via local git merge-tree analysis
- PR review comment operations (add, resolve threads, reply)
- File-level review state tracking (viewed/unviewed)
- GitHub Projects V2: list accessible projects, fetch table views, mutate field values, manage issue/PR/comment state within projects
- Project discovery pagination (org walk, nested project listing)
- Repository metadata: upstream detection, fork resolution, remote URL management
- PR code review comment line mapping from unified diffs
- Rate-limit probing and budget enforcement (core/graphql/search buckets)
- GitHub authentication diagnostics (scope validation, env vs keyring detection)
- PR refresh coordination (priority queue, background refresh pacing, visibility-based throttling)
- Hosted review/branch creation via gh CLI
- Issue source preference resolution (upstream/origin fallback)
- Cross-repository PR support (fork workflow detection, push target discovery)

**Public API / IPC / RPC**
- checkOrcaStarred() -> Promise<boolean | null>
- starOrca() -> Promise<boolean>
- getAuthenticatedViewer() -> Promise<GitHubViewer | null>
- listWorkItems(repoPath, limit?, query?, before?, preference?, connectionId?, noCache?) -> Promise<ListWorkItemsResult>
- countWorkItems(repoPath, query?, preference?, connectionId?) -> Promise<number>
- getWorkItem(repoPath, number, type, connectionId?) -> Promise<GitHubWorkItem | null>
- getWorkItemByOwnerRepo(ownerRepo, number, type) -> Promise<GitHubWorkItem | null>
- getPRForBranch(repoPath, branchName, connectionId?) -> Promise<PRInfo | null>
- getPRForBranchOutcome(repoPath, branchName, connectionId?) -> Promise<PRRefreshOutcome>
- getPullRequestPushTarget(repoPath, prNumber, connectionId?) -> Promise<GitPushTarget | null>
- getRepoSlug(repoPath, connectionId?) -> Promise<{owner, repo} | null>
- getRepoUpstream(repoPath, connectionId?) -> Promise<OwnerRepo | null>
- createGitHubPullRequest(args) -> Promise<CreateHostedReviewResult>
- getPRChecks(repoPath, number, connectionId?) -> Promise<PRCheckRunDetails>
- getPRCheckDetails(repoPath, number, checkRunId, connectionId?) -> Promise<PRCheckDetail | null>
- rerunPRChecks(repoPath, number, checkRunId?, connectionId?) -> Promise<GitHubRerunPRChecksResult>
- getPRComments(repoPath, number, connectionId?) -> Promise<PRComment[] | null>
- addPRReviewComment(args) -> Promise<GitHubCommentResult>
- addPRReviewCommentReply(args) -> Promise<GitHubCommentResult>
- resolveReviewThread(args) -> Promise<GitHubCommentResult>
- setPRFileViewed(args) -> Promise<GitHubPRFileViewedState>
- mergePR(repoPath, number, method, connectionId?) -> Promise<GitHubCommentResult>
- setPRAutoMerge(args) -> Promise<GitHubCommentResult>
- updatePRState(repoPath, number, state, connectionId?) -> Promise<boolean>
- updatePRTitle(repoPath, number, title, connectionId?) -> Promise<boolean>
- updatePRDetails(repoPath, number, args, connectionId?) -> Promise<boolean>
- requestPRReviewers(args) -> Promise<GitHubCommentResult>
- removePRReviewers(args) -> Promise<GitHubCommentResult>
- getIssue(repoPath, number, connectionId?) -> Promise<IssueInfo | null>
- listIssues(repoPath, limit?, preference?, connectionId?) -> Promise<IssueListResult>
- createIssue(args) -> Promise<CreateHostedReviewResult>
- updateIssue(args) -> Promise<GitHubCommentResult>
- addIssueComment(args) -> Promise<GitHubCommentResult>
- listLabels(args) -> Promise<GitHubProjectLabel[] | null>
- listAssignableUsers(args) -> Promise<GitHubAssignableUser[] | null>
- listAccessibleProjects(repoPath?, connectionId?) -> Promise<ListAccessibleProjectsResult>
- getProjectViewTable(args) -> Promise<GetProjectViewTableResult>
- listProjectViews(args) -> Promise<ListProjectViewsResult>
- resolveProjectRef(args) -> Promise<ResolveProjectRefResult>
- updateProjectItemFieldValue(args) -> Promise<GitHubProjectMutationResult>
- clearProjectItemFieldValue(args) -> Promise<GitHubProjectMutationResult>
- updateIssueBySlug(args) -> Promise<GitHubProjectMutationResult>
- updatePullRequestBySlug(args) -> Promise<GitHubProjectMutationResult>
- addIssueCommentBySlug(args) -> Promise<GitHubProjectCommentMutationResult>
- updateIssueCommentBySlug(args) -> Promise<GitHubProjectCommentMutationResult>
- deleteIssueCommentBySlug(args) -> Promise<GitHubProjectCommentMutationResult>
- listLabelsBySlug(args) -> Promise<ListLabelsBySlugResult>
- listAssignableUsersBySlug(args) -> Promise<ListAssignableUsersBySlugResult>
- listIssueTypesBySlug(args) -> Promise<ListIssueTypesBySlugResult>
- updateIssueTypeBySlug(args) -> Promise<GitHubProjectMutationResult>
- getWorkItemDetailsBySlug(args) -> Promise<ProjectWorkItemDetailsBySlugResult>
- getRateLimit(options?) -> Promise<GetRateLimitResult>
- diagnoseGhAuth() -> Promise<GhAuthDiagnostic>
- setPRRefreshOutcomeObserver(observer) -> void
- clearVisiblePRRefreshWindow(windowId) -> void
- enqueuePRRefreshByBranch(args) -> void
- enqueuePRRefreshByNumber(args) -> void

**External dependencies**
- npm: child_process (execFile, spawn via gh CLI)
- npm: util (promisify)
- npm: fs/promises (mkdtemp, readFile, rm, writeFile for temp file handling)
- npm: path (join)
- npm: os (tmpdir)
- npm: electron (webContents for IPC broadcasting)
- external binary: gh (GitHub CLI) — executes all GitHub API calls via ghExecFileAsync
- external binary: git — executes fetch, merge-base, merge-tree, rev-parse, rev-list
- network service: GitHub REST API v3 (via gh api ...)
- network service: GitHub GraphQL API v4 (via gh api graphql ...)
- network service: GitHub CLI auth (gh auth status, gh auth login, gh auth refresh)

**Persistence**
- in-memory cache: ownerRepoCache (30s TTL, 512 entries max) — caches {owner, repo} slug lookups from git remotes
- in-memory cache: repositoryMergeMetadataCache (10min TTL, 256 entries max) — caches merge queue requirement and merge method settings per repository
- in-memory cache: prCheckLogTailCache (unbounded, 128 entries max) — caches PR check log tails (up to 16KB each)
- in-memory cache: rateLimitSnapshot (30s TTL) — caches GitHub rate-limit probe (core/search/graphql buckets)
- in-memory cache: projectViewOwnerCache — caches GitHub owner metadata from project queries
- in-memory cache: projectViewFieldsCache — caches GitHub project field definitions per project
- in-memory queue: PRRefreshCoordinator queue — stores pending PR refresh requests with priority, backoff, and visibility tracking
- electron process environment: GITHUB_TOKEN, GH_TOKEN — read for env-shadowed credential detection

**Cross-platform concerns**
- SSH/WSL support: all gh calls routed through ghExecFileAsync (runner) which handles WSL path translation and SSH execution via connectionId
- SSH repo support: explicit --repo and API target arguments bypass cwd requirement for SSH paths
- macOS/Windows/Linux: consistent gh CLI PATH lookup and error message parsing
- Network connectivity: offline graceful degradation (fetch failures return null, not throw)
- Git version compatibility: fallback from modern git merge-tree (--merge-base) to legacy form on older git
- GitHub Enterprise: host-aware gh auth status parsing, multi-host account detection

### `main-ipc`

IPC handler surface bridging Electron main process and React renderer. Centralizes 500+ async/sync channel handlers for terminal PTY management, filesystem operations, git/repo control, SSH connections, notifications, settings, accounts, and integration with external services (GitHub, GitLab, Linear, Jira).

**Rust portability:** tier=`mixed` · effort=`XL` · target=`orca-ipc (new Rust library crate) + platform-specific pty/ssh bindings + tauri IPC bridge`  
_LARGE subsystem spanning 148 files, 506 IPC handlers. Core logic is pure business logic (git operations, account state, repo/worktree management) that ports cleanly to Rust. PTY spawning requires node-pty replacement or binding to native pseudo-terminal APIs. SSH depends on current relay architecture (multiplexer, credential handling). Electron ipcMain must become tauri invoke + commands. Filesystem watching via @parcel/watcher needs cross-platform reimpl or binding (likely `notify-rust` crate for notifications, `ssh2` Rust crate for SSH, `pty` Rust crate for PTY). Browser embedding (Electron BrowserWindow guests) has no direct Tauri equivalent — would require web-view or headless browser integration. Filesystem auth layer (registerWorktreeRootsForRepo, authorizeExternalPath) is simple validation logic porting easily. Notification dispatching ports to OS notifications; sound loading (custom audio files) ports to system sound APIs. Account OAuth flows (GitHub, Linear, Jira) currently use Electron's net module for HTTP; ports to reqwest/tokio. Mobile device/runtime environment RPC proxying is pure socket/message routing. Effort scaled up due to: (1) comprehensive refactor of ipcMain.handle pattern to Tauri command/listen, (2) PTY lifecycle management complexity, (3) SSH relay integration, (4) filesystem watcher cross-platform concerns._

**Capabilities**
- Terminal PTY lifecycle (spawn, resize, kill, signal) with local and SSH provider routing
- Filesystem watching with git-based fallback and WSL support
- Git operations (status, commit, push, pull, branch compare, worktree creation)
- SSH target management and port forwarding with relay support
- Repository cloning and worktree creation/removal
- Notifications with custom sounds and permission gating
- Account management (Claude, Codex, GitHub, GitLab, Linear, Jira)
- Settings persistence and retrieval
- Crash reporting and telemetry
- Browser tab embedding and download management
- Speech recognition model download and dictation
- CLI installation (local and WSL)
- Developer permissions and computer-use controls
- Agent hook installation status and management
- Runtime environment pairing and remote RPC proxying
- Mobile device access grants

**Public API / IPC / RPC**
- app:getFeatureWallAssetBaseUrl
- app:getIdentity
- app:getKeyboardInputSourceId
- app:relaunch
- app:setUnreadDockBadgeCount
- pty:spawn
- pty:write
- pty:resize
- pty:kill
- pty:signal
- fs:authorizeExternalPath
- fs:stat
- fs:listFiles
- fs:watch
- gh:issue
- gh:listIssues
- gh:updateIssueBySlug
- ssh:connect
- ssh:disconnect
- ssh:addTarget
- ssh:removeTarget
- repos:list
- repos:clone
- repos:remove
- worktrees:list
- worktrees:create
- worktrees:remove
- notifications:dispatch
- notifications:requestPermission
- settings:get
- settings:set
- claudeAccounts:list
- claudeAccounts:add
- codexAccounts:list
- telemetry:track
- browser:registerGuest
- speech:downloadModel
- runtime:getStatus
- runtimeEnvironments:call
- runtimeEnvironments:subscribe

**External dependencies**
- electron (ipcMain, BrowserWindow, app, dialog, shell, clipboard, systemPreferences, powerMonitor, net)
- node:fs/promises, node:fs (file I/O)
- node:child_process (execFile, spawn for git, CLI, defaults)
- node:crypto (randomUUID)
- node:path, node:os
- node:stream/promises, node:util
- node:dgram (UDP for network detection)
- @parcel/watcher (filesystem watching fallback)
- zod (runtime type validation)
- qrcode (QR code generation for pairing)
- @electron-toolkit/utils

**Persistence**
- Store (Electron userData persisted state)
- SshConnectionStore (SSH target definitions)
- ClaudeUsageStore, CodexUsageStore, OpenCodeUsageStore
- CrashReportStore
- RateLimitService
- KeybindingService
- terminal-history (worktree history deletion)
- floating-workspace-directory grants

**Cross-platform concerns**
- WSL detection and distro listing (Windows)
- PowerShell/Git Bash availability checks (Windows)
- Keyboard input source probe via /usr/bin/defaults (macOS only, returns null on non-Darwin)
- macOS notification center integration with app bundle ID
- Path normalization for Windows UNC/WSL conversions
- Separate PTY providers: LocalPtyProvider vs SshPtyProvider routing
- SSH relay grace period for disconnect/reconnect on all platforms
- Terminal-specific shell path handling (wsl.exe detection)
- Codex home environment isolation for WSL vs host

### `main-misc-infra`

Heterogeneous infrastructure supporting Orca's main process: PTY environment setup and terminal attribution (git/gh wrapper injection), crash reporting with deduplication and breadcrumbs, memory profiling across Electron and host processes, network proxy configuration and diagnostics, and utilities for Ghostty config import, shell environment parsing, PDF export, and SQLite database access.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca-infra (hypothetical new crate within workspace); vendored: better-sqlite3 or rusqlite for sqlite; native: libc for ps/wmic spawning, system DNS APIs; ui-native: htmlToPdf remaps to native PDF generation (Apple PDFKit or Windows Print API)`  
_Primarily io-bound (fs, process spawning, network). Pure functions like gitconfig parsing, process deduplication, ghostty config parsing are straightforward ports. SQLite wrapping trivial with rusqlite. Process enumeration needs libc::getpwuid for Unix, WinAPI for Windows. Proxy/DNS diagnostics are platform-specific subprocess invocations — scutil stays as CFNetwork or system_process wrapper. HTML-to-PDF requires native windowing (SwiftUI on macOS). PTY registry and crash breadcrumbs are simple in-memory maps/vecs. Shell startup env parsing is pure text manipulation. Attribution shim generation is pure string output (no fs dependency on read side after boot). Overlay mirroring (symlink safety) maps to fs::symlink/hard_link with lstat guards. Memory snapshot collection hits the sweet spot: low-level process enumeration is essential but manageable in Rust; ring buffers and coalescing are trivial. Tailscale MagicDNS caching is straightforward. Risk: macOS Ghostty config precedence and zsh ZDOTDIR expansion must be validated against real configs; Windows path merging complexity (PATHEXT, legacy PATH, AppData env var fallbacks) needs careful testing._

**Capabilities**
- Terminal attribution via shell wrappers for git/gh commands (POSIX and Windows native shells with PATH prepending)
- Ghostty terminal emulator config discovery, parsing, and migration/preview to Orca settings
- Crash reporting with in-memory deduplication, breadcrumb tracking, and persistent JSON store (max 5 reports)
- Process-gone classification (recoverable vs non-recoverable Chromium child-process churn)
- Memory profiling: per-PTY tracking via registry; host-wide process tree enumeration (ps on Unix, wmic on Windows); per-worktree history sparkline buffer
- Network proxy auto-configuration from environment or settings with Electron session integration and probing
- macOS system DNS resolver health diagnostic (scutil --dns parsing)
- macOS Tailscale MagicDNS detection and user hints for network issues
- PTY environment variable sourcing from shell startup files (bash login, zsh multi-file with ZDOTDIR support)
- Shell overlay directory mirroring (Pi/OpenCode symlink/junction safety without descending into links)
- OMP (oh-my-posh) command interception to inject per-PTY status extensions
- Windows PATH and environment variable management across WSL and native contexts
- Sync SQLite database adapter wrapping Node's built-in DatabaseSync
- HTML to PDF export with image-load gating via Electron BrowserWindow
- Warm-boot PTY registry hydration from live daemon to attribute orphaned sessions

**Public API / IPC / RPC**
- applyTerminalAttributionEnv(baseEnv, options)
- resolveAttributionShellFamily(options)
- previewGhosttyImport(store): Promise<GhosttyImportPreview>
- parseGhosttyConfig(content)
- mapGhosttyToOrca(parsed, isMacOS)
- findGhosttyConfigPaths(): Promise<string[]>
- findGhosttyConfigPath(): Promise<string | null>
- CrashReportStore.fromUserData(userDataPath)
- CrashReportStore.record(input), getLatestPending(), listRecent(), markSent(id), dismiss(id)
- recordCrashBreadcrumb(name, data)
- recordCoalescedCrashBreadcrumb({name, data, coalesceKey, minIntervalMs})
- getCrashBreadcrumbSnapshot()
- ProcessGoneDedupe(options).shouldRecord(key)
- getProcessGoneDedupeKey(processType, reason, exitCode)
- shouldRecordProcessGoneCrash({source, processType, serviceName, reason, exitCode, expectedTeardown})
- shouldRecoverRendererAfterProcessGone({reason, expectedTeardown})
- collectProcessGoneMetricDetails(metrics)
- buildProcessGoneCrashDetails(details)
- collectMemorySnapshot(store): Promise<MemorySnapshot>
- parsePsOutput(stdout), parseWmicOutput(stdout), collectSubtree(index, root)
- registerPty(entry), unregisterPty(ptyId), listRegisteredPtys()
- hydrateLocalPtyRegistryAtBoot(store): Promise<void>
- applyElectronProxySettings(settings, options): Promise<ProxyApplyResult>
- ensureElectronProxyFromEnvironment(options): Promise<ProxyApplyResult>
- classifyMacSystemResolverHealth(scutilOutput)
- readCurrentProcessMacSystemResolverHealth(): Promise<SystemResolverHealth>
- parseMacTailscaleDnsDiagnostic(scutilOutput)
- withMacTailscaleDnsHint(message, detail?)
- readShellStartupEnvVar(name, options)
- mirrorEntry(sourcePath, targetPath)
- safeRemoveTree(path)
- safeRemoveOverlay(overlayDir, overlayRoot)
- getPosixOmpShellWrapper(), getPowerShellOmpShellWrapper()
- removeInheritedNoColor(env)
- addOrcaWslInteropEnv(env)
- mergePersistedWindowsPath(baseEnv, options)
- readPersistedWindowsPathSegments(options)
- htmlToPdf(html): Promise<Buffer>
- SyncDatabase.prepare(sql), exec(sql), pragma(sql, options), close()

**External dependencies**
- electron (app, session, BrowserWindow)
- node-pty (as 'import * as pty')
- node:sqlite (DatabaseSync)
- node:child_process (spawn, spawnSync, execFileSync, exec/promisify)
- node:fs/promises, node:fs, fs/promises
- node:path, path, node:os, os
- node:crypto (randomUUID, crypto)
- system binaries: /usr/sbin/scutil (macOS DNS resolver), ps (Unix process enumeration), wmic (Windows process enumeration)
- @parcel/watcher or similar watching (inferred from overlay-mirror context)

**Persistence**
- JSON file: ${userDataPath}/crash-reports.json (CrashReportStore, max 5 reports with atomic write via temp + rename)
- JSON file: ${userDataPath}/orca-terminal-attribution/{posix,win32}/{git,gh,git.cmd,gh.cmd,*.ps1} (shell wrapper shims with version tracking)
- Memory buffer: pty-registry Map<string, PtyRegistration> (process-wide in-memory table, unregister on PTY teardown)
- Memory buffer: history ring buffers in collector (one per worktree, capacity 60 samples, 10min staleness cutoff)
- Memory buffer: process-gone dedupe window (recent 2sec-window events, max 128 keys)
- Memory buffer: crash breadcrumbs array (max 30 entries, appended at crash time)
- Memory cache: macOS Tailscale DNS diagnostic (5min TTL, expires and refreshes on scutil execution)
- In-flight promise coalescing: memory snapshot collector (prevents concurrent ps/wmic sweeps)

**Cross-platform concerns**
- macOS-specific: Ghostty XDG config discovery with native ~/Library/Application Support fallback, scutil DNS queries, system resolver health checks
- Linux-specific: Ghostty XDG config discovery only
- Windows-specific: native shell vs WSL shell detection, directory junctions (symlink safety in overlay-mirror), WMIC process enumeration, PowerShell wrapper generation, PATHEXT and AppData resolution
- WSL-specific: shell family classification, Codex home path detection, wrapper environment interop (PI_CODING_AGENT_DIR vs ORCA_OMP_CODING_AGENT_DIR)
- Unix/POSIX-specific: ps process enumeration with locale-agnostic decimal handling, shell startup file sourcing (bash .bash_profile, zsh .zshenv/.zprofile/.zshrc/.zlogin with ZDOTDIR)
- All platforms: Electron proxy session configuration, git/gh shell wrapper injection, OMP extension path injection

### `main-platform-misc`

Collection of auxiliary backend services for Orca IDE: discovers installed skills (from home, project repos, and plugin caches), manages keyboard bindings (persistence, normalization, conflict resolution), facilitates nested repository discovery and project group imports (gitignore-aware scanning with depth/timeout bounds), and manages star-Orca GitHub nag prompts (threshold-based, agent count tracking).

**Rust portability:** tier=`io` · effort=`M` · target=`orca_misc (filesystem scanning, path resolution, keybinding serialization) + orca_github for star-nag integration`  
_Filesystem scanning and path logic are pure Rust-friendly. Keybinding file I/O is straightforward JSON serialization. Star-nag state management is simple (counters + booleans). Main coupling is to Electron IPC (broadcast, menu rebuild) and GitHub client (checkOrcaStarred). Rust port should expose domain functions (discoverSkills, scanNestedRepos, etc.) that the SwiftUI/Cocoa shell calls via RPC or JSON bridge. Electron ipcMain/send mechanics map to native event dispatch or Swift notifications. Store + stats collection integrate with native persistence layer (defaults/user-defaults on macOS)._

**Capabilities**
- Discover SKILL.md files in ~/.codex, ~/.agents, ~/.claude, and repo .agents/.claude/.codex subdirectories (symlink-aware, respects 4/9 depth limits per source kind)
- Summarize skill metadata (name, description, file count, modification time) by parsing SKILL.md frontmatter
- Read and write platform-specific keyboard shortcut overrides to ~/.orca/keybindings.json (darwin/linux/win32 sections)
- Detect and reject keybinding conflicts via shared conflict resolver
- Migrate legacy keybindings from settings to dedicated keybindings.json file
- Scan folders recursively for nested git repositories (respects .gitignore, skips node_modules/dist/build, detects bare repos via HEAD/objects/refs)
- Resolve and canonicalize repo paths, deduplicate scanned repos
- Create project group hierarchy and import nested repos under groups or as flat selection
- Track agent spawn count across app versions and emit threshold-based GitHub star notifications (doubling backoff: 50→100→200)
- Broadcast keybinding changes to all windows and rebuild app menu on modification

**Public API / IPC / RPC**
- discoverSkills(args): Promise<SkillDiscoveryResult> — find all SKILL.md files
- buildSkillDiscoverySources(args): SkillScanRoot[] — construct scan roots for home/repos
- scanNestedRepos(args): Promise<NestedRepoScanResult> — discover git repos in folder
- createNestedProjectGroupResolver(args): NestedProjectGroupResolver — create import resolver
- resolveNestedRepoSelection(args): ResolvedNestedRepoSelection — filter scanned repos
- resolveNestedRepoImportPaths(args): ResolvedNestedRepoSelection — validate import paths
- KeybindingService.getSnapshot(): KeybindingFileSnapshot
- KeybindingService.getOverrides(): KeybindingOverrides
- KeybindingService.setActionBindings(actionId, bindings): KeybindingFileSnapshot
- KeybindingService.reload(): KeybindingFileSnapshot
- KeybindingService.ensureFile(): KeybindingFileSnapshot
- StarNagService.start(): void
- StarNagService.stop(): void
- StarNagService.registerIpcHandlers(): void
- IPC: 'skills:discover' handler — async, returns SkillDiscoveryResult (supports WSL)
- IPC: 'keybindings:get' handler — returns current snapshot
- IPC: 'keybindings:setAction' handler — (actionId, bindings) → snapshot
- IPC: 'keybindings:ensureFile' handler — ensure file exists, authorize path
- IPC: 'keybindings:reload' handler — reload from disk
- IPC: 'keybindings:openFile' handler — open in default editor
- IPC: 'keybindings:revealFile' handler — show in file explorer
- IPC: 'star-nag:dismiss' handler — increase threshold (doubling), rebase baseline
- IPC: 'star-nag:complete' handler — mark starred, suppress future nags
- IPC: 'star-nag:forceShow' handler — dev-only override to show nag
- IPC send: 'keybindings:changed' (broadcast to all windows)
- IPC send: 'star-nag:show' (broadcast to main window)
- RPC: 'skills.discover' method (agent runtime)

**External dependencies**
- electron (app, BrowserWindow, ipcMain, shell APIs)
- node:fs (readFile, readdir, stat, open, realpath, mkdirSync, existsSync, writeFileSync, renameSync, unlinkSync)
- node:path (join, basename, dirname, relative, sep, isAbsolute)
- node:os (homedir, tmpdir)
- node:crypto (createHash for stable path IDs)
- ../github/client (checkOrcaStarred — shelled out gh CLI invocation)
- ../stats/collector (StatsCollector for agent spawn tracking)
- ../git/repo (isGitRepo for git detection)

**Persistence**
- ~/.orca/keybindings.json — platform-specific keybinding overrides (darwin/linux/win32 sections, atomic write via temp rename)
- PersistedUIState.starNagAppVersion — current app version baseline
- PersistedUIState.starNagBaselineAgents — agent spawn count at last threshold check
- PersistedUIState.starNagNextThreshold — next threshold to trigger nag (exponential backoff)
- PersistedUIState.starNagCompleted — permanent flag if user has starred

**Cross-platform concerns**
- WSL skill discovery on Windows via getWslHome() and getDefaultWslDistro()
- Platform-specific keybinding file location and conflict resolution (darwin/linux/win32)
- Path normalization handles both Unix (/) and Windows (\) separators
- Gitignore parsing and repo detection work across Unix and Windows filesystems

### `main-ports (Advertised URL & Port Scanning)`

Watches PTY output for HTTP(S) URLs printed by dev servers (e.g., Vite, Next.js); scans local and remote listening ports; attributes ports to workspace processes; and bridges advertised URLs to port forwarding/opening in the UI. Handles cross-platform port discovery (macOS lsof, Linux /proc/net/tcp, Windows netstat) and integrates URL caching with SSH remote connections.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca-ports with submodules: advertised-url-watcher (pure), port-scanner (io+platform), workspace-port-ownership (io), ssh-enrichment (pure). Consider vendoring: sysinfo for process metadata, procfs for /proc parsing.`  
_Core URL watcher and SSH enrichment are pure-logic (regex, caching, scoring). Platform port scanning needs native bindings: node-pty/ssh2 equivalent for PTY data, nix/heim/sysinfo for process metadata, custom /proc parsing (Linux), syscalls for lsof equivalents (macOS), WinAPI for netstat (Windows). Kill support needs os::unix::process or windows crates. Regex patterns can port directly. URL normalization logic is platform-agnostic. Candidate for staged approach: start with pure watcher+caching, defer OS port scanning to FFI or subprocess shims._

**Capabilities**
- Stateful PTY buffering to reassemble URLs split across data chunks
- ANSI/OSC escape sequence stripping from terminal output
- URL candidate extraction with trailing punctuation trimming
- Host classification (loopback/private-IP/public-IP/custom DNS) with preference scoring
- Per-port/worktree caching with LRU eviction and entry validation against listener PIDs
- Platform-specific listening port scanning via lsof (macOS), /proc/net/tcp (Linux), netstat (Windows)
- Process metadata collection (command line, working directory, process name)
- Port attribution to workspace by CWD ancestry or command-line path evidence
- Container process detection to classify non-workspace ports
- Port kill support with re-scan validation
- SSH remote port enrichment by mapping URLs to connection worktree sets
- Real-time advertised URL change events for UI broadcasts

**Public API / IPC / RPC**
- AdvertisedUrlWatcher (class constructor)
- AdvertisedUrlWatcher.bindPty(ptyId, worktreeId)
- AdvertisedUrlWatcher.unbindPty(ptyId)
- AdvertisedUrlWatcher.forgetWorktree(worktreeId)
- AdvertisedUrlWatcher.ingest(ptyId, chunk, now)
- AdvertisedUrlWatcher.lookup(worktreeId, port, currentListenerPid)
- AdvertisedUrlWatcher.lookupBest(worktreeIds, port, currentListenerPid)
- AdvertisedUrlWatcher.reconcileScan(worktreeIds, observations)
- AdvertisedUrlWatcher.invalidate(worktreeId, port)
- AdvertisedUrlWatcher.onDidChange(listener)
- AdvertisedUrlWatcher.clear()
- stripTerminalControls(text)
- extractUrlCandidates(cleaned)
- classifyHost(hostname)
- scanWorkspacePorts(worktrees, urlWatcher)
- attributePortToWorkspace(port, worktrees)
- parseLsofListeningOutput(output)
- parseNetstatListeningOutput(output)
- parseProcNetTcp(content)
- isContainerProcess(port)
- getStoreWorkspacePortProbes(store, repoId)
- filterWorkspacePortProbes(worktrees, repoId)
- killWorkspacePort(worktrees, args)
- scanWorkspacePortProbes(worktrees)
- getWorktreeIdsForConnection(store, connectionId)
- getConnectionIdsForWorktree(store, worktreeId)
- enrichSshDetectedPorts(ports, worktreeIds, watcher, options)
- enrichSshForwardEntries(entries, worktreeIds, watcher)
- IPC channel: 'workspacePorts:scan' (handler)
- IPC channel: 'workspacePorts:kill' (handler)
- IPC channel: 'workspacePorts:advertised-url-changed' (webContents.send)
- RPC method: 'workspacePorts.scan(repoId?)'
- RPC method: 'workspacePorts.kill({repoId?, pid, port})'
- advertisedUrlWatcher (singleton export)

**External dependencies**
- child_process (execFile for lsof, ps, netstat, powershell.exe)
- fs/promises (readFile, readdir, readlink for /proc scanning on Linux)
- path (path.resolve, path.basename)
- Electron (ipcMain, BrowserWindow for IPC handlers and broadcasts)
- process.platform/process.pid for cross-platform OS detection
- process.kill(pid, SIGTERM) for killing processes

**Persistence**
- AdvertisedUrlWatcher.cache (in-memory, capped at 256 entries, keyed by worktreeId::port)
- AdvertisedUrlWatcher.ptyToWorktree mapping (volatile, cleared on PTY unbind)
- AdvertisedUrlWatcher.buffers (per-PTY ANSI/URL buffering, cleared on unbind)
- AdvertisedUrlWatcher.pending (pre-bind PTY output buffer, max 32 entries)
- AdvertisedUrlWatcher.scanSnapshots (per-worktree listener observations from last reconciliation)
- AdvertisedUrlWatcher.validationBaselines (PID state for cache validation)
- AdvertisedUrlWatcher.startupAbsentAllowances (grace period for startup race conditions)
- Port scan results not persisted (computed on-demand per scan request)

**Cross-platform concerns**
- macOS: lsof -nP -iTCP -sTCP:LISTEN -F pcn (extract listening ports, process names, CWDs via lsof/ps)
- Linux: /proc/net/tcp and /proc/net/tcp6 (hex-encoded addresses), /proc/[pid]/fd (socket inode mapping), /proc/[pid]/cmdline and /proc/[pid]/cwd
- Windows: netstat -ano -p tcp and Get-CimInstance Win32_Process (PowerShell JSON output)
- IPv4/IPv6 address parsing and private-range detection (RFC 1918, link-local, ULA)
- Path normalization: case-sensitive on Linux, case-insensitive on Windows
- Wildcard bind address normalization (0.0.0.0, ::, * → localhost for connect)
- Process kill via process.kill() available on all platforms

### `main-providers: Provider Abstraction & Dispatch`

Unified abstraction layer for PTY (pseudo-terminal), filesystem, and Git operations across local and remote (SSH) targets. Provides pluggable provider implementations so the renderer/IPC layer doesn't need to know whether operations run locally or over SSH.

**Rust portability:** tier=`mixed` · effort=`XL` · target=`orca-providers (within new rust workspace); depends on tokio for async, ssh2-rs or openssh for SSH multiplexing, nix/libc for PTY/process inspection, gitoxide or git2-rs for Git ops`  
_PTY spawn on Unix requires nix crate for openpty/forkpty syscalls + signal handling; Windows needs conpty Windows API bindings. Shell-ready marker scanning is pure (S tier). SFTP read streaming needs async SFTP wrapper over ssh2-rs. Git provider needs git2-rs or gitoxide; SshGitProvider is pure RPC forwarding so maps 1:1. File watching needs Notify crate (cross-platform abstraction). Process inspection (resolveProcessCwd) is platform-specific: Unix uses /proc/[pid]/cwd symlink read, Windows uses GetProcessTimes/GetFileInformationByHandle. Shell fallback logic and environment variable injection (worktree history, WSL context, Windows path merging, Orca attribution) are pure logic. Biggest lift is maintaining node-pty-like semantics (foreground process tracking, child process enumeration) in Rust._

**Capabilities**
- Spawn/manage PTY shells locally via node-pty with shell-ready startup command support (OSC 777 markers)
- Handle Windows shell selection (PowerShell.exe vs pwsh.exe vs cmd.exe vs Git Bash) with fallback shells
- Scope shell history per-worktree to prevent command bleed between projects (HISTFILE injection)
- Manage WSL context for Windows-hosted terminals and distro selection
- Attach/reattach to existing daemon PTY sessions with snapshot/replay support
- Stream PTY I/O with data batching and acknowledgment; track foreground processes and child process subtrees
- Dispatch filesystem ops (readDir, readFile, writeFile, stat, delete, rename, copy, search, watch) to local fs or remote SFTP
- Stream large file reads via relay.fs.readFileStream with fallback to single request
- Watch filesystem for changes on local and remote targets; filter by directory scope
- Dispatch Git operations (status, diff, commit, merge/rebase, worktree management) to local libgit2 or remote git.exe
- Support non-interactive exec queue on remote Git to prevent parallel in-flight conflicts
- Detect merge/rebase conflict state and abort operations atomically

**Public API / IPC / RPC**
- LocalPtyProvider (class): spawn, write, resize, shutdown, sendSignal, getCwd, getInitialCwd, serialize, revive, listProcesses, getDefaultShell, getProfiles, onData, onExit callbacks
- SshPtyProvider (class): same IPtyProvider interface, proxies via SshChannelMultiplexer RPC (pty.spawn, pty.write, pty.exit notifications)
- SshFilesystemProvider (class): readDir, readFile, writeFile, stat, delete, rename, copy, search, watch via mux.request & mux.onNotification
- SshGitProvider (class): all IGitProvider methods (status, diff, commit, etc.) via mux.request
- registerSshPtyProvider(connectionId, provider): register SSH PTY provider in registry by connection
- registerSshFilesystemProvider(connectionId, provider): register SSH filesystem provider
- registerSshGitProvider(connectionId, provider): register SSH Git provider
- getSshFilesystemProvider(connectionId): retrieve or throw SSH_FILESYSTEM_PROVIDER_UNAVAILABLE_MESSAGE
- getSshGitProvider(connectionId): retrieve SSH Git provider
- IProviderRegistry (type): getPtyProvider(connectionId), getFilesystemProvider(connectionId), getGitProvider(connectionId) - null/undefined connectionId routes to local provider
- IPtyProvider (interface): 16 methods for PTY lifecycle (spawn, attach, write, resize, shutdown, sendSignal, getCwd, getInitialCwd, clearBuffer, hasChildProcesses, getForegroundProcess, serialize, revive, listProcesses, getDefaultShell, getProfiles)
- IFilesystemProvider (interface): 15 methods (readDir, readFile, writeFile, stat, delete, createFile, createDir, rename, copy, realpath, search, listFiles, watch)
- IGitProvider (interface): 26 methods (status, history, commit, diff, stage, unstage, discard, merge/rebase, push, pull, fetch, worktree management)

**External dependencies**
- node-pty: native PTY spawning, resize, kill, I/O streams on Unix/Windows
- electron: ipcMain for IPC handler registration, app for userData paths
- ssh2 (SFTPWrapper, Stats): SFTP file operations for remote filesystem
- child_process (execFile, spawnSync): shell exec and process inspection (resolveProcessCwd via /proc inspection or lsof)
- fs (fs/promises): local filesystem I/O, temp dir creation, chmod for executable bits
- path (basename, dirname, join, win32): cross-platform path manipulation
- os (tmpdir): OS-level temp directory

**Persistence**
- PTY lifecycle state: ptyProcesses (Map<id, node-pty.IPty>), ptyShellName, ptyLoadGeneration, ptyDisposables, ptyOwnership (id -> connectionId mapping)
- PTY session recovery: serialize/revive methods for daemon crash recovery with scrollback and CWD restoration
- Filesystem watch registrations: SshFilesystemProvider.watchListeners (Map<rootPath, callbacks>) for efficient fs.changed event filtering
- Git exec queue: SshGitProvider.nonInteractiveExecQueues (Map<worktreePath, queue>) to serialize concurrent git operations
- Shell-ready state: ShellReadyScanState (matchPos, heldBytes) to detect OSC 777 marker in PTY output

**Cross-platform concerns**
- Windows shell selection: resolveWindowsShellLaunchArgs dispatches to cmd.exe, PowerShell.exe, pwsh.exe, or Git Bash based on user preference and COMSPEC
- Windows PowerShell variant handling: resolveEffectiveWindowsPowerShell decides between boxed PowerShell 5.1 vs pwsh.exe at spawn time
- Windows ConPTY vs Unix PTY: node-pty.spawn auto-detects; different NAPI cleanup paths on exit (Windows kills via process.kill, Unix via socket close)
- WSL context detection: parseWslPath identifies //wsl.localhost/DistroName paths; wsl.exe spawned with distro selection and WSLENV merging
- Git Bash on Windows: resolveGitBashPath locates git-bash.exe from Git for Windows; handled as separate shell family
- Path normalization: win32 path module for Windows symlink and UNC path handling in filesystem ops

### `main-rate-limits`

Centralized rate limit polling service tracking token usage quotas across four AI providers (Claude, Codex, Gemini, OpenCode Go). Manages polling intervals, stale data policies, account-scoped credentials, multi-platform runtime targeting (host/WSL), and pushes state updates to the renderer via Electron IPC.

**Rust portability:** tier=`io` · effort=`L` · target=`orca-rate-limits (internal crate) + reqwest (HTTP client), tokio (async runtime), rusqlite or sled (credential caching, optional)`  
_Heavy I/O: subprocess spawning (Claude/Codex CLIs), HTTPS requests (Claude/Gemini/OpenCode APIs), Keychain FFI (macOS), file I/O (credentials, managed homes). PTY scraping logic is pure text parsing (easily portable). Main challenge: replace Electron's net.fetch + session.proxy with reqwest + system proxy integration; replace macOS Keychain API with security_framework crate; replace node-pty event handling with tokio streams. Render IPC ('rateLimits:update') becomes IPC to Swift frontend or message passing to TUI. Generation counter + stale policy logic is portable async orchestration. No DOM/React dependencies._

**Capabilities**
- Poll rate limits from Claude OAuth API endpoint
- Poll rate limits from Codex via JSON-RPC app-server or interactive PTY fallback
- Poll rate limits from Gemini via Google CloudCode API
- Poll rate limits from OpenCode Go via webpage scraping and server endpoints
- Manage OAuth credentials from Keychain (macOS), .credentials.json, and environment variables
- Target code execution to host runtime or WSL distros with account-scoped home directories
- Handle inactive/non-selected accounts with per-account caching and debounced fetch-on-open
- Apply stale data policy: keep recent snapshots on transient failures, discard stale data after 30 minutes
- Debounce background polling (15min default, 5min minimum) and window-aware polling (only when visible and focused)
- Parse CLI output from interactive PTY sessions (terminal escape sequences, percent parsing)
- Detect and handle platform-specific errors (ConPTY crashes, Tailscale DNS failures, proxy configuration)
- Queue fetch requests during in-flight cycles to coalesce rapid account switches and refresh button clicks
- Push state snapshots to renderer with 'rateLimits:update' IPC channel

**Public API / IPC / RPC**
- RateLimitService class with public methods: attach(mainWindow), start(), stop(), getState(), refresh(), refreshForCodexAccountChange(), refreshCodexForTarget(), refreshForClaudeAccountChange(), refreshClaudeForTarget(), fetchInactiveClaudeAccountsOnOpen(), fetchInactiveCodexAccountsOnOpen(), evictInactiveClaudeCache(accountId), evictInactiveCodexCache(accountId), setPollingInterval(ms), onStateChange(listener), setCodexHomePathResolver(), setCodexFetchTarget(), setClaudeAuthPreparationResolver(), setClaudeFetchTarget(), setSettingsResolver(), setInactiveClaudeAccountsResolver(), setInactiveCodexAccountsResolver()
- fetchClaudeRateLimits(options): Promise<ProviderRateLimits>
- fetchManagedAccountUsage(account): Promise<ProviderRateLimits>
- fetchCodexRateLimits(options): Promise<ProviderRateLimits>
- fetchGeminiRateLimits(geminiCliOAuthEnabled): Promise<ProviderRateLimits>
- fetchOpenCodeGoRateLimits(cookie, workspaceIdOverride): Promise<ProviderRateLimits>
- getInitialClaudeRateLimitTarget(settings, platform): ClaudeAccountSelectionTarget
- getInitialCodexRateLimitTarget(settings, platform): CodexAccountSelectionTarget
- codexAuthExists(codexHomePath): boolean
- Electron IPC channel: 'rateLimits:update' (sends RateLimitState to renderer)

**External dependencies**
- electron/net and electron/session: HTTP requests with proxy support
- node:fs and node:fs/promises: read OAuth credentials, Keychain outputs, managed auth files
- node:child_process spawn/exec: launch interactive Claude CLI, Codex JSON-RPC server
- node:os homedir: locate ~/.claude, ~/.codex, ~/.gemini directories
- node:util promisify: convert callback-based exec() to promises
- node:events EventEmitter: used in tests for mocking PTY streams
- node:crypto randomUUID: generate request IDs for OpenCode API calls
- Keychain native integration: readActiveClaudeKeychainCredentials, readManagedClaudeKeychainCredentials (macOS only via runtime-auth-service)
- claude CLI binary: spawned for /usage TUI scraping as PTY fallback when OAuth unavailable
- codex CLI binary: spawned with -s read-only -a untrusted app-server for JSON-RPC protocol
- wsl.exe: wrapped shell for Codex calls targeting WSL distros from Windows host

**Persistence**
- ~/.claude/.credentials.json: legacy plaintext OAuth credentials for Claude (read-only)
- ~/Library/Keychains/ (macOS): scoped and legacy Claude OAuth tokens via Keychain API
- CLAUDE_CONFIG_DIR env var: scoped Keychain service names for managed accounts
- ~/.codex/auth.json: Codex auth presence check (read-only file existence test)
- CODEX_HOME env var: custom Codex home directory for managed accounts
- ~/.gemini/oauth_creds.json: Gemini refresh + access tokens (read-write for token refresh)
- ~/.local/share/opencode/auth.json (Linux), ~/Library/Application Support/opencode/auth.json (macOS), APPDATA/opencode/auth.json (Windows): OpenCode session cookies (read-only)
- RateLimitService in-memory state: Map<accountId, ProviderRateLimits> for inactive accounts, with generation counters for race detection

**Cross-platform concerns**
- macOS: Tailscale DNS diagnostic hints injected into Electron net.fetch; native Keychain integration for Claude OAuth
- Windows: ConPTY crash avoidance (skips PTY fallback for active quota UI); batch script execution via cmd.exe /c wrapper; WSL distro targeting via wsl.exe with UNC path translation
- Linux/WSL: standard POSIX PTY cleanup with SIGHUP neutralization after kill; XDG_DATA_HOME / XDG_CONFIG_HOME support for OpenCode paths
- Cross-platform: HTTP_PROXY/HTTPS_PROXY env vars bridged into Electron session.proxy; process.platform checks for platform-specific fallback logic

### `main-runtime`

Multi-session workspace/terminal orchestration engine for the Electron IDE, managing PTY lifecycle, RPC dispatch to 70+ methods, session state persistence, and real-time terminal multiplexing across local and remote contexts.

**Rust portability:** tier=`mixed` · effort=`XL` · target=`orca-runtime (workspace crate); vendorable: ws, zod-equivalent validation, tweetnacl-rust/libsodium, rusqlite or sqlx`  
_Tier: mixed because RPC dispatch and orchestration are pure IO, but terminal multiplexing requires PTY platform abstractions (pty_process on Unix, Windows console APIs), Electron CDP bridge binds to JS-side browser, account/auth requires mobile-specific WebSocket transport semantics. To port: (1) Extract pure orchestration DB schemas + coordinator polling loop (io tier); (2) Build Rust RPC dispatcher with same method registry + param validation (pure/io); (3) Port terminal I/O buffering, fit negotiation, layout state (io); (4) Abstraction layer for PTY provider (platform tier, retain Electron's node-pty calls via IPC); (5) WebSocket transport + E2EE channel as dedicated module (io tier); (6) Git/file operations stay in Node.js bridge; (7) Browser/account/speech APIs remain JS-only, call via RPC. Cross-platform: WSL path normalization (io), platform detection (pure), SSH terminal backend (io, requires ssh2-rs or paho-mqtt-rust). Effort blocker: PTY multiplexing per-session requires full terminal driver state machine rewrite; mobile-specific fit/layout negotiation tightly coupled to Electron event loop._

**Capabilities**
- Workspace session lifecycle management (create, activate, sleep, remove worktrees)
- Multi-session terminal multiplexing with PTY allocation and reclamation
- Real-time terminal I/O streaming with ring-buffer limits and fit override negotiation
- Mobile floor/layout management with viewport sync and auto-restore timers
- RPC method dispatch (70+ methods across terminal, git, files, browser, orchestration, accounts, computer-use, speech)
- Orchestration engine with distributed task DAG, message queuing, coordination loops, and dispatch stale-base checking
- Browser control via Electron CDP bridge and agent browser session registry
- File watching, search, and diff operations via git-grep and ripgrep integration
- Git operations (clone, fetch, push, rebase, merge, commit, branch management)
- GitHub/GitLab/Linear/Jira issue integration with PR/MR creation and tracking
- Account/auth lifecycle (Claude, Codex, mobile device tokens, E2EE pairing)
- Computer-use (screen capture, cursor control, hotkey dispatch, app enumeration)
- Speech-to-text dictation with streaming audio chunking
- Automation rules (schedulable tasks with workspace/repo selectors)
- UI state persistence (tab layout, pane splits, settings, feature interactions)
- Memory profiling and diagnostics snapshots
- SSH terminal support with identity management
- WebSocket RPC transport with auth tokens, long-poll keepalive, and binary terminal frames
- Unix socket RPC transport for local CLI connections
- E2EE channel encryption (TweetNaCl NaCl.box) for mobile pairing
- Scrollback limits and terminal buffer serialization

**Public API / IPC / RPC**
- terminal.list, terminal.show, terminal.create, terminal.split, terminal.close, terminal.focus, terminal.rename, terminal.send, terminal.wait, terminal.subscribe, terminal.unsubscribe, terminal.read, terminal.inspectProcess, terminal.isRunningAgent, terminal.clearBuffer, terminal.multiplex, terminal.resizeForClient, terminal.setDisplayMode, terminal.getDisplayMode, terminal.updateViewport, terminal.restoreFit, terminal.getAutoRestoreFit, terminal.setAutoRestoreFit, terminal.resolveActive
- worktree.list, worktree.show, worktree.activate, worktree.create, worktree.rm, worktree.sleep, worktree.set, worktree.ps, worktree.lineageList, worktree.detectedList, worktree.forceDeleteBranch, worktree.prefetchCreateBase, worktree.resolvePrBase, worktree.resolveMrBase, worktree.persistSortOrder
- git.status, git.diff, git.history, git.stage, git.unstage, git.discard, git.commit, git.push, git.pull, git.fetch, git.fastForward, git.rebaseFromBase, git.abortMerge, git.abortRebase, git.upstreamStatus, git.bulkStage, git.bulkUnstage, git.bulkDiscard, git.branchDiff, git.branchCompare, git.commitDiff, git.commitCompare, git.conflictOperation, git.generateCommitMessage, git.generatePullRequestFields, git.cancelGenerateCommitMessage, git.cancelGeneratePullRequestFields, git.remoteFileUrl, git.checkIgnored
- files.list, files.listAll, files.read, files.readDir, files.readPreview, files.write, files.writeBase64, files.writeBase64Chunk, files.create, files.createDir, files.createDirNoClobber, files.createFile, files.delete, files.rename, files.copy, files.open, files.openDiff, files.stat, files.watch, files.unwatch, files.search, files.browseServerDir, files.listMarkdownDocuments, files.commitUpload
- browser.goto, browser.back, browser.forward, browser.reload, browser.screenshot, browser.fullScreenshot, browser.click, browser.dblclick, browser.hover, browser.focus, browser.fill, browser.type, browser.keypress, browser.keyboardInsertText, browser.select, browser.selectAll, browser.scroll, browser.scrollIntoView, browser.get, browser.is, browser.find, browser.wait, browser.eval, browser.exec, browser.console, browser.check, browser.drag, browser.upload, browser.download, browser.pdf, browser.snapshot, browser.highlight, browser.clear, browser.tabCreate, browser.tabClose, browser.tabSwitch, browser.tabShow, browser.tabCurrent, browser.tabList, browser.tabSetProfile, browser.tabProfileShow, browser.tabProfileClone, browser.profileCreate, browser.profileDelete, browser.profileList, browser.profileDetectBrowsers, browser.profileImportFromBrowser, browser.profileClearDefaultCookies, browser.cookie.get, browser.cookie.set, browser.cookie.delete, browser.storage.local.get, browser.storage.local.set, browser.storage.local.clear, browser.storage.session.get, browser.storage.session.set, browser.storage.session.clear, browser.viewport, browser.setHeaders, browser.setCredentials, browser.setMedia, browser.setDevice, browser.setOffline, browser.geolocation, browser.clipboardRead, browser.clipboardWrite, browser.dialogAccept, browser.dialogDismiss, browser.mouseMove, browser.mouseDown, browser.mouseUp, browser.mouseClick, browser.mouseWheel, browser.intercept.enable, browser.intercept.disable, browser.intercept.list, browser.network, browser.screencast.subscribe, browser.screencast.unsubscribe, browser.capture.start, browser.capture.stop
- orchestration.send, orchestration.check, orchestration.reply, orchestration.inbox, orchestration.taskCreate, orchestration.taskList, orchestration.taskUpdate, orchestration.dispatch, orchestration.dispatchShow, orchestration.ask, orchestration.reset, orchestration.run, orchestration.runStop, orchestration.gateCreate, orchestration.gateList, orchestration.gateResolve
- repo.list, repo.show, repo.add, repo.rm, repo.clone, repo.create, repo.update, repo.setBaseRef, repo.baseRefDefault, repo.searchRefs, repo.hooks, repo.hooksCheck, repo.issueCommandRead, repo.issueCommandWrite, repo.setupScriptImports, repo.reorder, repo.sparsePresets, repo.saveSparsePreset, projectGroup.list, projectGroup.create, projectGroup.update, projectGroup.delete, projectGroup.moveProject, projectGroup.scanNested, projectGroup.importNested
- github.issue, github.listIssues, github.createIssue, github.updateIssue, github.addIssueComment, github.prForBranch, github.prChecks, github.prCheckDetails, github.prComments, github.prFileContents, github.mergePR, github.updatePR, github.updatePRTitle, github.updatePRState, github.setPRAutoMerge, github.setPRFileViewed, github.requestPRReviewers, github.removePRReviewers, github.resolveReviewThread, github.addPRReviewComment, github.addPRReviewCommentReply, github.rerunPRChecks, github.listLabels, github.listAssignableUsers, github.countWorkItems, github.listWorkItems, github.workItem, github.workItemByOwnerRepo, github.workItemDetails, github.repoSlug, github.repoUpstream, github.rateLimit, github.project.listAccessible, github.project.viewTable, github.project.listViews, github.project.resolveRef, github.project.listIssueTypesBySlug, github.project.listAssignableUsersBySlug, github.project.listLabelsBySlug, github.project.updateIssueBySlug, github.project.updateIssueCommentBySlug, github.project.updateIssueTypeBySlug, github.project.updateItemField, github.project.clearItemField, github.project.updatePullRequestBySlug, github.project.addIssueCommentBySlug, github.project.deleteIssueCommentBySlug, github.project.workItemDetailsBySlug
- gitlab.listIssues, gitlab.createIssue, gitlab.updateIssue, gitlab.addIssueComment, gitlab.listLabels, gitlab.listMRs, gitlab.updateMR, gitlab.addMRComment, gitlab.addMRInlineComment, gitlab.resolveMRDiscussion, gitlab.updateMRReviewers, gitlab.updateMRState, gitlab.mergeMR, gitlab.listWorkItems, gitlab.workItemByPath, gitlab.workItemDetails, gitlab.todos, gitlab.jobTrace, gitlab.retryJob, gitlab.diagnoseAuth, gitlab.rateLimit
- linear.connect, linear.disconnect, linear.status, linear.testConnection, linear.listTeams, linear.listProjects, linear.listCustomViews, linear.getCustomView, linear.listCustomViewProjects, linear.listCustomViewIssues, linear.listProjects, linear.listProjectIssues, linear.listIssues, linear.searchIssues, linear.getProject, linear.getIssue, linear.issueComments, linear.createIssue, linear.updateIssue, linear.addIssueComment, linear.teamMembers, linear.teamLabels, linear.teamStates, linear.selectWorkspace
- jira.connect, jira.disconnect, jira.status, jira.testConnection, jira.selectSite, jira.getIssue, jira.listIssues, jira.searchIssues, jira.listProjects, jira.listIssueTypes, jira.listCreateFields, jira.listPriorities, jira.listTransitions, jira.createIssue, jira.updateIssue, jira.addIssueComment, jira.listAssignableUsers
- hostedReview.create, hostedReview.forBranch, hostedReview.getCreationEligibility
- accounts.list, accounts.selectClaude, accounts.selectCodex, accounts.removeClaude, accounts.removeCodex, accounts.subscribe, accounts.unsubscribe
- session.tabs.list, session.tabs.listAll, session.tabs.createTerminal, session.tabs.activate, session.tabs.close, session.tabs.move, session.tabs.subscribe, session.tabs.subscribeAll, session.tabs.unsubscribe, markdown.readTab, markdown.saveTab
- computer.capabilities, computer.permissionsStatus, computer.permissions, computer.listWindows, computer.listApps, computer.getAppState, computer.click, computer.drag, computer.scroll, computer.typeText, computer.pasteText, computer.hotkey, computer.pressKey, computer.performSecondaryAction, computer.setValue
- speech.dictation.start, speech.dictation.chunk, speech.dictation.finish, speech.dictation.cancel
- clipboard.startImageUpload, clipboard.appendImageUploadChunk, clipboard.commitImageUpload, clipboard.abortImageUpload, clipboard.saveImageAsTempFile
- ssh.connect, ssh.getState
- status.get, stats.summary, diagnostics.memory
- ui.set, ui.get, ui.recordFeatureInteraction, settings.update, settings.get
- notifications.subscribe, notifications.unsubscribe, runtime.clientEvents.subscribe
- preflight.check, preflight.detectAgents, preflight.detectRemoteAgents, preflight.refreshAgents
- automation.list, automation.show, automation.create, automation.update, automation.delete, automation.runNow, automation.runs
- workspacePorts.scan, workspacePorts.kill
- skills.discover
- host.platform, host.wsl.isAvailable, host.wsl.listDistros, host.pwsh.isAvailable, host.gitBash.isAvailable

**External dependencies**
- Electron (ipcMain, BrowserWindow, webContents)
- ws (WebSocket, WebSocketServer)
- tweetnacl (NaCl.box E2EE)
- node:sqlite (DatabaseSync for orchestration DB)
- child_process (subprocess spawning for git/shell)
- fs/fs.promises (file I/O)
- net (Unix socket transport)
- path (cross-platform path utilities)
- crypto (randomUUID, randomBytes, createHash for auth tokens)
- os (homedir, platform detection)
- zod (RPC parameter validation)
- git binary (via wslAwareSpawn/gitExecFileAsync)
- ripgrep (rg binary for file search)
- node-pty or equivalent PTY provider (registered via setPtyController)
- Electron CDP bridge (for browser automation)
- SSH2 client (for SSH terminal support)

**Persistence**
- orchestration DB (sqlite): messages, tasks, dispatch_contexts, decision_gates, coordinator_runs tables
- UI state store: getUI(), updateUI() - tab layout, pane splits, pane history
- Client settings store: getSettings(), updateSettings() - agentStatusHooksEnabled, feature flags
- Worktree metadata store: getAllWorktreeMeta(), setWorktreeMeta() - per-worktree state
- Automation rules store: listAutomations(), createAutomation(), updateAutomation() - scheduled task definitions
- Workspace session store: setWorkspaceSession(), getWorkspaceSession() - active terminal/worktree bindings
- TLS certificate: tls-certificate.ts - persisted SSL cert for WebSocket
- E2EE keypair: e2ee-keypair.ts - NaCl public/secret key pair
- Device registry: device-registry.ts - paired mobile device tracking
- Runtime metadata: runtime-metadata.ts - bootstrap file with transport details

**Cross-platform concerns**
- Windows WSL path handling (parseWslPath, toWindowsWslPath, wslAwareSpawn)
- macOS, Linux, Windows platform detection (process.platform, darwin/win32/linux forks)
- Cross-platform path normalization (isWindowsAbsolutePathLike, joinWorktreeRelativePath)
- WSL distro listing (host.wsl.listDistros)
- PowerShell detection (host.pwsh.isAvailable)
- Git Bash availability check (host.gitBash.isAvailable)
- SSH identity handling (cross-platform SSH terminal support)
- PTY driver negotiation (desktop vs mobile fit override, xterm emulation)

### `main-source-control`

Provider-agnostic abstraction layer for detecting and interacting with hosted code review systems (GitHub PRs, GitLab MRs, Bitbucket/Azure DevOps/Gitea PRs). Handles review discovery, creation eligibility validation, and PR/MR creation across five major Git hosting platforms.

**Rust portability:** tier=`mixed` · effort=`M` · target=`orca-scm-forge (in shared workspace) - pure Rust provider detection, enum mapping, and eligibility state machine. Depends on orca-git-exec (wraps child_process execution) for git/gh/glab subprocess calls.`  
_Core logic is pure Rust (provider detection, state validation, data mapping). CLI subprocess execution (git, gh, glab) remains in orca-git-exec. SSH relay abstraction (getSshGitProvider) must map to Rust trait (e.g., RemoteGitProvider). Network APIs (GitHub GraphQL, GitLab REST, Bitbucket REST, Azure DevOps, Gitea) can be vendored Rust HTTP clients (reqwest, octocrab for GitHub). Eligibility validation state machine is particularly amenable to Rust enums/Result types. Most effort in abstracting gh/glab CLI calls into idiomatic Rust types and integrating with credential/auth system (currently shell commands 'gh auth status', 'glab auth status')._

**Capabilities**
- Detect hosting provider from git remote (auto-detect in order: GitLab, GitHub, Bitbucket, Azure DevOps, Gitea)
- Fetch hosted review info by branch name with support for provider-specific PR/MR linking hints
- Fetch hosted review info by exact review number
- Validate PR/MR creation eligibility (checks: detached HEAD, existing review, unsupported provider, default branch, uncommitted changes, no upstream, unpushed commits, out-of-sync upstream, authentication status)
- Create pull requests on GitHub and GitLab with title, body, base/head branches, draft flag, and template support
- Normalize review refs and base refs across provider variations
- Deduplicate multiple provider detections (prioritized order preserved)
- Map provider-specific review data (state, status, mergeable, headSha, conflictSummary) to unified HostedReviewInfo type

**Public API / IPC / RPC**
- forge-provider.ts: FORGE_PROVIDERS (const array of 5 ForgeProvider objects)
- forge-provider.ts: getForgeProviderById(id: ForgeProviderId) => ForgeProvider
- forge-provider.ts: getForgeProviderForRepository(context: ForgeProviderRepositoryContext) => Promise<ForgeProvider | null>
- forge-provider.ts: detectHostedReviewProvider(context: ForgeProviderRepositoryContext) => Promise<HostedReviewProvider>
- hosted-review.ts: getHostedReviewForBranch(input) => Promise<HostedReviewInfo | null>
- hosted-review.ts: getHostedReviewByNumber(input) => Promise<HostedReviewInfo | null>
- hosted-review-creation.ts: getHostedReviewCreationEligibility(args: HostedReviewCreationEligibilityInput) => Promise<HostedReviewCreationEligibility>
- hosted-review-creation.ts: createHostedReview(repoPath, input: CreateHostedReviewInput, connectionId?) => Promise<CreateHostedReviewResult>
- IPC channels: 'hostedReview:forBranch', 'hostedReview:getCreationEligibility', 'hostedReview:create'
- RPC methods: 'hostedReview.forBranch', 'hostedReview.getCreationEligibility', 'hostedReview.create'

**External dependencies**
- child_process (execFile, spawn)
- gh CLI (GitHub authentication & PR creation)
- glab CLI (GitLab authentication & MR creation)
- git (branch inspection, remote detection, upstream status)
- Network: GitHub API (via gh CLI), GitLab API (via glab CLI), Bitbucket REST API, Azure DevOps REST API, Gitea API

**Persistence**
- No persistent cache; eligibility/review state computed on-demand
- Stats collector records PR creation events (pr_created) with URL deduplication at Electron IPC boundary

**Cross-platform concerns**
- SSH remote repositories via connectionId (uses getSshGitProvider relay for Windows, macOS, Linux hosts)
- Local file paths use platform-appropriate path resolution (path.resolve on local, posix.normalize on SSH/POSIX remotes)
- Windows WSL support routed through existing git/runner infrastructure
- Git optionalLocksDisabledEnv prevents lock contention during concurrent operations
- Detached HEAD and branch state detection works on all platforms

### `main-speech subsystem`

Speech-to-text recognition for Orca via sherpa-onnx ONNX models. Manages multiple ASR model catalogs (Parakeet, Zipformer, Whisper, Paraformer), handles streaming and offline inference, downloads/extracts/validates models from GitHub releases, and provides a warm-reusable worker-thread pool for continuous dictation with partial and final transcript delivery.

**Rust portability:** tier=`ffi` · effort=`XL` · target=`orca-speech (new crate in workspace; vendorable: sherpa-onnx Rust bindings via ort crate or ffmpeg + ONNX Runtime C++, or k2-fsa/sherpa-onnx Rust wrappers when available)`  
_sherpa-onnx is a C++ library with no official Rust bindings (as of Feb 2025). Options: (1) Use ort crate (ONNX Runtime Rust) + hand-write transducer/paraformer/whisper decoding logic (~1-2 weeks); (2) FFI wrapper around libsherpa-onnx.so/.dylib/.dll via bindgen + unsafe calls (~2-3 weeks); (3) Wait for k2-fsa upstream Rust port. Model download/extraction/validation/sample-rate conversion are pure Rust (io tier). Worker-thread architecture becomes rayon or tokio task-spawning. Platform permissions (macOS TCC microphone) require native swift calls via Foundation. Estimated 4-6 weeks to full parity with current implementation, assuming FFI route and handling of streaming state machine complexity._

**Capabilities**
- Download and verify speech-to-text models from GitHub releases with SHA256 integrity checking
- Extract tar.bz2 archives and flatten nested model directory structures
- Support streaming (online) and offline speech recognition modes
- Load and initialize sherpa-onnx (ONNX Runtime-based ASR) via platform-specific native addon
- Feed raw audio samples to recognizer with sample rate conversion/resampling
- Emit partial and final transcripts as events; signal ready/stopped/error states
- Implement hotword boosting for transducer models via BPE vocab file discovery
- Warm-reuse worker threads across multiple dictation sessions; lazy teardown with 1-hour idle timeout
- Track download progress and model state (not-downloaded, downloading, extracting, ready, error)
- Enforce microphone permission on macOS via systemPreferences.askForMediaAccess
- Multi-owner session isolation (desktop, terminal) with proper lifecycle cleanup
- Resample audio to target sample rate using linear interpolation
- Resolve platform-specific sherpa-onnx native addon paths in dev and packaged app modes

**Public API / IPC / RPC**
- ipcMain.handle('speech:getCatalog')  → SpeechModelManifest[]
- ipcMain.handle('speech:getModelStates') → SpeechModelState[]
- ipcMain.handle('speech:downloadModel', modelId: string) → void
- ipcMain.handle('speech:cancelDownload', modelId: string) → void
- ipcMain.handle('speech:deleteModel', modelId: string) → void
- ipcMain.handle('speech:startDictation', modelId: string, hotwords?: string[], sessionId?: string) → void
- ipcMain.handle('speech:feedAudio', buffer: Buffer, sampleRate: number, sessionId?: string) → void
- ipcMain.handle('speech:stopDictation', sessionId?: string) → void
- getSpeechModelManager(store: SpeechSettingsStore) → ModelManager (lazy singleton)
- getSpeechSttService(store: SpeechSettingsStore) → SttService (lazy singleton)
- SPEECH_MODEL_CATALOG: SpeechModelManifest[]
- getCatalogModel(id: string) → SpeechModelManifest | undefined
- SttEvent type union: {type:'ready'}, {type:'partial',text?:string}, {type:'final',text?:string}, {type:'stopped'}, {type:'error',error?:string}
- window.webContents.send('speech:downloadProgress', {modelId, progress})
- window.webContents.send('speech:ready', {sessionId})
- window.webContents.send('speech:partial', {text, sessionId})
- window.webContents.send('speech:final', {text, sessionId})
- window.webContents.send('speech:stopped', {sessionId})
- window.webContents.send('speech:error', {error, sessionId})

**External dependencies**
- sherpa-onnx (npm pkg v1.12.37, WASM-only, used for type defs only)
- sherpa-onnx-darwin-arm64 (native addon v1.12.37, macOS ARM64)
- sherpa-onnx-darwin-x64 (native addon v1.12.37, macOS Intel)
- sherpa-onnx-linux-arm64 (native addon v1.12.37, Linux ARM64)
- sherpa-onnx-linux-x64 (native addon v1.12.37, Linux x64)
- sherpa-onnx-win-x64 (native addon v1.12.37, Windows x64)
- Node.js built-ins: worker_threads, fs, path, crypto (hash/sha256), https, stream/promises, child_process (tar spawn)
- Electron APIs: app.getPath(), app.isPackaged, process.resourcesPath, BrowserWindow, ipcMain, systemPreferences.askForMediaAccess
- System tar binary (Windows 10+ tar.exe or system tar)

**Persistence**
- ~/.config/Orca/userData/speech-models/ (model directory tree, configurable via VoiceSettings.modelsDir)
- speech-models/{modelId}/*.onnx (model weights for transducers, paraformers, whispers)
- speech-models/{modelId}/tokens.txt (ONNX tokenizer vocab)
- speech-models/{modelId}/*.vocab (BPE vocab for hotword matching, discovered at runtime)
- app.getPath('userData')/speech-hotwords-{sha256[:12]}.txt (ephemeral hotwords per-session)

**Cross-platform concerns**
- macOS TCC microphone permission check via systemPreferences.getMediaAccessStatus/askForMediaAccess
- Windows System32/tar.exe location resolution with fallback check
- Platform-specific sherpa-onnx native addon selection (darwin-{arm64|x64}, linux-{arm64|x64}, win-x64)
- HTTPS model download redirect following (5-redirect limit)
- Path resolution for packaged vs dev: process.resourcesPath vs __dirname, app.asar vs app.asar.unpacked

### `main-startup`

Orchestrates the Orca Electron app initialization sequence: path hydration, process configuration, single-instance locking, dev instance identity tracking, CLI command redirection (AppImage), and first-window startup services (daemon PTY provider and agent hook server coordination).

**Rust portability:** tier=`io` · effort=`M` · target=`orca_startup`  
_Most modules are pure (identity generation, path merging, diagnostics logging). Shell hydration requires child_process spawn + captured stdout parsing. AppImage CLI redirection requires spawnSync and PATH checks. Single-instance lock depends on Electron/OS APIs (would become platform-specific file-based mutex in Rust). HTTP/2 network config applies to native HTTP client crate. Process supervision (parent PID tracking, signal handlers) maps to standard Rust signal crate. Major surface: need to replace Electron's app.requestSingleInstanceLock() with a native implementation (lockfile or named mutex per userData path). GPU feature flags are Electron-specific and should be removed/rewritten for SwiftUI or headless Rust renderer._

**Capabilities**
- Configure Electron network compatibility (HTTP/2 disable via persisted settings or env var)
- Patch process PATH with version manager and homebrew directories
- Configure dev/packaged user data paths and E2E isolation
- Hydrate shell PATH by spawning user login shell and capturing exports
- Acquire single-instance lock and handle second-instance focus requests
- Bypass single-instance lock for diagnostics on macOS
- Dev parent process supervision (IPC disconnect detection, watchdog polling, signal handling)
- AppImage direct CLI command detection and redirect to packaged Node entrypoint
- Startup diagnostics logging via ORCA_STARTUP_DIAGNOSTICS env var
- First-run education suppression for dev builds
- Dev instance identity generation (app name, dock badge, AppUserModelId hashing)
- Uncaught pipe error guard (suppress EIO/EPIPE, rethrow others)
- GPU feature configuration (hardware acceleration, enable-features flags)
- Concurrent startup of daemon PTY provider and agent hook server with 12s timeout
- ANSI escape sequence stripping from shell output
- PATH segment deduplication and insertion-order preservation

**Public API / IPC / RPC**
- configureElectronNetworkCompatibility(options?)
- shouldDisableHttp2ForElectronNetworking(options?)
- patchPackagedProcessPath()
- configureDevUserDataPath(isDev)
- configureOrcaUserDataPathEnv()
- shouldInstallManagedHooks(isDev)
- installDevParentDisconnectQuit(isDev)
- installDevParentWatchdog(isDev)
- installDevParentSignalQuit(isDev)
- isDevParentShutdownRequested()
- resetDevParentShutdownRequestForTests()
- installUncaughtPipeErrorGuard()
- enableMainProcessGpuFeatures()
- acquireSingleInstanceLock(app, onSecondInstance)
- shouldBypassSingleInstanceLock(options)
- logSingleInstanceLockFailure(write?)
- logSingleInstanceLockBypass(write?)
- maybeRedirectAppImageCliLaunch(options?)
- getAppImageCliArgs(argv, env, options)
- startFirstWindowStartupServices(services)
- isStartupDiagnosticsEnabled(env?)
- logStartupDiagnostic(event, details?, write?)
- writeStartupDiagnosticLine(message, write?)
- hydrateShellPath(options?)
- mergePathSegments(segments)
- _resetHydrateShellPathCache()
- getDevInstanceIdentity(isDev, env?)
- shouldSuppressDevEducation(args)
- suppressDevEducationForStore(store, now?)

**External dependencies**
- electron (app, BrowserWindow, nativeTheme)
- node:child_process (spawn, spawnSync)
- node:fs (existsSync, readFileSync, writeSync)
- node:path (join, delimiter)
- node:crypto (createHash for sha1 hashing)
- node:os (homedir, tmpdir)
- internal: ../codex-cli/command (getVersionManagerBinPaths)
- internal: ../e2e-config (getMainE2EConfig)
- internal: ../persistence (Store type)
- internal: ../../shared/types (ShellHydrationFailureReason, PersistedUIState, AppIdentity)

**Persistence**
- orca-data.json (electronHttp1CompatibilityMode persisted setting)
- userData path redirection (orca-dev for dev builds, orca for packaged)
- orca-runtime.json (RPC endpoint metadata, single-instance lock gating)
- agent-hooks/endpoint.env (hook server port and token)
- E2E temporary userData directories (isolated per spec)

**Cross-platform concerns**
- macOS: /bin/zsh default login shell, SHA1 app user model ID hashing, GPU channel early establish flags
- Linux: AppImage detection via APPIMAGE/APPDIR env vars, Xvfb GPU disabling for CI, /snap/bin and linuxbrew paths
- Windows: PATH delimiter is semicolon, Cmd.exe/PowerShell instead of POSIX shell, Path vs PATH case sensitivity
- POSIX: Shell hydration spawning with -ilc flags, process.kill(ppid, 0) for parent existence check
- Dev mode: electron-vite IPC channel disconnect vs watchdog PID polling depending on launcher
- SSH: Process parent tracking and signal forwarding reliability considerations

### `main-telemetry`

PostHog-backed telemetry transport layer for Orca. Provides a gated, rate-limited, validated event pipeline from main and renderer processes to analytics infrastructure with full consent management (env-var overrides, user opt-in/out, first-launch banner flow, CI detection).

**Rust portability:** tier=`io` · effort=`M` · target=`orca_telemetry (in new workspace) — vendor posthog-rust or wrap posthog HTTP API with reqwest for network transport; use uuid for install_id generation; std::env for env-var reads`  
_The transport logic (burst caps, validation, consent resolution, cohort classification) is pure and readily ported. PostHog SDK dependency is the main blocker — no maintained Rust SDK exists; need to either vendor the Node SDK's protocol (HTTP POST to /api/event) or implement a thin wrapper. Electron app version / IPC boundary are Electron-specific and must remain in the Swift shell via IPC callbacks. Install ID persistence can move to Rust but requires Store trait dependency. Consent env-var logic is pure and portable. Rate-limiting logic is pure. Error classification is pure (7 lines). Cohort classifiers depend on Store.getRepoCount() and Store.getOnboarding() — these become RPC calls from Rust. Shutdown flush (2s timeout) can be modeled as a tokio select! on posthog flush promise._

**Capabilities**
- PostHog event capture with configurable flush/queue settings
- Three-tier burst-cap rate-limiting (per-event-name token bucket, per-session ceiling of 1000 events, consent-mutation cap of 5 per session)
- Consent state resolution with precedence (DO_NOT_TRACK > ORCA_TELEMETRY_DISABLED > CI detection > persisted user preference)
- First-launch existing-user banner acknowledgment (silent persist without event emission) and opt-in/out event signaling
- Common properties injection (app_version, platform, arch, os_release, install_id, session_id, orca_channel) with schema validation
- Cohort classification (nth_repo_added, onboarding_cohort) injected at IPC boundary
- Error classification (error_class enum: binary_not_found vs unknown)
- IPC surface with strict input validation and threat model defense against compromised renderer
- Graceful shutdown with 2s-bounded flush and fail-safe telemetry guard (never crashes app)
- Per-event persistent install_id (UUID v4) with stability contract across launches

**Public API / IPC / RPC**
- initTelemetry(store: Store): void
- track<N extends EventName>(name: N, props: EventProps<N>): void
- setOptIn(via: OptInVia, optedIn: boolean): Promise<void>
- persistBannerAcknowledgeWithoutEmitting(): Promise<void>
- trackAppOpenedOnce(): void
- shutdownTelemetry(): Promise<void>
- shouldOptOutSdkAtInit(consent: ConsentState): boolean
- resolveConsent(settings: GlobalSettings): ConsentState
- consumeBurstToken(name: EventName): boolean
- consumeConsentMutationToken(): boolean
- resetBurstCapsForSession(): void
- initCohortClassifier(store: Store): void
- getCohortAtEmit(): { nth_repo_added: number | undefined }
- initOnboardingCohortClassifier(store: Store): void
- getOnboardingCohortAtEmit(): { cohort: OnboardingCohort | undefined }
- classifyError(err: unknown): ClassifiedError
- generateInstallId(): string
- readInstallId(store: Store): string | undefined
- IPC 'telemetry:track' channel
- IPC 'telemetry:setOptIn' channel
- IPC 'telemetry:acknowledgeBanner' channel
- IPC 'telemetry:getConsentState' channel

**External dependencies**
- posthog-node (^5.33.3) - analytics SDK client
- electron - app version, quit lifecycle hooks, IPC boundary
- node:crypto - randomUUID for install_id and session_id generation
- node:os - platform, arch, os_release detection

**Persistence**
- GlobalSettings.telemetry.installId (UUID, generated once at first launch via migration, stable across sessions)
- GlobalSettings.telemetry.optedIn (boolean | null: null = pending_banner, true = enabled, false = user_opt_out)
- GlobalSettings.telemetry.existedBeforeTelemetryRelease (boolean, set at migration for first-launch banner cohort detection)

**Cross-platform concerns**
- Detects platform via node:os.platform() (darwin, linux, win32, sunos, freebsd, openbsd, cygwin, netbsd)
- Detects arch via node:os.arch() (x64, arm64, ia32, mips, mipsel, ppc, ppc64, s390, s390x, sparc64)
- Detects OS release via node:os.release()
- CI detection checks env-var presence across GitHub Actions, GitLab CI, CircleCI, Travis, Buildkite, Jenkins, TeamCity
- DO_NOT_TRACK and ORCA_TELEMETRY_DISABLED env-var checks are cross-platform

### `main-usage subsystem (Claude/OpenCode usage tracking + stats collection)`

Tracks and aggregates Claude API usage (tokens, costs, cache metrics) and OpenCode database usage across Orca worktrees. Maintains persistent SQLite-free analytics: parses Claude transcripts from ~/.claude/{projects,transcripts}/, scans OpenCode local databases, attributes usage to worktrees/projects, computes session-level and daily aggregates, estimates costs based on current Claude API pricing. Also collects agent lifecycle stats (spawn/stop times) via OSC title detection.

**Rust portability:** tier=`io` · effort=`M` · target=`orca-usage (new crate in workspace) + better-sqlite3-sys (vendored or sys crate); serde_json for persistence`  
_Core logic is CPU-bound JSON/JSONL parsing, filtering, and aggregation with no React DOM. File I/O (transcript discovery, line reading, SQLite queries) is async-friendly via tokio. Path canonicalization can use std::fs::canonicalize. The challenge is multi-DB schema inference (tableExists/columnExists queries) — either port the inline schema sniffing logic or use a minimal SQLite wrapper (rusqlite). Pricing tables are static data. Stats event recording is straightforward. PTY ANSI stripping for meaningful content detection is regex-free in current code and portable. Electron IPC layer stays in main process (preload bridge to React); core computation moves to Rust, with Result<T> returned over IPC. Atomic file writes can use tempfile crate._

**Capabilities**
- Scan and parse Claude transcript JSONL files from ~/.claude/projects and ~/.claude/transcripts with deduplication across multiple file reads
- Parse OpenCode local SQLite databases with schema evolution handling (reads session_message, message, or session table depending on DB version)
- Attribute usage to Orca worktrees via cwd matching and path canonicalization across platforms
- Aggregate turns/events into per-session and per-day breakdowns by model, project, and location
- Compute estimated costs in USD using multi-tier Claude pricing tables (Opus 4.x, Sonnet 4.x, Haiku 4.5, legacy models, long-context thresholds)
- Calculate cache reuse rates (cache_read / (input + cache_read))
- Query usage by scope (all vs. Orca-only worktrees) and range (7d, 30d, 90d, all)
- Persist scan state and aggregates to JSON files with atomic temp-file writes
- Detect and track agent lifecycle via OSC title sequences (working → idle transitions)
- Record PR creation and agent spawn/stop events with bounded event log (10K cap) and PR deduplication

**Public API / IPC / RPC**
- ClaudeUsageStore class: setEnabled(enabled), getScanState(), refresh(force), getSnapshot(scope, range, limit), getSummary(scope, range), getDaily(scope, range), getBreakdown(scope, range, kind), getRecentSessions(scope, range, limit), getAutomationRunUsage(input)
- OpenCodeUsageStore class: setEnabled(enabled), getScanState(), refresh(force), getSnapshot(scope, range, limit), getSummary(scope, range), getDaily(scope, range), getBreakdown(scope, range, kind), getRecentSessions(scope, range, limit)
- StatsCollector class: record(event), onAgentStart(ptyId, at, repoId, worktreeId), onAgentStop(ptyId, at), hasCountedPR(prUrl), getSummary(), flush(), onAgentStarted(listener)
- AgentDetector class: onData(ptyId, rawData, at), onExit(ptyId)
- IPC channels: claudeUsage:getScanState, claudeUsage:setEnabled, claudeUsage:refresh, claudeUsage:getSnapshot, claudeUsage:getSummary, claudeUsage:getDaily, claudeUsage:getBreakdown, claudeUsage:getRecentSessions, openCodeUsage:* (same set), stats:summary

**External dependencies**
- electron (ipcMain, app.getPath, app.setName hook)
- better-sqlite3 (Database for OpenCode scan, opened in readonly mode with pragma query_only)
- fs/promises (readdir, stat, realpath for file discovery and canonicalization)
- readline (createInterface for streaming JSONL line parsing)
- homedir/os (locating ~/.claude and XDG_DATA_HOME for database discovery)
- path (platform-aware path normalization for Win32/Unix)
- child_process (none directly, but used by parent runtime for PTY spawn/exit signals)

**Persistence**
- orca-claude-usage.json: JSON file with schema version 3, processed files metadata (mtime, size, lineCount), flattened sessions/dailyAggregates, scan state (enabled, lastScanStartedAt, lastScanCompletedAt, lastScanError)
- orca-opencode-usage.json: Schema version 1, processed databases metadata, flattened sessions/dailyAggregates, scan state (same structure)
- orca-stats.json: Schema version 1, events (bounded to 10K), aggregates (totalAgentsSpawned, totalPRsCreated, totalAgentTimeMs, countedPRs dedup list, firstEventAt)
- Claude transcript source files: ~/.claude/projects/**.jsonl and ~/.claude/transcripts/**.jsonl (not owned by Orca; discovered and parsed)
- OpenCode source databases: XDG_DATA_HOME/opencode/opencode*.db files (not owned; discovered and read-only scanned)

**Cross-platform concerns**
- Path normalization: normalizeComparablePath (backward slash → forward slash, lowercase on Win32) for path comparison
- Path containment: isContainedPath handles forward/backward slashes, case-insensitive on Windows
- realpath resolution: fallback to input path on permission errors
- XDG_DATA_HOME fallback: $XDG_DATA_HOME on Linux/Mac, %LOCALAPPDATA% or %APPDATA% on Windows
- OPENCODE_DB env var: absolute or relative (resolved from XDG_DATA_HOME)
- Date/timezone: local calendar days extracted via getFullYear/getMonth/getDate to avoid UTC boundary mismatches with session timestamps
- PTY data: handles CRLF/CR normalization, ANSI/OSC escape sequences, backspace removal for meaningful content detection

### `main-window`

Manages Electron BrowserWindow lifecycle, native OS menus (macOS app-menu, File/Edit/View/Window/Help), dock badge status, window state persistence (bounds, maximized), keyboard shortcut interception (before-input-event), and IPC dispatch for renderer UI commands. Core orchestration point between main process, renderer, and OS window management.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`tauri (or winit + raw-window-handle for low-level control; macOS NSApplication + AppKit for window/menu/dock APIs)`  
_Heavy Electron BrowserWindow wrapping (lifecycle, state persistence, zoom sync). Menu building depends on shared keybinding logic (portable). Context menu generation is DOM-free (pure data). Keyboard interception via before-input-event requires native event loop integration. Dock/menu/traffic-light APIs are macOS-only and need SwiftUI equivalents. Window bounds/display validation uses Electron display APIs; substitute native screen enumeration (Cocoa on macOS, Win32 API on Windows). IPC becomes local RPC over MPSC channels or HTTP localhost. Clipboard ops map to native APIs (Cocoa NSPasteboard, Win32). Significant architecture lift: Tauri provides web-view + IPC bridge, but menu/dock/traffic-light customization requires dropping into native code or custom Swift wrapper. Alternatively, use alacritty_terminal for TUI rendering + pure-Rust model for app state; menus/dock become native shell concerns._

**Capabilities**
- Create and configure main BrowserWindow with platform-specific titlebar styles
- Persist and restore window bounds/maximized state across app restarts
- Intercept keyboard shortcuts before-input-event for app-level commands (zoom, sidebar toggle, workspace nav)
- Route focus state (markdown editor, terminal, shortcut recorder) to main process for context-aware keybinding
- Handle window lifecycle events (close, maximize, unmaximize, fullscreen, show, restore)
- Sync traffic light (macOS window controls) position with UI zoom factor
- Prevent remote page navigation to protected renderer (shell.openExternal instead)
- Validate webview guest processes (reject invalid partitions, preload, node integration)
- Context menu for editable surfaces (markdown with rich commands, native text input)
- Defer or reload renderer on crash/update with recovery timer
- Build and rebuild dynamic app menus with keybinding labels and appearance toggle checkboxes
- Handle second-instance app.activate() to focus existing window
- Register clipboard handlers (read/write text/image, save clipboard images to temp)
- Apply macOS dock badge with unread count
- Synchronize window state with runtime system (activateWorktree, createTerminal, revealsTerminalSession)
- Forward updater status and commands (check, download, quitAndInstall, dismissNudge)
- Gate media/fullscreen/pointerLock permissions

**Public API / IPC / RPC**
- createMainWindow(store, opts): BrowserWindow
- loadMainWindow(mainWindow)
- attachMainWindowServices(mainWindow, store, runtime, ...)
- registerClipboardHandlers()
- registerAppMenu(options)
- rebuildAppMenu()
- registerUpdaterHandlers(store)
- focusExistingMainWindow(options)
- setUnreadDockBadgeCount(count)
- buildEditableContextMenuTemplate(params, webContents)
- IPC handlers: clipboard:readText, clipboard:readSelectionText, clipboard:writeText, clipboard:writeSelectionText, clipboard:writeImage, clipboard:saveImageAsTempFile, app:reload, updater:getStatus, updater:getVersion, updater:check, updater:download, updater:quitAndInstall, updater:dismissNudge, window:minimize, window:maximize, window:request-close, window:isMaximized, menu:popup, ui:setMarkdownEditorFocused, ui:setTerminalInputFocused, ui:setFloatingTerminalInputFocused, ui:setShortcutRecorderFocused, ui:sync-traffic-lights, window:confirm-close
- IPC sends: window:maximize-changed, window:fullscreen-changed, window:close-requested, export:requestPdf, terminal:zoom, ui:openSettings, ui:ctrlTabKeyDown, ui:ctrlTabKeyUp, ui:dictationKeyDown, ui:toggleLeftSidebar, ui:toggleRightSidebar, ui:openQuickOpen, ui:openNewWorkspace, ui:deleteCurrentWorkspace, ui:openTasks, ui:switchRecentTab, ui:toggleWorktreePalette, ui:toggleFloatingTerminal, ui:jumpToWorktreeIndex, ui:jumpToTabIndex, ui:worktreeHistoryNavigate, ui:terminalShortcutCaptured, terminal:file-drop, ui:mobileMarkdownRequest, terminal:tabCreateReply (for runtime async)
- Runtime notifier callbacks: worktreesChanged, worktreeBaseStatus, worktreeRemoteBranchConflict, reposChanged, activateWorktree, createTerminal, revealTerminalSession, splitTerminal, renameTerminal, focusTerminal, focusEditorTab, closeSessionTab, moveSessionTab, openFile, openDiff, readMobileMarkdownTab, saveMobileMarkdownTab, closeTerminal, sleepWorktree, terminalFitOverrideChanged, terminalDriverChanged, browserDriverChanged

**External dependencies**
- electron (BrowserWindow, ipcMain, app, Menu, nativeTheme, screen, shell, clipboard, nativeImage)
- @electron-toolkit/utils (is.dev, is.prod)
- node:fs/promises (clipboard image temp file write)
- node:path (file path operations)
- node:crypto (randomUUID for request IDs)

**Persistence**
- store.getUI().windowBounds (x, y, width, height)
- store.getUI().windowMaximized (boolean)
- store.getUI().uiZoomLevel (stored and applied via setZoomLevel)
- store.getUI().lastUpdateCheckAt (timestamp)
- store.getUI().pendingUpdateNudgeId (campaign id or null)
- store.getUI().dismissedUpdateNudgeId (campaign id or null)
- store.getUI().dismissedUpdateVersion (version string or null)
- store.getSettings().windowBackgroundBlur (platform blur enablement)
- store.getSettings().appIcon (custom icon path or default)
- store.getSettings().terminalShortcutPolicy (orca-first or shell-first)
- store.getSettings().voice.enabled, .sttModel, .dictationMode
- Browser session partitions persisted per session profile in browserSessionRegistry
- Spell-checker dictionary entries per session

**Cross-platform concerns**
- macOS: hiddenInset titleBarStyle, traffic-light position sync, app-menu in system bar (File/Edit/View/Window/Help), vibrancy blur, dock.setBadge
- Windows: hidden titleBarStyle, acrylic blur, autoHideMenuBar with Alt reveal, renderer window controls (minimize/maximize/close buttons), moveTop() focus pulse
- Linux: no custom titlebar, autoHideMenuBar with Alt reveal, no native blur equivalent
- All: keybinding context awareness (terminal vs app context), modifier key mapping (Meta on macOS, Control on Win/Linux), zoom via CSS factor ^1.2
- SSH/remote: clipboard image written via SFTP to remote /tmp when connectionId provided

### `observability (error-tracking lane)`

Local-first observability and error tracking for the Electron main process. Captures distributed traces (spans) to NDJSON files with optional OTLP/HTTP export, redacts secrets from traces before serialization, and provides user-initiated bundle collection/upload to support infrastructure with consent-aware privacy controls.

**Rust portability:** tier=`io` · effort=`M` · target=`orca-observability (async tokio-based tracer, crossterm for terminal integration if needed)`  
_Pure span recording + redaction logic (no platform deps) is straightforward. NDJSON sink needs async file I/O with buffering/rotation (rewrite with tokio::fs, async-lock). OTLP exporter needs HTTP client (use reqwest or similar). Consent resolution is pure (env var reading). IPC boundary stays in Swift/Electron — only the core tracer and bundle collection logic needs to move to Rust. Store-as-ProtoBuf (not JSON) for OTLP wire format to avoid re-encoding. AsyncLocalStorage equivalent in Rust requires tokio-local storage or similar context injection pattern (lower complexity than full OpenTelemetry SDK)._

**Capabilities**
- Span recording with AsyncLocalStorage-based context propagation
- NDJSON file sink with automatic size-based rotation (10 MB x 10 files)
- Six-rule secrets redactor with provider-key fingerprints and labeled-kv patterns
- Optional OpenTelemetry/HTTP traces exporter for user-controlled collectors
- Bundle collection from rotated trace files with lookback window filtering
- Two-step authenticated bundle upload (token → upload_url → POST NDJSON)
- Bundle deletion via ticket ID
- Consent-aware privacy gating (DO_NOT_TRACK, ORCA_TELEMETRY_DISABLED, ORCA_DIAGNOSTICS_DISABLED, CI detection)
- Instrumentation helpers for git, IPC, PTY, worktree, agent, external editor, and updater lifecycle events
- Diagnostics status reporting for Privacy pane (file size, enablement state, OTLP status)

**Public API / IPC / RPC**
- initObservability() -> ObservabilityConsent
- shutdownObservability() -> Promise<void>
- resolveObservabilityConsent() -> ObservabilityConsent
- getObservabilityConsent() -> ObservabilityConsent | null
- getTraceFilePath() -> string
- getDiagnosticsStatus() -> DiagnosticsStatus
- clearLocalTraces() -> void
- collectDiagnosticBundle(meta) -> CollectedBundle
- uploadDiagnosticBundle(opts) -> Promise<UploadBundleResult>
- deleteDiagnosticBundle(opts) -> Promise<void>
- withSpan<T>(name, fn, options) -> Promise<T>
- startSpan(name, options) -> ActiveSpan
- getActiveSpanContext() -> SpanContext | undefined
- setActiveSink(sink) -> void
- withGitSpan(meta, fn) -> Promise<T>
- withIpcSpan(meta, fn) -> Promise<T>
- withWorktreeSpan(meta, fn) -> Promise<T>
- withPtySpan(meta, fn) -> Promise<T>
- withAgentSpan(meta, fn) -> Promise<T>
- withExternalEditorSpan(meta, fn) -> Promise<T>
- withUpdaterSpan(meta, fn) -> Promise<T>
- IPC diagnostics:getStatus
- IPC diagnostics:openTraceFolder
- IPC diagnostics:clearTraces
- IPC diagnostics:collectBundle
- IPC diagnostics:openBundlePreview
- IPC diagnostics:discardBundlePreview
- IPC diagnostics:uploadBundle
- IPC diagnostics:deleteBundle

**External dependencies**
- electron (app, ipcMain, dialog, shell)
- node:async_hooks (AsyncLocalStorage)
- node:crypto (randomBytes)
- node:fs (openSync, writeSync, readFileSync, statSync, unlinkSync, renameSync, etc.)
- node:http (request, ClientRequest, IncomingMessage)
- node:https (request)
- node:os (homedir, platform, tmpdir, arch, release)
- node:path (join, dirname)
- node:url (URL)
- node:events (EventEmitter)

**Persistence**
- NDJSON trace file at ~/Library/Application Support/Orca/logs/main.trace.ndjson (macOS) or platform-equivalent
- Rotated trace family: main.trace.ndjson, main.trace.ndjson.1 through main.trace.ndjson.9
- Temporary bundle preview files in system tmpdir
- File permissions: 0o700 directories, 0o600 files (user-only read/write)

**Cross-platform concerns**
- macOS: ~/Library/Application Support/Orca
- Windows: %APPDATA%/Orca
- Linux: ~/.config/Orca
- CIFS filesystem compatibility (close before rename)
- File permission hardening via fchmodSync with Windows best-effort fallback
- Cross-platform shell invocation for file manager (shell.showItemInFolder)
- HTTP and HTTPS endpoint support with protocol detection

### `text-generation + hermes (agent hooks)`

Manages local and remote text generation for commit messages, pull request descriptions, and branch names via external LLM agents (claude, codex, cursor, pi, opencode, omp); integrates with Hermes agent framework to monitor LLM lifecycle hooks and report status back to Orca via HTTP.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca-text-generation (pure subprocess management); orca-hermes-plugin (file/config I/O + Python code generation + SSH SFTP)`  
_Text generation is primarily I/O-bound process spawning (candidate for std::process::Command + tokio) and agent communication. Hermes plugin installation is file/config I/O (fs, yaml parsing). Core logic is platform-agnostic (prompt building, model discovery parsing, result formatting). Platform-specific: Windows batch detection + cmd.exe wrapper routing (inline), macOS DNS diagnostics (optional feature), WSL UNC path detection (inline), taskkill/SIGKILL branching (inline). SSH remote execution uses sftp_rs or paramiko bindings (future). Hermes Python plugin __init__.py is generated as string literal; shipping vendored or delegating to Python installation. Agent discovery/execution reuses shared CommitMessagePlan structures. Challenge: timeout handling + cancel tokens require background task spawning; initial port uses tokio::task. macOS Tailscale DNS hint is UI-layer concern; Rust version can emit structured error codes + let renderer decide._

**Capabilities**
- Generate commit messages from staged git diffs
- Generate pull request fields (title, description) from branch diffs
- Generate branch names from work context
- Discover available LLM models from agents dynamically via --list-models
- Execute agents locally via child_process spawn/exec
- Execute agents remotely via SSH SFTP with process planning
- Cancel in-flight generation operations per worktree
- Resolve agent environment variables (CODEX_HOME, HERMES_HOME, PI_CODING_AGENT_DIR, ORCA_*_SOURCE_* shadow vars)
- Parse and apply Claude auth patches to agent subprocess env
- Handle Windows batch script launching via cmd.exe wrapper + taskkill /T /F tree killing
- Resolve Windows command line safety constraints for prompts in argv
- Sanitize and truncate agent stderr/stdout for user-facing error messages
- Generate Hermes plugin YAML manifest and Python __init__.py
- Enable/disable Hermes orca-status plugin in config.yaml
- Validate Orca-managed Hermes plugin file integrity
- Install/remove Hermes plugin on local filesystem
- Install/remove Hermes plugin remotely via SFTP
- Report Hermes install status (installed/partial/not_installed/error)
- Post hook events via HTTP to Orca (session start/end, LLM calls, tool calls, approval requests)

**Public API / IPC / RPC**
- generateCommitMessageFromContext(context, params, target) -> GenerateCommitMessageResult
- generatePullRequestFieldsFromContext(context, params, target) -> GeneratePullRequestFieldsResult
- generateBranchNameFromContext(context, params, target) -> GenerateBranchNameResult
- discoverCommitMessageModelsLocal(agentId, env, agentCommandOverride) -> DiscoverCommitMessageModelsResult
- discoverCommitMessageModelsRemote(agentId, cwd, execute, agentCommandOverride) -> DiscoverCommitMessageModelsResult
- cancelGenerateCommitMessageLocal(cwd) -> void
- cancelGeneratePullRequestFieldsLocal(cwd) -> void
- resolveCommitMessageSettings(settings, discoveryHostKey, operation, repo) -> ResolveCommitMessageSettingsResult
- getPullRequestDraftContext(execGit, input) -> PullRequestDraftContext | null
- prepareLocalCommitMessageAgentEnv(agentId, resolvers) -> {ok: true; env?: NodeJS.ProcessEnv} | {ok: false; error: string}
- HermesHookService.getStatus() -> AgentHookInstallStatus
- HermesHookService.install() -> AgentHookInstallStatus
- HermesHookService.installRemote(sftp, remoteHome) -> Promise<AgentHookInstallStatus>
- HermesHookService.remove() -> AgentHookInstallStatus

**External dependencies**
- node:child_process (spawn, exec)
- ssh2 (SFTPWrapper type for remote file ops)
- yaml (parse, stringify)
- node:crypto (randomUUID)
- node:fs (readFileSync, writeFileSync, mkdirSync, rmSync, etc.)
- node:os (homedir)
- node:path (join, dirname, resolve)

**Persistence**
- GlobalSettings.sourceControlAi (SourceControlAiSettings with selectedModelByAgent, selectedThinkingByModel, customPrompt, customAgentCommand)
- GlobalSettings.commitMessageAi (legacy CommitMessageAiSettings)
- Hermes config.yaml (plugins.enabled, plugins.disabled, custom plugin fields)
- Hermes plugin files (~/.hermes/plugins/orca-status/plugin.yaml, __init__.py)
- Orca agent hook endpoint config file (ORCA_AGENT_HOOK_ENDPOINT env var points to file with set KEY=VALUE pairs)

**Cross-platform concerns**
- Windows: taskkill /T /F for process tree termination (child.kill() inadequate for .cmd wrappers)
- Windows: cmd.exe /c routing for .cmd/.bat script execution
- Windows: PATH vs. Path environment variable fallback
- macOS: Tailscale DNS diagnostic hints injected into error messages (withMacTailscaleDnsHint)
- macOS: Shell startup env var reading for agent config dirs (~/.bashrc, ~/.zshrc, etc.)
- WSL: WSL UNC path detection and handling for CODEX_HOME
- Linux/macOS: SIGKILL process termination
- All: Cross-platform agent binary resolution (codex, claude, pi, opencode, omp, cursor)
- All: Platform-aware version comparison for discovered models (parseVersionSegment, compareVersionDesc)

## renderer

### `ui-automations`

Provides a complete UI for Orca users to schedule and manage agent automation runs. Includes creation/editing of automations with cron scheduling, precheck commands, session management, run history tracking with token/cost attribution, and integration with external automation systems (Hermes, OpenClaw).

**Rust portability:** tier=`ui-native` · effort=`L` · target=`orca-ui-automations (new crate in the Rust workspace)`  
_Pure UI-native port: no file IO, no process management, all backend calls delegate to RPC/HTTP. Rebuild as native SwiftUI views (macOS) with custom pickers for schedule/agent/workspace selection. Hermes output parser can be ported as pure Rust with markdown parsing. Time formatting and cron validation helpers are pure and portable. IPC becomes HTTP API calls to the Rust backend. Key dependencies: custom Schedule picker component (date/time picker UI), markdown renderer for Hermes output, cron expression formatter (cron-rs crate exists). External automations (Hermes/OpenClaw) table requires only read-only API bindings. Session/workspace reuse logic is pure state management with no side effects in UI layer._

**Capabilities**
- Schedule agent tasks via cron expressions (presets: hourly, daily, weekdays, weekly, custom)
- Create/edit/delete/pause/enable automations with multi-field configuration (prompt, agent, workspace mode, session reuse)
- Run automations immediately via 'Run Now' button
- Track run history with status badges (queued, started, launched, done, failed, skipped variants)
- Display automation run output as terminal snapshots or workspace state
- Collect and attribute token usage and estimated costs to automation runs
- Execute precheck shell commands before scheduled automation dispatch
- Support both local and SSH-based execution targets
- Manage missed run grace windows (configurable 30min-48hr window)
- Support workspace reuse vs fresh workspace per run
- Browse available automation templates (repo health, release prep, maintenance checks)
- Manage external automations via Hermes and OpenClaw providers
- Paginate and view external automation run history
- Parse and display Hermes cron output with formatted markdown sections
- Search and select base branches for 'create from' workspace mode

**Public API / IPC / RPC**
- window.api.automations.create(AutomationCreateInput): Automation
- window.api.automations.update({id, updates: AutomationUpdateInput}): Automation
- window.api.automations.delete({id: string}): void
- window.api.automations.runNow({id: string}): Promise<AutomationRun>
- window.api.automations.runPrecheck({automationId, runId}): Promise<AutomationPrecheckResult | null>
- window.api.automations.markDispatchResult(AutomationDispatchResult): Promise<AutomationRun>
- window.api.automations.snapshotWorkspaceName({workspaceId, displayName}): number
- window.api.automations.createExternal(ExternalAutomationCreateInput): Promise
- window.api.automations.updateExternal(ExternalAutomationUpdateInput): Promise
- window.api.automations.runExternalAction(ExternalAutomationActionInput): Promise
- ipcMain handlers: automations:list, automations:listRuns, automations:listExternalManagers, automations:listExternalRuns, automations:create, automations:update, automations:delete, automations:runNow, automations:runPrecheck, automations:markDispatchResult, automations:snapshotWorkspaceName, automations:rendererReady
- useAppStore: selectedAutomationId, setSelectedAutomationId, closeAutomationsPage, recordFeatureInteraction('automation-created', 'automation-run'), hydratePersistedUI
- Event: 'orca:automations-changed'

**External dependencies**
- react (hooks: useState, useCallback, useEffect, useMemo, useRef)
- lucide-react (icons: CalendarClock, Play, Pause, Pencil, Trash2, Check, RefreshCw, Plus, X, Eye, Clock, etc.)
- sonner (toast notifications)
- @radix-ui (Dialog, ToggleGroup, Select, Popover, Tabs, Tooltip, ContextMenu components)
- electron (ipcMain, ipcRenderer for IPC communication)

**Persistence**
- sqlite: automations table (id, name, prompt, agentId, projectId, workspaceMode, workspaceId, baseBranch, reuseSession, enabled, rrule, nextRunAt, lastRunAt, executionTargetType, executionTargetId, precheckCommand, precheckTimeoutSeconds, missedRunGraceMinutes)
- sqlite: automationRuns table (id, automationId, status, scheduledFor, dispatchedAt, workspaceId, terminalSessionId, workspaceDisplayName, trigger, usage)
- electron-store: selectedAutomationId (persisted UI state)

**Cross-platform concerns**
- SSH automation execution: supports remote execution with authentication handling, connection status checking, precheck over SSH
- Local vs SSH execution targets with separate precheck command execution paths
- Workspace availability detection across local worktrees
- Terminal session availability checks (macOS via Electron, Linux via terminal emulation)
- Cross-platform schedule evaluation and timezone-aware formatting

### `ui-browser-pane`

Renders and manages embedded browser tabs within Orca's IDE. Provides a multi-page browser UI with address bar, navigation, grab-mode element annotation, remote browser streaming, and mobile driver control. Coordinates Electron webviews locally and remote browser daemons via RPC.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-ui-browser (new crate in renderer workspace). UI rebuilt in SwiftUI on macOS; Rust logic split: (1) thin UI glue → swift-rs FFI, (2) webview/DOM manipulation → alacritty_terminal integration, (3) RPC/state management → orca-runtime-client, (4) grab/annotation logic → orca-browser-annotation (pure Rust)`  
_This is the UI-native tier because it manages live Electron webviews and complex React state (grab mode FSM, remote stream subscription, viewport sync, keyboard input queueing). Rewrite requires: (1) SwiftUI view for browser chrome (address bar, toolbar, tabs), (2) native Cocoa webview embedding (WKWebView on macOS instead of Electron), (3) equivalent of alacritty_terminal for remote screencast rendering, (4) port grab-mode state machine and annotation pipeline, (5) reimport cookie/session profiles. The grab-mode element selection and annotation UI is inherently DOM/CSS-dependent; remote browser interaction is pure RPC (portable). Address bar autocomplete, zoom, find-in-page are portable. Drag passthrough, focus stealing prevention are macOS-specific. Total scope: 2-3 weeks for SwiftUI port + integration._

**Capabilities**
- Render local Electron webviews (single/multi-tab with tab switching)
- Stream remote browser frames via screencast protocol over RPC to runtime
- Navigate URLs with history-based address bar autocomplete
- Find-in-page with next/previous match navigation
- Element grab/annotation: pick elements, attach context + metadata to agent chat
- Webpage zoom in/out/reset with percent display
- Download management: request, progress tracking, completion notices
- Permission/popup/error notice display for page load failures
- Mobile driver mode: pause local input, show 'Take back' dialog when remote controls browser
- Viewport override (responsive device presets: desktop, iPad, iPhone, etc.)
- Drag passthrough for tab reordering (transparent webview during drag)
- CSS anchor positioning for tab moves (avoid webview reparenting/destruction)
- Keyboard shortcuts: reload, hard reload, find, zoom, grab toggle, focus address bar
- Context menu: open link in new tab, copy link, open in external browser
- Cookie import from system browsers into session profiles
- Screenshot capture for selected elements
- Page state sync (title, URL, load progress, favicon, canGoBack/canGoForward)

**Public API / IPC / RPC**
- BrowserPane (main export, React component)
- BrowserPaneOverlayLayer (overlay positioning via CSS anchors)
- BrowserAddressBar (address bar with history dropdown)
- BrowserFind (find-in-page bar)
- BrowserToolbarMenu (viewport/profile/cookie import menu)
- BrowserMobileDriverOverlay (mobile driver lock dialog)
- GrabConfirmationSheet (element grab review UI)
- useGrabMode (grab lifecycle state machine hook)
- rememberLiveBrowserUrl, getLiveBrowserUrl, clearLiveBrowserUrl (tab URL tracking)
- registerPersistentWebview, unregisterPersistentWebview, destroyPersistentWebview (webview registry)
- acquireWebviewsDragPassthrough, setWebviewsDragPassthrough (drag mode passthrough)
- buildBrowserAddressBarSuggestions (history/search suggestions)
- formatBrowserAnnotationsAsMarkdown (convert annotations to agent prompt text)
- isEditableKeyboardTarget (check if event target is editable)
- getBrowserPagesForWorkspace (filter pages by workspace)
- ORCA_BROWSER_FOCUS_REQUEST_EVENT, queueBrowserFocusRequest, consumeBrowserFocusRequest (focus IPC)
- ORCA_BROWSER_PAGE_ZOOM_EVENT, applyBrowserPageZoom, browserPageZoomLevelToPercent (zoom control)
- isBrowserAutomationVisible, acquireBrowserAutomationVisibility (automation visibility tokens)
- getRemoteBrowserKeyboardShortcut, getRemoteBrowserKeypressKey (remote input mapping)

**External dependencies**
- react: hooks (useState, useEffect, useCallback, useRef, useMemo, useLayoutEffect, createPortal)
- zustand: useAppStore state management
- lucide-react: icons (Globe, Search, Copy, Trash2, RefreshCw, ArrowLeft, ArrowRight, Loader2, etc.)
- @/components/ui/*: radix-ui components (Button, Input, Dialog, Dropdown, Popover, Tooltip, Toggle, ScrollArea, Command)
- sonner: toast notifications
- electron: webview tag, FindInPageOptions, FoundInPageEvent, IPC via window.api.*
- window.api.browser.*: IPC to main thread browser API
- window.api.ui.*: IPC for zoom, reload, find, clipboard, focus signals
- window.api.runtime.*: RPC to runtime daemon (browser.tabCreate, browser.eval, browser.viewport, browser.tabShow, browser.tabClose, etc.)
- window.api.fs.*: file path authorization
- window.api.shell.*: open external URLs
- window.api.runtimeEnvironments.*: subscribe to active environment changes

**Persistence**
- browserPagesByWorkspace (Zustand store): array of BrowserPageState (title, URL, loading, favicon, canGoBack, canGoForward, loadError, annotations, viewportPresetId)
- browserTabsByWorktree (Zustand store): array of BrowserWorkspaceState (activePageId, sessionProfileId, worktreeId)
- browserUrlHistory (Zustand store): recent URLs for address bar autocomplete
- browserDefaultSearchEngine (Zustand store): search engine selection
- browserKagiSessionLink (Zustand store): Kagi session token
- browserSessionProfiles (Zustand store): per-profile cookies and import state
- remoteBrowserPageHandlesByPageId (Zustand store): mapping of local page ID to remote (environmentId, remotePageId) for session restore
- webviewRegistry (module-level Map): local tab ID → Electron webview element
- registeredWebContentsIds (module-level Map): local tab ID → Electron webContentsId
- liveBrowserUrlByTabId (module-level Map): transient URL cache for tab restoration

**Cross-platform concerns**
- macOS: window.focus() to prevent reactivation of previously-frontmost app when hiding focused webview
- All platforms: webview.findInPage(), webview.stopFindInPage() (Electron API)
- All platforms: Electron zoom level management (exponential to percentage conversion)
- All platforms: drag passthrough via pointer-events CSS manipulation
- URL handling: file:// → absolute path conversion (handles /C:/ prefix on Windows)
- localhost-like detection: 127.0.0.1, ::1, 0.0.0.0 for local server connection hints

### `ui-editor subsystem`

Unified code/diff editor surface supporting multiple file types (source code, markdown, images, diffs, notebooks, mermaid, CSV) with dual editing modes (Monaco for source, ProseMirror/Tiptap for rich markdown), real-time Git conflict resolution, and diff comments for code review workflows.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-editor (or orca-ui-editor within a larger orca-ui crate); use existing markdown parsing crates (pulldown-cmark, syntect for highlighting) and port ProseMirror/Tiptap behavior to a Rust state machine + SwiftUI/AppKit view controllers (macOS) or direct OpenGL/skia-safe canvas rendering if web-based fallback is needed`  
_This is the most complex UI subsystem in Orca. Porting requires: (1) Rust parser/renderer for Tiptap extensions (code blocks, tables, math, details, links, images, task lists) as a state machine, (2) macOS-native text editor backed by NSTextView or Cocoa text handling, (3) Monaco editor equivalent (probably lightweight Rust editor library like helix-core or implement custom one), (4) markdown preview rendering (pulldown-cmark + CSS/layout engine), (5) diff viewer (likely custom, similar to delta editor pattern), (6) combined diff virtualization (needs efficient range-tree for lazy section loading), (7) image/PDF/Mermaid viewers as WebViews or native controls, (8) notebook cell execution UI and output rendering, (9) conflict UI and Git integration (parse conflict markers, highlight blocks), (10) all keyboard shortcuts, selection/copy behavior, view state persistence, autosave lifecycle. The edit surface itself (ProseMirror) is deeply JS/DOM-coupled; a faithful Rust port would need a similar CRDT-like transaction system. Consider phased approach: stage 1 = Monaco-equivalent editor + basic markdown preview, stage 2 = rich markdown editor (Tiptap clone), stage 3 = diff/conflict views, stage 4 = notebooks/media viewers._

**Capabilities**
- Multi-mode file editing (edit, diff, conflict-review, markdown-preview, combined-diff, changes-view)
- Monaco-based source code editor with syntax highlighting, conflict markers, Git diff decorations, and line-by-line diff comments
- Rich markdown editing via Tiptap/ProseMirror with WYSIWYG formatting, tables, math, code blocks, details/summary elements, and TOC generation
- Markdown preview rendering with remark/rehype pipeline (GFM, frontmatter, math, syntax highlighting, sanitization, internal/external link routing)
- Source/rich/preview mode switching for markdown files with automatic fallback on unsupported content
- Diff viewer (single-pane and side-by-side) for Git diffs, supporting editable unstaged changes and read-only historical diffs
- Combined/multi-file diff viewer with lazy-loaded sections, virtualizer-based rendering, file tree navigation, and diff section persistence
- Conflict resolution UI with conflict block highlighting, navigation between unresolved blocks, and conflict review panel with file-level comparison
- Changes view mode showing unstaged modifications in diff format with inline editing
- Diff comments (line-level and selection-level) on markdown source with comment decorators, popovers, and copy-to-clipboard formatting
- Notebook (.ipynb) viewer and editor with cell execution, output rendering, cell reordering, and cell code/markdown/raw type support
- Image viewer (PNG, JPEG, WebP, SVG) with zoom, pan, fullscreen, rotation controls, and diff viewer for binary image changes
- Mermaid diagram viewer for .mmd/.mermaid files with diagram-to-source toggle
- CSV viewer with delimiter detection and tabular display in rich mode
- PDF viewer with search, navigation, and page-level rendering
- Markdown frontmatter (YAML/TOML) extraction, display as read-only banner, and preservation during edits
- Autosave with dirty state tracking, draft buffering per file, and undo/redo persistence across tab switches
- Editor state restoration (cursor position, scroll offset, fold state) using LRU caches keyed per file path and view
- Search/find functionality with match highlighting in both Monaco and markdown preview surfaces
- Keyboard shortcuts: Cmd+S/Ctrl+S to save, Cmd+F/Ctrl+F for find, modal-aware context menu on gutter right-click
- Local image insertion in markdown with workspace-relative path resolution and image preview in rich editor
- Document link resolution (@-mentions) with auto-completion and pop-up menu for Orca internal markdown document navigation
- Markdown TOC (table of contents) generation with collapsible sidebar and smooth scroll-to-heading navigation
- Split-pane editor support with shared Monaco models, view-state isolation, and coherent dirty/draft state across panes
- Language detection based on file extension with fallback to Monaco language ID
- Readonly mode for versioned/historical file views (diffs from branches, commits) and temporary conflict placeholders
- Auto-height Monaco editors for conflict review inline file previews
- Selection copy contextual behavior (primary selection on Linux, regular clipboard on macOS/Windows)
- Export active markdown to PDF via backend HTML→PDF conversion
- File rename dialog for untitled files with directory picker and path validation

**Public API / IPC / RPC**
- EditorPanel (default export, main React component)
- EditorContent function
- MonacoEditor (default export, lazy)
- RichMarkdownEditor (default export, lazy)
- DiffViewer (default export, lazy)
- CombinedDiffViewer (default export, lazy)
- MarkdownPreview (default export, lazy)
- ImageViewer (default export, lazy)
- ImageDiffViewer (default export, lazy)
- MermaidViewer (default export, lazy)
- CsvViewer (default export, lazy)
- IpynbViewer (default export, lazy)
- PdfViewer (default export, lazy)
- EditorAutosaveController (default export)
- EditorPanelShell, EditorPanelHeader, EditorViewToggle (supporting components)
- extractFrontMatter, prependFrontMatter (markdown frontmatter utilities)
- buildMarkdownTableOfContents (TOC generation)
- formatMarkdownReviewNotes, sortMarkdownReviewNotes, copyMarkdownReviewNotesForAgent (review note formatting)
- getMarkdownSourceLineOffset (frontmatter line offset calculation)
- findGitConflictBlocks, buildGitConflictDecorations (conflict detection/decoration)
- getNextConflictNavigationIndex (conflict navigation logic)
- getCombinedDiffFileTreeSectionKey, createCombinedDiffSectionIndexMap, handleCombinedDiffFileTreeNavigation (combined diff tree utilities)
- createCombinedDiffLoadScheduler (diff section lazy-loading)
- parseCsv, detectCsvDelimiter (CSV parsing)
- window.api.ui.setMarkdownEditorFocused, window.api.ui.onRichMarkdownContextCommand, window.api.ui.onExportPdfRequested (IPC channels to preload/main)

**External dependencies**
- react (19.2.5) - UI framework
- @monaco-editor/react (4.7.0) - Monaco editor wrapper and diff editor
- monaco-editor (0.55.1) - code editor with syntax highlighting and conflict markers
- @tiptap/react, @tiptap/core (3.22.5) - rich text editor framework
- @tiptap/starter-kit, @tiptap/markdown - markdown parsing and node schema
- @tiptap/extension-* (code-block-lowlight, details, image, link, mathematics, placeholder, tables, task-item/list) - rich editor extensions
- @tiptap/pm (prosemirror-state, prosemirror-view, prosemirror-model, prosemirror-tables) - underlying CRDT/edit model
- lowlight (3.3.0) - syntax highlighting for code blocks via @tiptap/extension-code-block-lowlight
- react-markdown (10.1.0) - markdown rendering in preview mode
- remark-gfm, remark-breaks, remark-frontmatter, remark-math, remark-parse - remark parsing plugins
- rehype-highlight, rehype-katex, rehype-raw, rehype-sanitize, rehype-slug - rehype HTML transformations
- katex (0.16.45) - LaTeX math rendering
- mermaid (11.15.0) - diagram rendering
- pdfjs-dist (5.7.284) - PDF rendering in viewer
- dompurify (3.4.2) - HTML sanitization for markdown preview
- html-to-image (1.11.13) - canvas export for markdown screenshots
- @tanstack/react-virtual (3.13.24) - virtualizer for combined diff lazy-rendering
- lucide-react (0.577.0) - icon library
- sonner (2.0.7) - toast notifications
- @xterm/xterm, @xterm/addon-* (terminal emulation) - used for terminal preview in notebooks
- window.api.* - preload bridge to main/backend (fs, shell, export, notebook, ui, session)
- useAppStore (Zustand) - global state (file contents, drafts, editor settings, scroll caches)
- @/lib/scroll-cache - view state caching system
- @/lib/monaco-setup - Monaco language configuration and themes

**Persistence**
- scrollTopCache - Map, per-file scroll position (LRU-evicted)
- cursorPositionCache - Map, per-file cursor line/column (LRU-evicted)
- diffViewStateCache - Map, diff editor view state (model selection, folding state)
- combinedDiffViewStateCache - Map, combined diff entries, loaded sections, scroll offset, side-by-side preference
- editorDrafts (Zustand store) - in-memory draft content per file ID (survives tab close until file close)
- markdownViewMode (Zustand) - per-file markdown view preference (source/rich/preview)
- editorViewMode (Zustand) - per-file editor mode (edit/diff/changes)
- markdownFrontmatterVisible (Zustand) - frontmatter banner visibility per file
- editorFontZoomLevel (Zustand) - global editor font zoom factor
- fileContents (Zustand) - loaded file content (lazy-loaded, cleared on file close)
- diffContents (Zustand) - computed Git diff result per file (regenerated on worktree changes)

**Cross-platform concerns**
- macOS: primary selection middle-click paste in Monaco via settings.primarySelectionMiddleClickPaste
- Linux: primary selection middle-click paste via isLinuxUserAgent check in Monaco options
- Windows: standard clipboard behavior (no primary selection)
- Monaco Linux: custom gutter context menu instead of built-in native menu (cross-platform parity)
- Keyboard: platform-specific Cmd (macOS) vs Ctrl (Windows/Linux) for Cmd+S, Cmd+F shortcuts
- Theme: system dark-mode detection via matchMedia('(prefers-color-scheme: dark)') for auto-switching
- Shell integration: window.api.shell.openPath (cross-platform file explorer reveal, Electron Shell API)
- PDF export: window.api.export.htmlToPdf delegates to backend (handles platform-specific PDF rendering)
- Notebook execution: window.api.notebook.runPythonCell (SSH2/remote Python execution in backend)
- Terminal rendering: @xterm/addon-webgl for cross-platform terminal emulation in notebook outputs

### `ui-feature-wall`

Multi-modal feature education and onboarding system delivering contextual tours (overlays pinned to UI targets), feature tips (modal dialogs with setup actions), and an interactive feature-wall (workflow-based guided tour through Orca's capabilities).

**Rust portability:** tier=`ui-native` · effort=`L` · target=`orca-ui-feature-education (new; coords with orca-shared for tour definitions, orca-renderer-bridge for IPC)`  
_Heavy React/DOM dependency in ContextualTourOverlay (portal rendering, MutationObserver, target measurement via getBoundingClientRect, floating panel positioning math, focus management). Feature-wall primarily data-driven UI composition; animatedVisuals are pure rendering logic (SVG/canvas). Tour positioning logic (overlay-position.ts) is pure math—portable. Refactor needed: (1) Extract positioning logic to shared pure Rust lib. (2) Keep React components in Swift/SwiftUI with native UIView containers for portal slots. (3) Animated visuals (SVG storyboards) need native Canvas/CoreGraphics rewrites. (4) MutationObserver retry loop becomes platform-native surface observation (UIView hierarchies on macOS). (5) Telemetry and persistence (localStorage) become native code. Zustand store selectors map to SwiftUI @ObservedObject. IPC: feature-wall doesn't shell out but reads settings/preflightStatus from store (fetch those via async IPC instead). Focus trap logic needs native implementation._

**Capabilities**
- Display full-screen feature-wall modal with workflow-based step navigation (agents-orchestration, workbench, review)
- Track tour completion/visited state persisted to localStorage
- Render contextual tours as floating panels with portal rendering, target measurement, and repositioning on scroll/resize
- Show feature tips modals with CLI installation workflows, voice setup, and orchestration skill enablement
- Measure and track telemetry on tour depth, dwell time, exit actions, and user progression
- Position overlays intelligently with fallback placement (top/bottom/left/right) relative to target elements
- Handle keyboard navigation in tours (Escape to skip, arrow keys, Enter/Space for actions)
- Render animated visual demonstrations (editor slash-menu, browser use, computer use, tasks, workspaces)
- Manage multi-step setup actions with progress indicators and checkmark completion states
- Gate tours by modal state, onboarding visibility, and feature interaction history
- Auto-trigger contextual tours on surface visibility with MutationObserver and retry logic

**Public API / IPC / RPC**
- FeatureWallModal (default export)
- FeatureTipsModal (default export)
- ContextualTourOverlay (named export)
- getFeatureWallOpenSource(modalData: Record<string, unknown>): FeatureWallOpenSourceTelemetry
- getFeatureTipForModal(args): FeatureTip | null
- FeatureWallTourSurface (component with onDone callback)
- useFeatureWallCompletion(isOpen, hasConnectedTaskSource, ...): FeatureWallCompletionState
- useContextualTour(id: ContextualTourId, enabled: boolean, source?: string): void
- performContextualTourStepAction(args): void
- useFeatureWallTourTelemetry(args): { markExitAction(...) }
- track('feature_wall_opened' | 'feature_wall_closed' | 'feature_wall_group_selected' | 'feature_wall_feature_selected' | 'orca_cli_feature_tip_shown' | 'orca_cli_feature_tip_setup_clicked' | 'orca_cli_feature_tip_setup_result')
- window.api.cli.install(): Promise<CliInstallStatus>
- window.api.shell.openUrl(url: string)
- window.api.ui.writeClipboardText(text: string)
- localStorage keys: orca_feature_wall_visited_workflows, orca_feature_wall_completed_workflows, orca_feature_wall_visited_agent_steps, orca_feature_wall_completed_agent_steps, orca_feature_wall_visited_workbench_steps, orca_feature_wall_completed_workbench_steps, orca_feature_wall_visited_review_steps, orca_feature_wall_completed_review_steps

**External dependencies**
- @radix-ui/dialog (Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter)
- lucide-react (Loader2, Mic, ArrowLeft, ArrowRight, X icons)
- sonner (toast notifications)
- react (hooks: useEffect, useCallback, useLayoutEffect, useMemo, useRef, useState, useId, createPortal)
- @/store (useAppStore Zustand)
- @/lib/telemetry (track function)
- @/components/ui/button (Button)
- @/components/ui/dialog (Dialog suite)
- @/lib/utils (cn utility)
- @/hooks/useShortcutLabel (formatShortcutLabel)
- @/hooks/usePrefersReducedMotion (prefers-reduced-motion detection)

**Persistence**
- localStorage: orca_feature_wall_visited_{workflows,agent_steps,workbench_steps,review_steps}
- localStorage: orca_feature_wall_completed_{workflows,agent_steps,workbench_steps,review_steps}
- Zustand store: activeModal='feature-wall'|'feature-tips'|'setup-guide', activeContextualTourId, activeContextualTourStepIndex, featureTipsSeenIds, contextualToursSeenIds, featureInteractions, modalData
- Session: telemetry state (openedAtMs, exitAction) in useRef, measureVersion polling intervals

**Cross-platform concerns**
- Platform-agnostic except: shell.openUrl() for docs links (browser platform)
- Keyboard bindings loaded from Zustand store (keybindings prop), supports both macOS (Cmd) and Linux/Windows (Ctrl) via getShortcutPlatform()
- Prefers-reduced-motion media query respected for animated visuals (GIF playback, waveform animations)
- Handles hardened browser contexts where localStorage may be unavailable

### `ui-lib-hooks (Orca Renderer)`

Pure client-side utilities and React hooks for Orca's IDE renderer. Provides agent status detection, terminal interaction, UI state management, workspace session handling, browser/editor integration, file path normalization, caching, scroll anchoring, keyboard shortcuts, theme management, and metadata fetching—all decoupled from DOM/component rendering.

**Rust portability:** tier=`pure` · effort=`M` · target=`orca-lib (new pure shared lib crate: agent-detection, path-utils, agent-status-types, metadata-caching)`  
_Core agent status detection, path manipulation, and caching logic are pure TypeScript with no I/O or DOM. IPC event handling and React hooks are framework-specific and must be reimplemented in Swift/SwiftUI for macOS shell. Metadata caching can port to a simple Rust cache (Arc<DashMap> with TTL cleanup). Platform-specific shortcuts, theme resolution, and file icon mapping depend on OS APIs (Cocoa, Win32) but the pure logic (language → icon table, keybinding format rules) is portable. External IPC channels (window.api.*) will become async Rust message dispatch or Swift interop calls; this layer is the translation boundary._

**Capabilities**
- Detect and classify agent activity from terminal titles (Claude Code, Gemini, Codex, Cursor, Pi, etc.) using pattern matching and state tracking
- Normalize file paths across Windows/POSIX systems and compute relative paths within worktree roots
- Manage workspace sessions via IPC: tab switching, worktree activation, terminal creation, browser operations
- Track unread tab/worktree state and compute badge counts for dock notifications
- Cache metadata (GitHub/GitLab/Linear issues, repo labels, team members) with TTL-based expiry and deduplication of in-flight requests
- Preserve virtualized scroll position by row identity, not pixels, across remeasures and row reordering
- Format keyboard shortcuts per platform (macOS/Windows/Linux) from keybinding definitions
- Resolve document theme preference (dark/light/system) and apply CSS classes with transition suppression
- Detect programming language from file extension for syntax highlighting
- Map file extensions/names to semantic icons via lucide-react library
- Manage composer state for new workspace creation: repo selection, issue linking, setup policies, agent configuration
- Handle IPC events for application-wide concerns: zoom level, updater status, keyboard input, SSH connectivity, rate limits
- Watch external file changes and reload notifications (editor, non-editor targets)
- Render display labels for repos by path with collision resolution
- Track installed agent skills and refresh discovery on changes
- Orchestrate floating terminal and markdown tab creation/focus
- Handle multi-selection paste from primary selection and file drops into editor
- Detect and label supported agent types with fallback to neutral glyph for unknown agents
- Coalesce multiple working agents per worktree and suppress inherited terminal status
- Track agent hook completion notifications with debouncing and coordinator lifecycle

**Public API / IPC / RPC**
- detectAgentStatusFromTitle(title: string) → AgentStatus | null
- clearWorkingIndicators(title: string) → string
- createAgentStatusTracker(onBecameIdle: () => void) → { handleTitle, reset }
- normalizeTerminalTitle(title: string) → string
- getAgentLabel(title: string) → string | null
- isClaudeAgent(title: string) → boolean
- formatAgentTypeLabel(agentType) → string
- agentTypeToIconAgent(agentType) → TuiAgent | null
- getWorkingAgentsPerWorktree(args) → Record<string, WorktreeAgents>
- getUnreadBadgeCount(args) → number
- hasActiveWorkspaceActivity(worktreeId, tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree) → boolean
- isInactiveWorkspace(worktreeId, tabsByWorktree, ptyIdsByTabId, browserTabsByWorktree) → boolean
- normalizeRelativePath(path) → string
- getRelativePathInsideRoot(filePath, rootPath) → string | null
- basename(path) → string
- dirname(path) → string
- joinPath(basePath, relativePath) → string
- getFileTypeIcon(path) → LucideIcon
- detectLanguage(filePath) → string
- extractIpcErrorMessage(error, fallback) → string
- useIpcEvents() → void
- useComposerState(options) → UseComposerStateResult
- useVirtualizedScrollAnchor({...options}) → void
- useShortcutLabel(actionId) → string
- useShortcutKeys(actionId) → string[]
- useShortcutKeyCombos(actionId) → string[][]
- usePrefersReducedMotion() → boolean
- useMountedRef() → MutableRefObject<boolean>
- useImmediateMutation() → { isPending, run }
- useGlobalFileDrop() → void
- useEditorExternalWatch() → void
- useAutoAckViewedAgent() → void
- useAutomationDispatchEvents() → void
- useUnreadDockBadge() → typeof clearUnreadDockBadgeCount
- useInstalledAgentSkill(agentType) → InstalledAgentSkillState
- useDetectedAgents(args) → UseDetectedAgentsResult
- usePrimarySelectionPaste(enabled) → void
- useRadixBodyPointerEventsRecovery() → void
- useSidebarResize<T extends HTMLElement>(options) → ResizeState
- useSettingsNavigationMetadata() → SettingsNavSection[]
- useGitHubSlugMetadata(args) → MetadataState
- useIssueMetadata(args) → MetadataState
- useRepoAssignees(args) → MetadataState
- useRepoLabels(args) → MetadataState
- useTeamMembers(args) → MetadataState
- useTeamStates(args) → MetadataState
- createMetadataRequestStore<T>() → MetadataRequestStore<T>
- loadMetadata<T>(store, key, fetcher) → Promise<T>
- getFreshMetadata<T>(store, key, now) → CachedMetadata<T> | null
- clearMetadataRequestStore<T>(store) → void
- applyDocumentTheme(preference, options) → void
- resolveDocumentTheme(preference, matchMedia) → boolean
- shouldCancelVirtualizedScrollOffsetRestore(args) → boolean
- requestVirtualizedScrollAnchorRecord(scrollElementSelector) → void
- handleSwitchTab(direction) → boolean
- handleSwitchRecentTab() → boolean
- handleSwitchTerminalTab(direction) → boolean
- handleSwitchTabAcrossAllTypes(direction) → boolean
- getRepoDisplayLabelsByPath(items) → Map<string, string>
- TOGGLE_FLOATING_TERMINAL_EVENT: 'orca-toggle-floating-terminal'
- ORCHESTRATION_SETUP_STATE_EVENT: 'orca:orchestration-setup-state'
- VIRTUALIZED_SCROLL_ANCHOR_RECORD_EVENT: 'orca-record-virtualized-scroll-anchor'

**External dependencies**
- react
- zustand
- sonner (toast notifications)
- monaco-editor (language detection)
- lucide-react (file type icons)
- clsx (className merging)
- tailwind-merge (CSS class utilities)
- node:path (standard library path utilities)
- node:fs (standard library file system—implicit via runtime RPC)
- node:url (standard library URL parsing)
- node:perf_hooks (performance measurement)

**Persistence**
- Zustand app store (settings, keybindings, worktrees, terminals, tabs, browser state)
- In-memory cache via MetadataRequestStore (GitHub/GitLab/Linear metadata, TTL 5 min)
- Window event listeners (resize, matchMedia, file drop, keyboard shortcuts)
- DOM classList mutations (theme dark/light classes, transition-suppression)

**Cross-platform concerns**
- Path normalization handles Windows (backslash, UNC paths, drive letters) and POSIX (/)
- Case-insensitive path comparison on Windows volumes (C:/, //) vs case-sensitive on POSIX
- Keyboard shortcut formatting per platform: macOS (⌘ Cmd), Windows/Linux (Ctrl)
- System theme detection via matchMedia('(prefers-reduced-motion: reduce)') and ('(prefers-color-scheme)')
- File drop handling routes to remote runtime (SSH, cloud) or local workspace
- SSH connectivity state, port forwarding, credential prompts (node:ssh2 binding via main IPC)

### `ui-misc (assorted UI components)`

Collection of specialized UI components for stats tracking, activity feed, dashboard agent display, mobile pairing, customizable pet overlay, workspace cleanup, command palette actions, voice dictation, agent selection, skills discovery, sparse checkout, crash reporting, repository selection, and network port scanning. Handles diverse user-facing features beyond the core editor/terminal layout.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-ui-misc (new crate in workspace; depends on orca-shared for types, orca-renderer for UI framework)`  
_Largest and most diverse subsystem. Heavy React + shadcn UI dependency means full rewrite in SwiftUI (macOS) or equivalent native framework. Requires: (1) UI framework port (all dialog/popover/combobox/tooltip/card components), (2) IPC endpoint stubs for window.api.speech/mobile/crashReports/shell/skills/pet/workspacePorts (replace with native APIs or Rust service calls), (3) animation system for pet sprite stepping (CoreAnimation on macOS, or canvas-based for cross-platform), (4) QR code generation library binding, (5) clipboard write APIs (native OS), (6) crash report formatting shared module (pure Rust), (7) workspace cleanup logic (mostly pure with fs/git IO), (8) dictation UI binding to OS speech recognition. Activity feed and dashboard rendering are performance-critical and require efficient tree/list virtualization. Pet overlay needs careful animation frame scheduling. Recommend staged porting: crash-report first (mostly type translation), then workspace-cleanup (pure logic), then modular UI pieces (stats, skills, repo-combobox as reusable form inputs), finally complex surfaces (activity feed, mobile pairing, dictation with live state)._

**Capabilities**
- Display API usage analytics (Claude/Codex/OpenCode) with daily charts and summaries
- Track agent spawn counts, execution time, and PR creation metrics
- Activity feed with per-agent pane threads grouped by status/project/worktree/agent type; unread tracking and mark-unread
- Live agent state indicators (working/blocked/waiting/done/interrupted) with freshness-aware staleness decay
- Dashboard agent row display with agent state dots, prompts, assistant responses, tool steps, child disclosure
- Mobile device pairing UI with QR code generation for iOS/Android
- Customizable pet overlay that responds to agent states with sprite-based animations (idle/working/sleeping animations)
- Workspace cleanup dialog with multi-tier safety classification (ready/review/protected), git status checks, context blocker detection, and bulk deletion with confirmation
- Cmd+J quick actions (new browser tab, markdown file, terminal, workspace, workspace delete, quick command)
- Voice dictation with audio capture, partial/final transcript display, insertion target tracking, stopped-session management
- Agent selector/combobox with search, favorites, default preference persistence, context menus
- Skills discovery page with filtering by provider/source, install status badges, file reveal, pagination
- Sparse checkout preset editor (CRUD for directory presets per repo)
- Crash report dialog with privacy-safe diagnostic text formatting, optional notes, anonymous/authenticated submission
- Repository combobox with git-specific filtering and remote indicators
- Workspace port scanner with periodic polling, advertised URL change detection, platform-specific availability

**Public API / IPC / RPC**
- StatsPane (export)
- STATS_PANE_SEARCH_ENTRIES (export)
- ActivityPrototypePage (default export; buildActivityEvents, buildAgentPaneThreads, groupActivityThreadsByStatus, getActivityThreadGroup, activityThreadResponseRenderPreview, activityThreadMatchesSearchQuery)
- DashboardAgentRow (component props: agent, onDismiss, onActivate, now, isUnvisited, dotSize)
- DashboardAgentChildDisclosure, DashboardAgentRowMessage, DashboardAgentRowToolStep (components)
- MobilePage (default export; HeroFlow, HeroIntro, HeroPaired, PhoneCarousel)
- PetOverlay (component with sprite animation and pet-state-driven animations)
- WorkspaceCleanupDialog (default export)
- CMD_J_QUICK_ACTIONS (array of quick actions with id, title, description, icon, verbKeywords, isAvailable, run)
- DictationController (component; exposes via window.api.speech IPC)
- SkillsPage (default export)
- RepoCombobox (component; searchRepos utility)
- AgentCombobox (component with combobox/context-menu integration)
- WorkspacePortScanner (component, returns null)
- SparseCheckoutPresetSelect (component)
- CrashReportDialog (component)

**External dependencies**
- react (hooks: useState, useEffect, useCallback, useMemo, useLayoutEffect, useRef, useId, useDeferredValue, useShallow)
- lucide-react (icon components: BarChart3, Bot, Clock, GitPullRequest, Bell, Search, etc.)
- qrcode (QRCodeBrowser for mobile pairing)
- sonner (toast notifications)
- zustand (useAppStore via react/shallow)
- shadcn-ui components (Button, Input, Dialog, Select, Dropdown, Popover, Tooltip, Card, Scroll Area, Badge, Command, ContextMenu)
- window.api IPC channels: speech.startDictation, speech.stopDictation, speech.onPartialTranscript, speech.onFinalTranscript, speech.onStopped, speech.onError, ui.writeClipboardText, ui.writeClipboardImage, ui.onDictationKeyDown, ui.onOpenCrashReport, shell.openUrl, shell.openInFileManager, mobile.listDevices, mobile.listNetworkInterfaces, mobile.getPairingQR, mobile.revokeDevice, crashReports.getLatestReport, crashReports.getLatestPending, crashReports.dismiss, crashReports.submit, crashReports.copyLatestDiagnostics, skills.discover, pet.read, workspacePorts.onAdvertisedUrlChanged, gh.viewer

**Persistence**
- useAppStore selectors (statsSummary, fetchStatsSummary, agentStatusByPaneKey, retainedAgentsByPaneKey, tabsByWorktree, worktreeMap, repoMap, acknowledgedAgentsByPaneKey, acknowledgeAgents, unacknowledgeAgents, agentStatusEpoch)
- useDashboardData hook (useDashboardData, useNow, useRetainedAgents)
- workspaceCleanupScan, workspaceCleanupLoading, workspaceCleanupError store slices
- scanWorkspaceCleanup, markCandidateViewed, dismissCandidates, resetDismissals, removeWorkspaceCleanup store actions
- dictationState, setDictationState, setPartialTranscript store updates
- acknowledged agent pane state persisted per paneKey
- crash reports on-disk (via window.api.crashReports.getLatestReport)
- sparse presets per-repo (via sparsePresetsByRepo, saveSparsePreset store)
- mobile pairing device list (window.api.mobile.listDevices)
- activity feed: unread thread state (via acknowledgedAgentsByPaneKey store)

**Cross-platform concerns**
- QR code generation (mobile pairing, platform-agnostic)
- Crash report text formatting with redaction/truncation (shared across platforms)
- Voice dictation relies on window.api.speech (macOS/Linux speech recognition backend)
- Mobile pairing works across iOS/Android via QR
- Workspace port scanning platform-aware (reports platform in result)
- Sparse checkout UI platform-agnostic (git feature)
- Workspace cleanup scans remote worktrees (SSH provider availability detection)
- Pet sprite animation CSS-based, works cross-platform
- All IPC calls bridge Electron main/renderer

### `ui-onboarding (Orca Electron IDE)`

Orchestrates the first-run onboarding experience for Orca: stepping through agent selection, theme configuration, notification permissions, and GitHub CLI integration. Also manages the setup-guide modal for feature wall progression tracking, and smart workspace naming with cross-repository GitHub/GitLab/Linear task source integration.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca_ui_onboarding (or orca_ui_core for shared modal infrastructure)`  
_The onboarding/setup-guide subsystem is almost entirely React UI (JSX rendering, state management, event handlers). Porting requires: (1) rewriting all React/JSX components to native macOS SwiftUI equivalents, (2) replacing zustand store reads with Rust state bridging via IPC, (3) rewriting multi-step wizard state machine (useOnboardingFlow) as a native struct/enum, (4) porting telemetry tracking to Rust backend, (5) replacing Radix/cmdk UI components with native macOS controls (NSAlert, NSSplitViewController, NSTableView for source picker). The SmartWorkspaceNameField is particularly complex: search debouncing, deferred query results, async GitHub/GitLab API polling, cross-repo URL detection. Core orchestration logic (step navigation, persistence, skip confirmation) is portable; all view/styling is platform-specific. Consider extracting the state machine (useOnboardingFlow logic) into a pure-state Rust struct first, leaving UI as a shim. Persistence layer (settings hydration, step tracking) will map cleanly to Rust serde structs; IPC layer already exists (window.api calls) so bridging is straightforward._

**Capabilities**
- Multi-step onboarding modal (4 core steps: agent pick, theme, integrations, notifications)
- Agent auto-detection and selection with collapsed disclosure for 40+ registered agents
- Theme preview and application with persistent settings hydration
- Notification permission prompts and sound testing via macOS/system APIs
- GitHub CLI installation status checking and integration setup
- Repository folder picker with local folder support
- Git clone with URL entry and server/local path handling
- Nested repository scanning with multi-select import workflow
- Telemetry event tracking (onboarding_started, onboarding_step_viewed, onboarding_step_completed, onboarding_agent_picked, onboarding_task_sources_snapshot)
- Step persistence and resume-from state across sessions
- Skip confirmation dialog with ESC and skip-button dismissal
- Settings hydration to avoid overwriting user preferences
- Setup-guide modal displaying feature wall setup steps with progress ring
- SmartWorkspaceNameField with multi-mode search: GitHub PR/issue, GitLab MR/issue, Linear issues, git branches, plain text naming
- Cross-repo GitHub URL detection and switch-repo prompts
- MR state filtering (Open/Merged/Closed/All) for GitLab tab
- Keyboard navigation (Enter to advance, Escape to skip, Tab to cycle modes)
- Auto-save on theme tile selection; skip reverts to entry theme
- Folder vs. git repo detection with non-git folder scanning
- Runtime environment path handling (SSH runtime vs. local paths)

**Public API / IPC / RPC**
- OnboardingFlow (default export, React component)
- useOnboardingFlow (hook returning OnboardingFlowController)
- shouldShowOnboarding(onboarding: OnboardingState | null): boolean
- showOnboardingFromRenderer(): Promise<void>
- onOnboardingReopened(callback: (state: OnboardingState) => void): () => void
- SetupGuideModal (default export)
- SmartWorkspaceNameField (default export)
- remapOpenOnboardingLastCompletedStep(snapshot: OnboardingProgressSnapshot): number
- prepareSkippedOnboardingPreferences(options): Promise<boolean>
- STEPS constant (array of step definitions)
- window.api.onboarding.update({...}): Promise<OnboardingState>
- window.api.repos.pickFolder(): Promise<string | null>
- window.api.repos.add({path, kind}): Promise<{repo: Repo} | {error: string}>
- window.api.repos.clone({url, destination}): Promise<Repo>
- window.api.notifications.{getPermissionStatus, requestPermission, openSystemSettings, playSound}
- window.api.settings.previewGhosttyImport()
- window.api.shell.openUrl(url)
- window.api.gh.workItem, workItemByOwnerRepo, repoSlug, listIssues
- window.api.gl.workItemByPath, listMRs
- window.api.cli.{getInstallStatus, install}
- callRuntimeRpc(target, 'repo.clone', {...}, {timeoutMs}): Promise<{repo: Repo}>

**External dependencies**
- react (useState, useEffect, useRef, useCallback, useMemo, useShallow)
- react-dom/server (renderToStaticMarkup, flushSync)
- lucide-react (icons: Check, ExternalLink, GitBranch, Github, Gitlab, etc.)
- sonner (toast notifications)
- zustand (useAppStore, shallow selectors)
- vitest (testing framework)
- node:path, node:fs (test file access)
- radix-ui (Tooltip, Dialog, Popover, Tabs, Command, Button components)
- cmdk (Command/CommandList/CommandGroup/CommandItem for search UI)

**Persistence**
- OnboardingState in global store (flowVersion, lastCompletedStep, outcome, closedAt, checklist)
- GlobalSettings in persistent store (theme, defaultTuiAgent, activeRuntimeEnvironmentId, workspaceDir, notificationsMuted, customSoundPath)
- Setup guide sidebar dismissed state (setSetupGuideSidebarDismissed)
- Local component state refs: themeBeforePreview, nestedScanId, selectedSourceKey (memory-only during session)

**Cross-platform concerns**
- macOS: notification permission prompts and system settings routing, Ghostty config preview
- SSH runtime environment path handling (vs. local file picker paths)
- Git repository detection across Windows/macOS/Linux (via shared isGitRepoKind)
- System shell PATH scanning for CLI tool presence (agents, gh, glab)
- Keyboard modifiers: Cmd (macOS) vs. Ctrl (Windows/Linux) via getScreenSubmitModifierLabel
- Cross-platform path normalization (getRuntimePathBasename for display)

### `ui-right-sidebar`

Right sidebar UI panel system hosting file explorer, search, version control (git/hosted reviews), checks (CI/CD), and port forwarding. Routes worktree/repo context to tab-based panels with dockable activity bar, draggable width resize, and coordinated state management.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-sidebar (new, thin-wrapper for SwiftUI + alacritty) + orca-git (port git operations)`  
_DOM/React UI must rebuild as SwiftUI (on macOS shell) + possible web view (terminal). Core logic for tree traversal, search, git operations, port management can be shared via Rust RPC. File watcher integration (@tanstack/react-virtual virtualization, tree state) needs migration to native file observer. SSH port forwarding APIs (window.api.ssh.*) must integrate with native tunnel impl. Significant rewrite: React hooks -> SwiftUI view states, Zustand store subscription -> native app state binding, Dialog/Dropdown/Popover radix primitives -> SwiftUI Sheet/Menu/Popover. Git operations already RPC'd, can decouple to Rust backend service._

**Capabilities**
- Tab-based panel switching (Explorer/Search/Source Control/Checks/Ports)
- File tree traversal with virtual scrolling, expand/collapse, drag-drop, inline rename/delete
- Full-text search across files with regex, case-sensitive, whole-word, include/exclude patterns
- Git status tree (staged/unstaged/untracked), bulk stage/discard, conflict resolution prompts
- Commit drafts per-worktree, AI commit message generation, branch compare/history visualization
- Pull request/merge request viewing, CI check status, check details modal, merge conflict detection
- Port forwarding UI (SSH) with add/edit/remove, detected ports, browser URLs
- Activity bar layout (top or side orientation) with overflow menu, status indicators, keyboard shortcuts
- Sidebar width resize (drag-to-resize left edge), max width scales with window, persists to store
- Diff comment viewing/clearing, confirmation dialogs, copy-to-clipboard feedback

**Public API / IPC / RPC**
- RightSidebar (default export, main component)
- FileExplorer (default export)
- SourceControl (default export)
- ChecksPanel (default export)
- Search (default export)
- PortsPanel (export)
- ActivityBarButton, TopActivityOverflowMenu (activity bar components)
- ChecksPanelReviewHeader, ConflictingFilesSection, ChecksList, PRCommentsList (checks sub-components)
- FileExplorerRow, FileExplorerToolbar, FileExplorerBackgroundMenu (explorer sub-components)
- SearchHeader, SearchFilters, FileResultRow, MatchResultRow (search sub-components)
- GitHistoryPanel (git history visualization)
- SourceControlDiscardDialog, CreatePullRequestDialog (modal dialogs)
- HostedReviewHeaderLink, HostedReviewActions (PR/MR link management)
- CommitArea, BulkActionBar (source control sub-components)
- buildFixCommitFailurePrompt, buildResolveConflictsPrompt, buildResolvePullRequestConflictsPrompt (prompt generation)
- getTopActivityBarLayout (activity bar layout calculation)
- getActiveChecksStatus, getLocalWorkspacePortSections (status queries)

**External dependencies**
- react (UI framework)
- @tanstack/react-virtual (virtual scrolling)
- zustand (useAppStore state)
- sonner (toast notifications)
- lucide-react (icon library)
- radix-ui (dropdown, tooltip, dialog, popover, context-menu components)
- rpc/IPC to main: callRuntimeRpc, getRuntimeGit*, getRuntimeRepo*, getRuntimeFile*
- window.api.ui (writeClipboardText, onFileDrop), window.api.shell (openPath, openUrl), window.api.ssh (addPortForward, removePortForward, updatePortForward)

**Persistence**
- useAppStore state: rightSidebarOpen, rightSidebarTab, rightSidebarWidth, activityBarPosition
- gitStatusByWorktree (git entries cache)
- gitBranchChangesByWorktree (branch diff cache)
- gitBranchCompareSummaryByWorktree (branch compare result)
- gitConflictOperationByWorktree (merge/rebase/cherry-pick state)
- remoteStatusesByWorktree (push/pull tracking)
- fileSearchStateByWorktree (search query, results, filters)
- expandedDirs, collapsedTreeDirs (file explorer tree state)
- prCache, hostedReviewCache (PR/MR fetch results by cacheKey)
- commitDrafts, commitErrors, remoteActionErrors (per-worktree session state)
- prGenerationRecords (AI PR field generation state)
- gitHistoryByWorktree (git history panel state)
- showDotfilesByWorktree (explorer visibility setting)

**Cross-platform concerns**
- Windows: separate titlebar layout with native controls, activity bar in sidebar body not titlebar
- macOS: activity bar in titlebar, drag regions calculated for window controls inset
- SSH/Remote: ports panel only visible for SSH-backed repos, different runtime targets
- Multi-platform keyboard shortcuts resolved via @/hooks/useShortcutLabel

### `ui-runtime-web`

Provides web/mobile browser client bridge to a paired remote Orca runtime via encrypted WebSocket RPC; implements end-to-end encrypted session synchronization, terminal/browser tab management, and preload API emulation for the Electron-like interface.

**Rust portability:** tier=`ui-native` · effort=`L` · target=`orca-web-runtime (new) + orca-shared (existing RPC envelope types)`  
_Core RPC transport (encryption, WebSocket framing, request routing) is pure I/O and feasible to port. Preload API is a compatibility shim that maps browser APIs to runtime RPC calls — must be rebuilt as native Swift/Cocoa equivalents in macOS shell. Session graph sync depends on React store (AppState) and terminal UI state introspection (PaneManager), requiring significant refactoring to extract into pure sync logic. E2EE via tweetnacl is portable. Main challenges: (1) Replace browser LocalStorage with native secure keychain/preferences, (2) Rebuild window.api.* facades as Swift/Cocoa IPC bridges, (3) Decouple React store mutations from terminal sync logic, (4) Terminal appearance negotiation currently relies on CSS media queries — may need native theme detection. Suggests phased port: phase 1 = WebSocket E2EE transport + RPC client, phase 2 = native preferences/keychain bridge, phase 3 = terminal session graph export API._

**Capabilities**
- End-to-end encrypted WebSocket connection with NaCl box encryption (tweetnacl)
- JSON-RPC request/response + streaming subscription multiplexing over WebSocket
- Remote terminal (PTY) session management and display mirroring
- Remote browser tab (Webview) creation and management
- Session state synchronization (tabs, worktrees, layout graph)
- Preload API surface emulation: settings, keybindings, runtime calls, GitHub/GitLab RPC proxying, file operations, git operations
- Pairing code parsing and WebSocket endpoint extraction
- Device-pairing authentication via shared encryption key
- Automatic reconnection with exponential backoff (500ms-15s)
- Browser clipboard image upload/PNG conversion
- Local storage persistence of paired environment config and workspace state
- Terminal stream subscription multiplexing (shared connection for files.watch, dedicated per subscription)
- Mobile session tab mirroring and parity sync from host

**Public API / IPC / RPC**
- WebRuntimeClient class: constructor(pairing: WebPairingOffer), call(method, params?, options?): Promise<RuntimeRpcResponse>, subscribe(method, params, callbacks, options?): Promise<WebRuntimeSubscriptionHandle>, close(options?): void
- createWebRuntimeSessionTerminal(args): Promise<boolean>
- createWebRuntimeSessionBrowserTab(args): Promise<boolean>
- activateWebRuntimeSessionWorktree(args): Promise<boolean>
- activateWebRuntimeSessionTab(args): Promise<boolean>
- closeWebRuntimeSessionTab(args): Promise<boolean>
- moveWebRuntimeSessionTab(args): Promise<boolean>
- splitWebRuntimeTerminal(ptyId, direction, source): boolean
- closeWebRuntimeTerminal(ptyId): boolean
- isWebRuntimeSessionActive(environmentId): boolean
- installWebPreloadApi(): void
- getActiveRuntimeTarget(settings): RuntimeClientTarget
- callRuntimeRpc<T>(target, method, params, options): Promise<T>
- window.api.runtime, window.api.runtimeEnvironments, window.api.repos, window.api.worktrees, window.api.fs, window.api.git, window.api.browser, window.api.gh, window.api.gl, window.api.settings, window.api.keybindings, window.api.session, window.api.hooks, window.api.pty, window.api.ssh
- WebSocket message format: {type: 'e2ee_hello'}, {type: 'e2ee_ready'}, {type: 'e2ee_authenticated'}, {type: 'e2ee_error'}, encrypted JSON-RPC with {id, deviceToken, method, params}

**External dependencies**
- tweetnacl (^1.0.3): NaCl box encryption/decryption for E2EE
- Browser WebSocket API (native)
- Browser localStorage API (native)
- Browser Clipboard API (native)
- Browser File/Canvas APIs (ImageBitmap, canvas.toBlob for clipboard PNG conversion)
- Browser URLSearchParams/URL parsing (native)
- Base64 encoding/decoding via window.atob/btoa (native)

**Persistence**
- localStorage key 'orca.web.settings.v1': GlobalSettings JSON
- localStorage key 'orca.web.ui.v1': UI state snapshot
- localStorage key 'orca.web.workspaceSession.v1': WorkspaceSessionState (active repo/worktree, browser history, tab layout)
- localStorage key 'orca.web.onboarding.v1': OnboardingState checklist
- localStorage key 'orca.web.githubCache.v1': GitHub PR/issue cache
- localStorage key 'orca.web.keybindings.v1': KeybindingOverrides by platform (darwin/linux/win32)
- In-memory: WebRuntimeClient connection state, active environment config, compatibility check cache (max 32 entries)

**Cross-platform concerns**
- Browser platform detection via window.navigator.platform for keybinding platform resolution (darwin/linux/win32)
- WebSocket HTTPS/HTTP check: rejects ws:// from HTTPS pages (mixed-content prevention)
- Browser-agnostic clipboard image handling: converts to PNG on all platforms
- Terminal appearance theming checks system prefers-dark CSS media query

### `ui-scm-views`

GitHub/GitLab Projects and PRs/Issues filtering UI, plus diff-level inline note editing. Provides GitHub Projects table views (pinned/recent/browse picker, field editors, grouping/sorting, column management), PR/issue filter dropdowns, and Monaco editor decorations for inline diff comments with React-backed card rendering.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-ui-github`  
_Projects view is a complex React+Monaco composition with dynamic column layout (CSS grid, frozen columns via translateX), transient state (sort/search overrides), grouping/sorting logic reused from shared crate, cell editors (custom fields, assignees, labels, iterations), and diff-comment Monaco integration. Requires: (1) SwiftUI table/list UI with column visibility/resizing (consider AppKit NSTableView wrapper if SwiftUI insufficient), (2) TipTap-equivalent rich markdown editor (SwiftUI + WebKit or native text editing), (3) alacritty_terminal integration for Monaco diff-view replacement, (4) persistent column preferences (UserDefaults), (5) IPC marshaling for Github.project.* and github.issue.* RPC methods, (6) rate-limit display widget. The grouping/sorting logic (shared/github-project-group-sort) ports cleanly to pure Rust. Cell rendering (type glyphs, assignee/label pickers, custom field renders) needs careful SwiftUI translation per field kind. Estimated: Projects table core (M), editors (M), diff-comments (M), picker/filters (S), rate-limit (S)._

**Capabilities**
- GitHub Projects table rendering with column visibility/width persistence (localStorage)
- Project picker with pinned/recent/browse-all discovery, paste-to-add URL/shorthand resolution
- View switching and transient search filter overrides (per-project, not persisted)
- Grouping and sorting with local sort-override (column header clicks)
- Inline field editors for single-select, iteration, date, number custom fields
- Assignee/label multi-select editors with optional issue-type toggle
- Frozen first-two columns with CSS scroll offset tracking
- Project row filtering by open repos (via slug index lookup)
- Dialog launching for project rows (repo-backed full dialog or slug-only simplified surface)
- PR/Issue filter dropdowns: author, assignee, reviewer, labels, state (open/closed/merged), draft flag
- Issue-source selector (upstream vs origin remote preference, per-repo)
- GitHub rate-limit display with bucket breakdown (REST/Search/GraphQL)
- Markdown composer with TipTap rich editor integration
- Diff-level inline notes: create, edit, delete with Monaco view-zone integration
- Diff comment card rendering inside Monaco view zones via React roots
- Scroll-to-comment coordination and mouse-event suppression for interactive zones

**Public API / IPC / RPC**
- ProjectViewWrapper (default export)
- ProjectPicker (exported, type ResolvedProjectSelection)
- ProjectViewList (default export)
- useDiffCommentDecorator (hook for mounting comment zones)
- DiffCommentCard (exported)
- DiffCommentPopover (exported)
- GitHubMarkdownComposer (exported)
- GitHubRateLimitPanel (exported)
- useGitHubRateLimitSnapshot (hook)
- IssueSourceSelector (default export)
- PRFilterSections (exported type SectionKey, PRFilterChange)
- PRFilterDropdowns (exported)
- PRFilterPickers (SingleSelectList, MultiSelectList, PickerOption type)
- IssueSourceIndicator (exported, sameGitHubOwnerRepo utility)
- gitlab-rate-limit-display (parallel to GitHub version)
- columns.ts: getAvailableColumns, loadHiddenColumns, saveHiddenColumns, TYPE_FIELD, TYPE_FIELD_ID
- window.api.gh.* IPC methods (listProjectViews, listAccessibleProjects, resolveProjectRef, projectWorkItemDetailsBySlug, rateLimit, patchProjectIssueOrPr, etc)
- callRuntimeRpc<> for environment targets (github.project.listViews, github.project.listAccessible, github.project.resolveRef, github.rateLimit)

**External dependencies**
- react (hooks: useState, useCallback, useEffect, useRef, useMemo, useLayoutEffect, createRoot)
- @tiptap/react (EditorContent, useEditor, Placeholder extension)
- monaco-editor (editor namespace, ICodeEditor, IViewZone, decorations)
- lucide-react (icons: ExternalLink, RefreshCw, KanbanSquare, Search, Table, X, ArrowDown, ArrowUp, ArrowUpDown, Columns3, Loader, Pin, ChevronDown, ChevronRight, Play, Pencil, Trash, FileText, CircleDot, GitPullRequest, Lock, Plus, AlertTriangle, Gauge, ImageIcon, CornerDownLeft)
- sonner (toast library)
- @radix-ui/tooltip (Tooltip, TooltipContent, TooltipTrigger)
- @radix-ui/popover (Popover, PopoverContent, PopoverTrigger)
- @radix-ui/hover-card (HoverCard, HoverCardContent, HoverCardTrigger)
- @radix-ui/dialog (Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle)
- @radix-ui/sheet (Sheet, SheetContent, SheetDescription, SheetTitle)
- @radix-ui/visually-hidden (VisuallyHidden)
- @radix-ui/command (Command, CommandEmpty, CommandInput, CommandItem, CommandList)
- window.api.* (shell.openUrl, app.reload, gh namespace IPC)
- window.localStorage (for column visibility persistence)

**Persistence**
- localStorage key 'orca.githubProject.hiddenColumns' (JSON map of scopeKey -> string[] of hidden field ids)
- localStorage key 'orca.githubProject.columnWidths' (JSON map of scopeKey -> Record<fieldId, width px>)
- Zustand app store: settings.githubProjects (pinned, recent, lastViewByProject, activeProject)
- Zustand app store: projectViewCache (multi-level keyed by projectViewCacheKey)
- Zustand app store: repos (for slug index lookups)

**Cross-platform concerns**
- macOS: Cmd+F to focus project search input (navigator.userAgent 'Mac' check)
- Linux/Windows: Ctrl+F to focus project search input
- Electron IPC via window.api.* (all platforms)
- Runtime target selection (environment vs local, getActiveRuntimeTarget)
- Monaco editor is cross-platform but view-zone onDomNodeTop callback timing is browser-specific

### `ui-settings`

Settings panel UI for configuring global Orca application preferences, agent capabilities, integrations, and per-repository hooks. Provides 25+ categorized panes (Agents, Accounts, Terminal, Git, Voice, SSH, etc.) with search-driven discovery, lazy pane loading, and validation of user inputs.

**Rust portability:** tier=`ui-native` · effort=`L` · target=`orca-settings-ui (new crate wrapping SwiftUI panels + custom Tauri/slint forms); Rust backend: orca-settings-core for validation, merge logic, and IPC handlers`  
_Bulk of work: rebuild 25+ pane forms in SwiftUI (macOS) or alternative native framework; reuse validation/persistence logic from backend. Search and navigation logic (tabs, groups, lazy loading) is pure Rust—straightforward port. IPC endpoints (window.api.* calls) map to Tauri commands. Risk: Form autocompletes (fonts, agents, themes) require font enumeration and agent catalog—already available in existing Rust modules. Shortcuts pane's keybinding recording and conflict detection are testable Rust utilities already extracted. Platform-specific UI (macOS permissions, WSL dialogs) requires native host integration via Tauri, SwiftUI bindings._

**Capabilities**
- 25 settings panes (Agents, Accounts, Orchestration, Computer Use, Voice, Setup Guide, General, Integrations, Git/Commit AI, Tasks, Terminal, Quick Commands, Browser, Floating Workspace, Appearance, Input, Notifications, Shortcuts, Stats, Remote Servers, SSH, Mobile, Developer Permissions, Privacy, Advanced, Experimental, per-Repository)
- Multi-level navigation with 8 groups (AI Capabilities, Set Up, Workflows, Interface, Remote Access, Privacy & Security, Advanced, Experimental)
- Full-text search across all settings with keyword matching and subsection targeting
- Lazy pane loading for performance—only active/searched sections render
- Unsaved change tracking with prompts for Git AI Author prompt edits
- Per-platform terminal shell selection (PowerShell, Git Bash, WSL distros, macOS)
- SSH host management with relay, jump host, proxy command support
- Voice dictation (STT) model download/installation and microphone permission requesting
- Theme switching with system-dark-light and per-component font/zoom controls
- Mobile device pairing with QR code generation
- Keybinding recording and terminal conflict detection
- Git branch naming auto-templates and commit message AI customization with draft persistence
- Task source selection (Linear, GitHub, GitLab, etc.)
- Browser home page and link routing configuration
- Floating workspace (global terminal, browser, markdown tabs)
- Agent enable/disable per TUI agent with command override input
- Account selection (Claude, Codex, Gemini, OpenAI, OpenCode) with per-runtime selection
- Integrations status dashboard (GitHub, GitLab, Linear, Bitbucket, Azure DevOps, Gitea)
- Repository hooks (YAML-based) inspection and per-repo settings
- MCP configuration editor per repository
- Developer permissions dashboard (macOS microphone, screen recording, etc.)
- Privacy/telemetry controls and diagnostic bundle generation
- Advanced compatibility settings (GPU acceleration toggle, terminal rendering, macOS key modifiers)
- Experimental features unlock (Shift-click reveal)
- Scrollback buffer preset/custom sizing
- Terminal theme import (Ghostty .config/ghostty/config)
- Auto-save delay and diff view preferences
- Minimap and markdown review notes toggles

**Public API / IPC / RPC**
- Settings (root component) exported via default
- AgentsPane, AccountsPane, OrchestrationPane, ComputerUsePane, VoicePane, SettingsSetupGuidePane, GeneralPane, IntegrationsPane, GitPane, CommitMessageAiPane, TasksPane, TerminalPane, QuickCommandsPane, BrowserPane, FloatingWorkspacePane, AppearancePane, InputPane, NotificationsPane, ShortcutsPane, StatsPane, RuntimeEnvironmentsPane, SshPane, MobileSettingsPane, DeveloperPermissionsPane, PrivacyPane, AdvancedPane, ExperimentalPane, RepositoryPane
- SettingsSection (wrapper for each pane) with id, title, description, searchEntries, badge, headerAction, forceVisible, isActive
- SearchableSetting (single filterable row) with keywords, title, description, forceVisible, id
- SettingsFormControls: SettingsRow, SettingsSwitchRow, SettingsSwitch, SettingsSegmentedControl, SettingsBadge, FontAutocomplete, SettingsSubsectionHeader
- matchesSettingsSearch(query, entries) for filter logic
- Exported search entry arrays: AGENTS_PANE_SEARCH_ENTRIES, ACCOUNTS_PANE_SEARCH_ENTRIES, etc. per pane
- window.api.settings.listFonts()
- window.api.shell.openUrl(url), window.api.shell.openInExternalEditor(path, cmd)
- window.api.mobile.listDevices(), getPairingQR(), revokeDevice(), listNetworkInterfaces()
- window.api.ssh.listTargets(), addTarget(), updateTarget(), testConnection()
- window.api.cli.getInstallStatus()
- useAppStore hooks: updateSettings, fetchSettings, closeSettingsPage, updateRepo, removeProject, switchRuntimeEnvironment, setSshTargetsMetadata, clearRemovedSshTargetState, recordFeatureInteraction, markFeatureTipsSeen, toggleStatusBarItem, etc.
- checkRuntimeHooks(runtimeTarget, repoId) RPC call
- getActiveRuntimeTarget(settings)
- useInstalledAgentSkill(skillName, options)
- useWindowsTerminalCapabilities(shouldLoad, requestWslCapabilities, ownerKey, runtimeTarget)

**External dependencies**
- react (hooks: useState, useEffect, useCallback, useMemo, useRef, useContext)
- react-dom (refs)
- sonner (toast notifications)
- lucide-react (icons)
- @radix-ui/primitive-icons (UI primitives)
- tailwindcss (className utilities via @/lib/utils)
- ipcRenderer (implicit via window.api.* methods)
- preload bridge window.api.*
- native APIs: window.navigator.userAgent, navigator.userAgentData?.platform, document.querySelector/addEventListener/getComputedStyle, CSS.escape, window.requestAnimationFrame, AbortSignal, localStorage (implicit in app store)

**Persistence**
- GlobalSettings type (store-managed, fetched on mount)
- settingsSearchQuery (transient state, cleared on exit)
- mountedsectionIds (Set per session, lazy-loaded panes)
- RepoHooksMap (per-repo hooks cache keyed by repoId, cleared on runtime change)
- Keybindings (fetched on mount, validated for conflicts)
- Draft state: scrollbackMode, fontSuggestions, hasUnsavedSourceControlAiPromptChanges (commit + branch prompts), ghostty (Ghostty import modal state)
- sshConnectionStates (synced from store via IPC listener)
- modelStates (speech model manifest + status)
- Per-pane draft input: RepoSettingsDraftInput for displayName/baseRef to handle IME composition mid-keystroke

**Cross-platform concerns**
- macOS-specific: isMacUserAgent checks for 'Mac' in navigator.userAgent; Darwin platform detection; SF Mono/Menlo font fallbacks; Option-key layout detection (US vs. non-US); microphonePermission request via window.api.developerPermissions.request; macOS permissions section shown only on Darwin
- Windows-specific: isWindowsUserAgent checks; PowerShell 7+, Git Bash, WSL distro probing; Windows Terminal capabilities API (wslAvailable, wslDistros, pwshAvailable); Cascadia Mono font fallbacks; Windows shell selection (powershell.exe vs wsl.exe); WSL distro CLI location setup
- SSH: SSH host add/edit form with proxy command, jump host, identity file, relay grace period; connection state polling
- Web-client: showDesktopOnlySettings flag hides Desktop-only panes (Voice, Browser, Notifications, Mobile, SSH, Developer Permissions, Advanced) when isWebClientLocation() true

### `ui-sidebar`

Left sidebar UI for Orca IDE showing workspaces (worktrees), projects (repos), filtering, drag-and-drop reordering, workspace kanban board, and sidebar controls. Implements virtualized list rendering, smart sorting, status/PR-based grouping, multi-select, inline renaming, and rich metadata display (agents, ports, git info).

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-sidebar-ui (native SwiftUI for macOS; would require reimplementing card layout, virtualization, drag-drop in native frameworks)`  
_Sidebar is deeply React/DOM-coupled: virtualized list rendering, Radix UI components, DOM-based drag detection (pointer events), CSS transitions, inline CSS properties. Requires complete native UI rewrite (SwiftUI for macOS). Core sorting/filtering/grouping logic is pure and portable (smart-sort.ts, visible-worktrees.ts, workspace-status.ts, worktree-list-groups.ts). State management (AppStore selectors) remains in Rust backend. Drag-and-drop reordering and workspace status board are heavy on DOM APIs (getBoundingClientRect, elementFromPoint, ResizeObserver). Virtualization strategy (overscan, gap, sticky headers) is specific to TanStack Virtual and would need native equivalent (NSTableView on macOS, or custom scroll handling). Agent/SSH status indicators, tooltips, context menus, modal dialogs all depend on Radix primitives. Rich card metadata (ports, agents, PR display, git info) requires data transformation helpers (pure, portable) plus native rendering. Estimated 3-4 week effort for feature parity: native card component, virtualization layer, drag/drop gesture handling, sidebar resize, search integration._

**Capabilities**
- Virtualized sidebar list rendering (TanStack React Virtual)
- Workspace/worktree card display with agent status, PR info, ports, git state
- Project/repo grouping with collapsible headers and manual/automatic ordering
- Smart sort (attention-based), recent activity, alphabetical, repo-based sorting
- Filtering by project, sleeping workspace visibility, default-branch hiding
- Workspace status board (kanban view) with drag-and-drop between status lanes and sidebar
- Multi-select with keyboard modifiers (Shift, Cmd/Ctrl)
- Inline worktree/project renaming with edit confirmation
- Worktree deletion flow with confirmation dialog (single/batch/lineage aware)
- Drag-and-drop reordering within sidebar and to workspace board
- Pointer-based autoscroll during drag operations
- SSH target/connection status indicators and reconnect prompts
- Project group creation/deletion/renaming (optional feature)
- Sidebar resize handle (fixed width constraint 220-500px)
- Setup guide sidebar entries and tooltips
- Search/palette for worktrees (integration point)
- Activity/agent badges and inline agent card expansion
- Task provider shortcuts (GitHub, GitLab, Linear, Jira)
- Push notification badge for Agents activity
- Imported worktrees visibility toggle and card actions

**Public API / IPC / RPC**
- Sidebar (default export, main component)
- index.tsx exports
- SidebarNav (Tasks, Automations, Agents, Mobile buttons)
- SidebarHeader (workspace board toggle, create workspace button)
- SidebarFilter (project, sleeping, default-branch filters)
- SidebarToolbar (add project, help, settings, feedback)
- WorktreeList (virtualized list container)
- WorktreeCard (individual worktree/workspace card)
- WorkspaceKanbanDrawer (workspace status board)
- useAppStore state selectors/mutators: sidebarOpen, sidebarWidth, setSidebarWidth, repos, fetchAllWorktrees, activeWorktreeId, toggleCollapsedGroup, setSortBy, setGroupBy, setShowSleepingWorkspaces, removeWorktree, openTaskPage, openActivityPage, openMobilePage, openModal, updateSettings, openSettingsPage, prefetchWorkItems, recordFeatureInteraction, createWorktree, moveWorktreesToStatus, pinWorktree, renamingWorktreeId, setRenamingWorktreeId, updateWorktreeMeta, deleteStateByWorktreeId, markWorktreesDeleting, clearWorktreeDeleteState, worktreeCardProperties, prCache, issueCache, tabsByWorktree, agentStatusByPaneKey, sshConnectionStates, filterRepoIds, setFilterRepoIds, hideDefaultBranchWorkspace, setHideDefaultBranchWorkspace
- Data hooks: useAllWorktrees, useWorktreeMap, useRepoMap (from store selectors)

**External dependencies**
- react (rendering)
- react-virtual / @tanstack/react-virtual (virtualization)
- lucide-react (icons: Pin, Loader2, Workflow, ChevronDown, Trash2, Search, etc.)
- sonner (toast notifications)
- @/components/ui/* (Button, Tooltip, Dropdown, Badge, ContextMenu, Popover - Radix-based)
- @/store (Zustand app state)
- @/lib/worktree-activation, @/lib/sidebar-worktree-activation (navigation helpers)
- @/lib/tab-has-live-pty (PTY detection)
- @/lib/running-agent-targets (agent send-target derivation)
- @/lib/right-sidebar-visibility (PR data visibility check)
- @/lib/worktree-git-identity-display (git metadata formatting)
- @/lib/window-visibility-interval (visibility polling)
- @/lib/passive-macos-app-data-access (macOS path checks)
- shared/types (Worktree, Repo, WorktreeLineage, ProjectGroup, WorkspaceStatus, etc.)
- shared/constants (DEFAULT_SHOW_SLEEPING_WORKSPACES)
- shared/repo-kind (isGitRepoKind, isFolderRepo)
- shared/task-providers (normalizeVisibleTaskProviders, resolveVisibleTaskProvider)
- shared/workspace-statuses (workspace status IDs, grouping)
- shared/worktree-ownership (external worktree visibility rules)
- shared/keybindings (keybindingMatchesAction)
- window.api.shell.openUrl (open external links)
- window.api.app.restart (restart Orca)

**Persistence**
- AppStore (Zustand): sidebarOpen, sidebarWidth, groupBy, sortBy, collapsedGroups, filterRepoIds, showSleepingWorkspaces, hideDefaultBranchWorkspace, settings (visibleTaskProviders, defaultTaskSource, defaultTaskViewPreset, showTasksButton, showAutomationsButton, showMobileButton), renamingWorktreeId, deleteStateByWorktreeId, worktreeCardProperties, prCache, issueCache

**Cross-platform concerns**
- macOS: isMacAppDataPath check for passive data access
- SSH: sshConnectionStates, sshConnectedGeneration, sshTargetLabels for remote worktrees
- Keyboard: platform-aware keybindings (getShortcutPlatform for Cmd vs Ctrl)
- Shell: window.api.shell.openUrl for cross-platform link opening
- Icon glyphs for GitHub/GitLab/repo types (platform-agnostic SVG)

### `ui-status-bar`

Bottom status bar that displays real-time telemetry and controls across multiple subsystems: LLM provider rate limits (Claude/Codex/Gemini/OpenCode Go), resource usage metrics (CPU/memory/sessions), SSH connection status, exposed ports, application updates, and experimental pet mascot. Provides account switching for providers and toggleable visibility of indicator segments.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-ui-status-bar`  
_Pure UI-native rewrite required. The status bar is a complex React component tree with fine-grained state management, responsive layout adaptation, and deeply integrated Radix UI components (DropdownMenu, Popover, Tooltip, Dialog). Non-negotiable requirements: (1) rebuild all 7 segments (provider menus, resource usage with sparklines and treemaps, ports, SSH, update, pet, floating terminal toggle) as native SwiftUI views on macOS; (2) preserve account switcher with WSL distro routing and rate-limit window rendering; (3) replicate resource usage popover's merged snapshot/session tree with kill confirmation dialogs; (4) implement workspace port scanner UI with external port list; (5) recreate SSH status rollup logic (connected/partial/connecting/disconnected) and sync phase display; (6) port context menu policy (opt-out selectors for nested surfaces). Secondary considerations: (1) icon rendering (lucide-react SVGs + custom Claude/Codex/Gemini/OpenCode Go icons must be ported as SF Symbols or drawn natively); (2) toast notifications (sonner) → NSAlert or custom overlay; (3) Zustand store → AppKit/SwiftUI@State or equivalent backend bridge; (4) IPC endpoints (window.api.*) require no change but shell bindings must reach them. Complexity drivers: responsive width-based layout switching (ResizeObserver → NSView.frame observation), account switch menu dynamism (buildCodexStatusSwitchGroups recursive group + target nesting), resource usage merge algorithm (mergeSnapshotAndSessions unifying memory snapshots + PTY sessions) and session termination with confirmation. No pure Rust intermediate layer; tight coupling to SwiftUI unavoidable._

**Capabilities**
- Display rate-limit windows for multiple LLM providers (session/weekly windows, usage percentages, bucket-based quotas)
- Account switching UI for Claude and Codex with runtime target selection (host vs WSL distros)
- Resource usage monitoring badge with CPU/memory metrics and session count, with collapsible detailed popover showing per-repo/worktree/session breakdown
- Port discovery and display (workspace-internal and external ports) with scan refresh on demand
- SSH connection status indicator with per-target status (connected/partial/connecting/disconnected) and sync phase display
- Application update indicator (downloading/ready/failed states) with progress percentage
- Experimental pet mascot toggle and character picker menu
- Floating terminal toggle (conditionally rendered based on settings)
- Context menu for toggling status bar item visibility (gated by CLI detection for agent-specific indicators)
- Right-click context menu dismissal with state isolation from other menus
- Responsive layout mode switching (compact icon-only view below 500px width, compact text below 900px)
- Activity indicator (spinning refresh icon) when rate limits are being fetched

**Public API / IPC / RPC**
- StatusBar component (exported as React.memo(StatusBarInner))
- buildCodexStatusSwitchGroups() — generate account switch menu entries for Codex
- buildClaudeStatusSwitchGroups() — generate account switch menu entries for Claude
- ProviderDetailsMenu component — dropdown menu for non-switching providers (Gemini, OpenCode Go)
- isStatusBarItemAvailable(id, detectedAgentIds) — gate indicator visibility on CLI detection
- shouldOpenStatusBarContextMenu(target) — determine if right-click opens global menu
- PetStatusSegment, ResourceUsageStatusSegment, PortsStatusSegment, UpdateStatusSegment, SshStatusSegment — sub-components for modular segments

**External dependencies**
- lucide-react — icon library (AlertTriangle, Activity, Plug, ChevronDown, RefreshCw, Server, MemoryStick, Loader2, etc.)
- sonner — toast notifications (error/success messaging)
- @/components/ui/* — internal Radix-based UI primitives (Button, Tooltip, DropdownMenu, Popover, Dialog, Badge, Input, Select, ContextMenu, HoverCard)
- window.api.claudeAccounts.* — IPC for Claude account list/select/reauthenticate
- window.api.codexAccounts.* — IPC for Codex account list/select/reauthenticate
- window.api.pty.listSessions() — IPC to fetch active PTY session list
- window.api.pty.kill(sessionId) — IPC to terminate a session
- window.api.ssh.connect/disconnect — IPC for SSH target management
- window.api.pet.import/importPetBundle — IPC for custom pet model uploads
- window.api.ui.writeClipboardText — IPC for clipboard copy (port addresses)
- ResizeObserver API — container width tracking for responsive layout

**Persistence**
- useAppStore — Zustand store subscriptions for: rateLimits (claude/codex/gemini/opencodeGo), statusBarItems (visibility toggles), statusBarVisible, floatingTerminalEnabled, settings (experimental flags), updateStatus, updateCardCollapsed, petId, petVisible, customPets, petSize, detectedAgentIds, activeWorktreeId
- AppStore.recordFeatureInteraction() — track feature telemetry events (usage-tracking, ssh, resource-manager, ports, pet-related)
- AppStore.toggleStatusBarItem(id) — persist visibility state per segment
- AppStore.setUpdateCardCollapsed/setCollapsed — persist UI state
- AppStore.setPetVisible/setPetId/setPetSize — persist pet preferences

**Cross-platform concerns**
- Windows-specific: navigator.userAgent check for 'Windows' to label host runtime as 'Windows' vs 'This device'
- Windows-specific: WSL distro selection and per-distro account routing (via getStatusBarPreferredWslDistro, normalizeCodexStatusRuntimeTarget)
- macOS/Linux: getActiveRuntimeTarget() uses settings.localAccountWslDistro/terminalWindowsWslDistro for runtime target resolution
- All platforms: ResizeObserver for responsive width-based layout switching (icon-only at <500px, compact at <900px)
- SSH status: connectionId-based remote detection (platform-agnostic sync phase reporting)
- Port scanning: platform-specific scanner via scanWorkspacePortsForTarget() returning platform key (windows/macos/linux/unknown)

### `ui-store`

Client-side state management for the Orca IDE, implemented as a Zustand store that holds the complete runtime state of repositories, worktrees, terminals, tabs, editor state, settings, integrations (GitHub, Linear, Jira, SSH), and UI chrome. This is the single source of truth for all renderer-side application state, synchronized with the backend via IPC/RPC calls.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-model or orca-state (new Rust crate in workspace) + serde for persistence`  
_This is pure data model — no business logic, no IO performed directly. However, it encompasses 20+ slice domains with complex cross-slice invariants (e.g., agent-status retention sync, tab group layout atomicity, cache TTL management). Rust port would be a struct-based event store + reducer pattern (similar to Redux). The Zustand hooks API surface must be replaced with Rust/Swift IPC stubs. Core challenges: (1) preserving WeakMap-based selector caching without Rust equivalent (likely resort to Arc<RwLock<>>), (2) replicating Zustand's shallow-equality-based subscriptions in a native app model, (3) modeling the complex union types (e.g., WorktreeNavHistoryEntry) and tagged structs, (4) handling the 'Map' and 'Set' usage for mutable collections — Rust would use HashMap/HashSet but lose JavaScript's reference semantics. Recommendation: model core state as nested structs, use serde for persistence, expose query API via RPC layer (not direct Rust/Swift objects)._

**Capabilities**
- Manage repository and worktree data structures
- Track terminal tabs, browser tabs, and unified tab groups with layout snapshots
- Maintain editor state (open files, diffs, git conflicts, search results)
- Store UI state (sidebar visibility, active modals, sidebar width, grouping/sorting preferences)
- Cache GitHub PRs, issues, checks, and comments with TTL-based invalidation
- Track Linear workspace selection, teams, projects, custom views, and cached issues
- Track Jira site selection and cached issues
- Manage SSH connection states, port forwards, detected ports, and credential requests
- Store agent status entries keyed by pane, with retention/snapshot system for completed agents
- Track detected agents (local and per-SSH-target) with refresh deduplication
- Maintain rate limit state for Claude and other model APIs by runtime target
- Store settings (global configuration, keybinding overrides, task provider visibility)
- Track workspace space analysis (disk usage by worktree/repo)
- Manage browser workspaces, pages, and session profiles with history
- Store diff comments (annotations on file diffs with delivery state)
- Track hosted review cache for branch reviews across GitHub/GitLab
- Maintain workspace cleanup candidates and dismissals
- Store memory snapshot and stats summary from daemon
- Handle Claude/Codex usage analytics (scans, summaries, breakdowns by model/project)
- Manage dictation state and speech model metadata
- Track worktree navigation history with back/forward navigation
- Store workspace-session-level diffs and conflict metadata

**Public API / IPC / RPC**
- useAppStore (Zustand hook - main store creator)
- useRepos, useActiveRepoId, useActiveRepo, useRepoMap, useRepoById
- useActiveWorktreeId, useWorktreesForRepo, useAllWorktrees, useWorktreeMap, useWorktreeById, useActiveWorktree
- useActiveTerminalTabs, useActiveTabId
- useSettings
- useSidebarOpen, useSidebarWidth, useActiveView, useActiveModal, useModalData, useGroupBy, useSortBy, useShowActiveOnly, useShowSleepingWorkspaces, useFilterRepoIds
- usePRCache, useIssueCache
- getAllWorktreesFromState, getWorktreeMapFromState, getHasAnyWorktreesFromState, getRepoMapFromState, selectFloatingVisibleTabCount
- createRepoSlice, createWorktreeSlice, createTerminalSlice, createTabsSlice, createUISlice, createSettingsSlice, createKeybindingsSlice
- createGitHubSlice, createHostedReviewSlice, createLinearSlice, createJiraSlice, createEditorSlice
- createSshSlice, createAgentStatusSlice, createDetectedAgentsSlice
- createStatsSlice, createMemorySlice, createBrowserSlice, createRateLimitSlice
- createSparsePresetsSlice, createDiffCommentsSlice, createWorkspaceSpaceSlice, createWorkspaceCleanupSlice
- createPreflightSlice, createDictationSlice, createWorktreeNavHistorySlice

**External dependencies**
- zustand (state management library)
- sonner (toast notifications)
- window.api (preload bridge to main process: IPC to runtime-rpc-client)
- @/runtime/runtime-rpc-client (RPC dispatch to backend)
- @/runtime/runtime-git-client (git operations via RPC)
- @/runtime/runtime-file-client (file operations via RPC)
- @/runtime/runtime-linear-client (Linear API via RPC)
- @/runtime/runtime-jira-client (Jira API via RPC)
- @/lib/agent-status (agent status detection and type inference)
- localStorage (implicit via electron-store-like patterns)

**Persistence**
- electron-store or equivalent: GlobalSettings (user settings, keybindings, task providers, UI preferences)
- electron-store: PersistedUIState (sidebar width, modal state, view preferences, feature tips, contextual tours)
- electron-store: PersistedOpenFile array (editor open files, git conflict metadata)
- electron-store: WorkspaceSessionState snapshots (terminal layouts, tab groups, browser history per session)
- electron-store: SetupScriptPromptDismissals, WorkspaceCleanupDismissals
- Memory-only (renderer session): agent status entries, terminal tab ownership cache, rate limit state, preflight status, detected agents, caches (GitHub PR/issue, Linear, Jira, hosted-review)

**Cross-platform concerns**
- Windows: WSL distro detection for runtime targeting (wslDistro field in rate-limit targets)
- Windows: WSL path handling (isWslUncPath detection in terminal slice)
- macOS/Linux: SSH terminal stream handling (pty ID parsing, remote runtime pty dispatch)
- macOS: MacOS-specific app data path detection (isMacAppDataPath for passive data access)
- All: Renderer user-agent detection (navigator.userAgent) for Windows vs Unix terminal behavior
- All: Cross-platform path normalization (isPathInsideOrEqual, joinPath utilities)

### `ui-tabs (TabBar + TabGroup)`

React component system for rendering and managing tabbed UI across multiple tab types (terminal, browser, editor). Handles drag-and-drop tab reordering within and across split pane groups, tab lifecycle (create/activate/close), and persistent pane layout with adjustable split ratios.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-ui-core (new SwiftUI-based crate for macOS, alacritty_terminal for terminal emulation, egui or similar for editor). Tab state machine logic can be 'io' tier (pure state transitions); rendering layer is ui-native.`  
_Pure state machine (tab activation, close, reorder, split-group logic) is portable to Rust. The entire rendering layer (React components, dnd-kit drag handlers, Tailwind styling, overlay positioning) must be rewritten natively. Key porting challenges: (1) dnd-kit drag-and-drop requires native gesture/pointer handling equivalent; (2) CSS anchor positioning for overlay targeting has no native equivalent (must use native positioning APIs); (3) Zustand store must map to Rust state management (Tauri state or custom Arc<Mutex<>>); (4) IPC channels (window.api.*) stay the same via Tauri command/listen; (5) Test coverage is heavy (17 tab-bar tests, 5 tab-group tests) — port test suite in parallel. The split-layout resize handle is pure imperative pointer logic (easily Rust-friendly). The rename-in-place inline editor must be reimplemented with native text input. Terminal/browser/editor tab pane bodies are managed by other subsystems (xterm.js → alacritty_terminal, React Editor → native code editor, webview → ?); this subsystem just coordinates focus/activation._

**Capabilities**
- Render terminal, editor file, and browser tabs in a horizontal tab strip with overflow scrolling
- Support tab drag-and-drop reordering within same group via dnd-kit
- Support cross-group tab dragging to split panes in left/right/up/down directions
- Display visual drop indicator (left/right blue bar) on drag hover over target tabs
- Display visual drop-zone overlay (center/left/right/up/down) on drag hover over pane bodies
- Activate/focus tabs and underlying content surfaces (terminals via focusTerminalTabSurface, editors/browsers via store state)
- Close individual tabs with pinning support to prevent accidental closes
- Context menu on right-click with close-others, close-to-right, split-group, and close-all options
- Inline rename tabs with commit on Enter, cancel on Escape
- Tab color picker for visual customization stored in unified tab state
- Display unread activity bell icon and amber wash background on terminal tab bell events
- Show git status indicators (diff/conflict icons) on editor file tabs
- Generate and display auto-generated tab titles (via Zustand settings.tabAutoGenerateTitle)
- Render agent provider icons on terminal tabs (from resolved TuiAgent via store hook)
- Handle keyboard shortcuts: Ctrl+Tab cycle active tab, F2 rename, context menu on Shift+F10
- Multi-split layout: render TabGroupSplitLayout with resizable dividers (5px pointer cap, MIN_RATIO 0.15 / MAX_RATIO 0.85)
- Persist split group structure and pane ratios in Zustand store
- Create new terminal tabs with optional Windows shell override (PowerShell/CMD/Git Bash/WSL)
- Quick launch agent menu (QuickLaunchButton) with memoized agent catalog ordered by defaults
- Handle Windows-specific shell capabilities detection and display shell-specific icons
- Mirror tab moves to paired web runtime sessions (mobile/cloud deployment)
- Emit custom event CLOSE_ALL_CONTEXT_MENUS_EVENT on window to dismiss all open context menus atomically
- Track tab-group focused state and active tab type (terminal/editor/browser) per group
- Apply CSS anchor positioning to anchor tab pane bodies for overlay positioning
- Reconcile tab order after mutations to maintain stable sort-order field
- Guard against editor tab opens when file already has unsaved conflicts/diffs

**Public API / IPC / RPC**
- TabBar (default export): renders terminal, editor, and browser tabs in sortable context with callbacks
- TabGroupPanel (default export): wraps TabBar + editor panel + drag overlays for a single pane
- TabGroupSplitLayout (default export): renders resizable split pane layout with tab group panels
- SortableTab (default export): individual terminal tab component with drag handle, context menu, rename
- EditorFileTab (default export): individual editor file tab component with git status, diff/conflict icons
- BrowserTab (default export): individual browser tab component with favicon, url display
- getBrowserTabLabel(tab): derives display label from browser tab title/url
- useTabDragSplit(worktreeId, enabled): hook returning activeDrag, collisionDetection, hoveredDropTarget, handlers (onDragStart/Move/Over/End/Cancel), sensors, setDragRootNode
- useTabGroupWorkspaceModel(groupId, worktreeId): hook returning activeTab, terminalTabs, editorItems, browserItems, commands object with activateTerminal/activateEditor/activateBrowser/closeItem/closeAllItems/splitGroup/createTerminal/createBrowser/createEditor
- resolveTabInsertion(event, isTabDragData, getDragCenter): computes HoveredTabInsertion from drag move/over/end events
- resolveTabIndicatorEdges(orderedVisibleTabIds, hoveredTabInsertion): maps HoveredTabInsertion to TabIndicatorEdge list for rendering left/right drop bars
- useHoveredTabInsertion(isTabDragData, getDragCenter): hook tracking hovered tab insertion, returns { clear(), update(event), tabInsertion }
- getTabPaneBodyDroppableId(groupId): derives dnd-kit droppable id for pane body
- tabGroupBodyAnchorName(groupId): derives CSS anchor-name for position-anchor overlay targeting
- getDropIndicatorClasses(dropIndicator): returns Tailwind classes for left/right pseudo-element blue bars
- getTabRootStateClasses(isActive): returns Tailwind classes for active vs inactive tab backgrounds
- reconcileTabOrder(existingOrder, currentTabIds): merges persisted sort-order with newly created tabs
- resolveGroupTabFromVisibleId(visibleId, groupTabs): looks up unified tab by visible id (entity id or unified id)
- buildTabAgentLaunchOptions(agents, cmdOverrides): builds menu items with labels and launch commands
- orderTabLaunchAgents(defaultAgent, detectedIds): orders agent list by default + detected
- resolveWindowsShellLaunchTarget(shell, runtimeTarget): maps shell name to PTY launch target
- CLOSE_ALL_CONTEXT_MENUS_EVENT: custom event name literal 'orca-close-all-context-menus'
- window.api.shell.openUrl(url), window.api.ui.writeClipboardText(text), window.api.shell.openPath(path), window.api.ui.onCtrlTabKeyDown/Up(handler)

**External dependencies**
- @dnd-kit/core (DndContext, DragOverlay, useDroppable, DragEndEvent, DragMoveEvent, DragOverEvent, DragStartEvent, CollisionDetection, pointerWithin, closestCenter, UniqueIdentifier, useSensor, useSensors, PointerSensor)
- @dnd-kit/sortable (SortableContext, useSortable)
- lucide-react (FilePlus, FileText, Globe, Plus, TerminalSquare, X, Minimize2, Pin, Columns2, Rows2, Copy, ExternalLink, Eye, ShieldAlert, Ellipsis, GitCompareArrows, PinOff)
- sonner (toast.error, toast.success)
- zustand/react/shallow (useShallow selector optimization)
- react (useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, lazy, Suspense)
- @/components/ui/dropdown-menu (DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger, DropdownMenuShortcut)
- @/components/ui/input (Input)
- @/components/ui/tooltip (Tooltip, TooltipContent, TooltipTrigger)
- window.api.* IPC channels (shell.openUrl, ui.writeClipboardText, shell.openPath, ui.onCtrlTabKeyDown, ui.onCtrlTabKeyUp)
- Electron webview element (for browser tab content overlay positioning)

**Persistence**
- groupsByWorktree[worktreeId]: TabGroup[] with id, activeTabId, tabOrder (unified tab ids)
- unifiedTabsByWorktree[worktreeId]: Tab[] with id, groupId, entityId, contentType, label, customLabel, color, sortOrder, isPinned, generatedLabel, createdAt
- tabsByWorktree[worktreeId]: TerminalTab[] with id, ptyId, title, defaultTitle, generatedTitle, customTitle, color, shellOverride, launchAgent, generation, pendingActivationSpawn, sortOrder, createdAt
- openFiles (editor slice): OpenFile[] with id, filePath, relativePath, mode (normal/preview/diff/conflict-review/markdown-preview), isDirty, tabId (unified tab link)
- browserTabsByWorktree[worktreeId]: BrowserTab[] with id, url, title, faviconUrl, tabId (unified tab link)
- unreadTerminalTabs: Record<terminalTabId, boolean> (bell activity state)
- renamingTabId: string | null (transient flag for rename trigger)
- expandedPaneByTabId: Record<tabId, boolean> (terminal pane collapse state)
- settings.tabAutoGenerateTitle: boolean (auto-title enablement)
- settings.activeRuntimeEnvironmentId: string | null (web/SSH runtime selection)
- settings.agentCmdOverrides: Record<TuiAgent, string> (custom launch commands)
- settings.defaultTuiAgent: TuiAgent (default agent for new tabs)
- settings.terminalWindowsShell: string (Windows shell choice: powershell.exe, cmd.exe, git bash, wsl)
- settings.terminalWindowsPowerShellImplementation: string (auto/pwsh/pwsh-desktop/pwsh-core)
- settings.experimentalUnifiedNewTabLauncher: boolean (new tab menu variant)
- activeTabIdByWorktree[worktreeId]: string | null (selected unified tab per worktree)
- activeWorktreeId: string | null (selected worktree for multi-repo)
- activeTabType: WorkspaceVisibleTabType (terminal/editor/browser/none)

**Cross-platform concerns**
- Windows shell detection: navigator.userAgent.includes('Windows'), then query capabilities via IPC (getWindowsTerminalCapabilities)
- SSH-backed PTY detection: repo.connectionId presence suppresses Windows shell menu (shell overrides don't apply remotely)
- Web runtime client detection: globalThis.__ORCA_WEB_CLIENT__ boolean flag for special handling of mobile/cloud tab moves
- Shell-specific icons: ShellIcon component maps shell name to shell-appropriate Lucide icon (PowerShell, CMD, Git Bash, WSL)
- Keyboard layout awareness: useShortcutLabel hook maps shortcuts to localized display strings
- Platform-specific path handling: normalizeRelativePath, basename for editor tab labels

### `ui-terminal`

Provides a multi-pane terminal UI with xterm.js emulation, split layouts, scrollback management, quick commands, and search. Handles PTY lifecycle, session serialization, and agent status tracking via IPC to the main process.

**Rust portability:** tier=`ui-native` · effort=`XL` · target=`orca-terminal (new crate in orca-native workspace); depends on alacritty_terminal for VT parsing/rendering, crossterm for keyboard event handling`  
_xterm.js (full React DOM-based terminal emulator) must be rewritten as native UI. Replace with alacritty_terminal (pure Rust VT parser) + native text rendering (SwiftUI Text + TextEditor on macOS; GTK4 TextBuffer on Linux). Significant work: (1) Parse terminal layout trees to native split view containers, (2) Implement xterm-compatible link detection (file paths, URLs, handles) in Rust, (3) Port search addon (regex + case-sensitive), (4) Implement serialization round-trip for scrollback (buffer capture format compatibility), (5) Ligature support via native font APIs (macOS CTFont, Linux Pango), (6) OSC sequence handling (7/9999 agents status, 52 clipboard, 2031 color scheme), (7) Keyboard routing with modifier bypass logic, (8) Multi-pane split layout ops (DFS traversal, resizing, reordering drag). NOT pure: calls native PTY via already-ported node-pty wrapper; integrates with existing app store for state sync. IPC layer (window.api.pty) remains in renderer but calls Rust backend via bridge instead of main Electron process._

**Capabilities**
- Multi-pane terminal rendering with vertical/horizontal splits
- Pane layout serialization/deserialization with flex ratios
- Terminal tab management (create/close/switch tabs per worktree)
- Terminal search with regex and case-sensitive modes
- Scrollback buffer capture/restore across app restarts
- Terminal quick commands (terminal-command or agent-prompt actions)
- PTY connection lifecycle: spawn, data streaming, exit handling, serialization
- Agent status detection from OSC 9999 payloads and title scanning
- Bracket paste mode detection and paste handling
- OSC 7 working directory tracking
- Ligatures rendering support
- File and URL link detection in terminal output
- Terminal bell detection for notifications
- Search addon with decorations and match highlighting
- Floating terminal panel with resizing, drag-reorder, tab management
- Terminal appearance (colors, fonts, cursor styles) from theme system
- Layout expansion/collapse with visual isolation for Activity portal
- Keyboard shortcut routing (Cmd+G/Cmd+Shift+G search nav, split/close pane shortcuts)
- Interactive keyboard event bypass policy for keys handled by main process
- GPU acceleration control, complex script detection
- Terminal output backlog with priority scheduling
- Pane focus management and follow-mouse options
- Mac Option-as-Alt modifier behavior per settings

**Public API / IPC / RPC**
- TerminalPane React component (default export)
- FloatingTerminalPanel React component
- TerminalSearch React component
- useTerminalKeyboardShortcuts hook
- useTerminalFontZoom hook
- useTerminalPaneLifecycle hook
- useTerminalPaneGlobalEffects hook
- createNewTerminalTab function
- closeTerminalTab function
- activateTerminalTab function
- toggleTerminalPaneExpand function
- connectPanePty function
- recordRuntimeCreatedTerminalPaneSplit function
- recordKeyboardCreatedTerminalPaneSplit function
- createTerminalQuickCommandDraft function
- TerminalQuickCommandDialog component
- PaneManager class (pane lifecycle, splits, layout ops)
- window.api.pty.onData event listener
- window.api.pty.onReplay event listener
- window.api.pty.onExit event listener
- window.api.pty.getMainBufferSnapshot IPC call
- window.api.pty.ackColdRestore IPC call
- window.api.pty.signal IPC call
- window.api.pty.sendSerializedBuffer IPC call
- window.api.pty.onSerializeBufferRequest listener
- window.api.pty.onClearBufferRequest listener
- TOGGLE_TERMINAL_PANE_EXPAND_EVENT custom event
- SPLIT_TERMINAL_PANE_EVENT custom event
- CLOSE_TERMINAL_PANE_EVENT custom event

**External dependencies**
- @xterm/xterm (6.1.0-beta.220) - terminal emulator core
- @xterm/addon-search (0.17.0-beta.220) - terminal search functionality
- @xterm/addon-ligatures (0.11.0-beta.220) - ligature rendering
- @xterm/addon-fit (0.12.0-beta.220) - pane fitting
- @xterm/addon-serialize (0.15.0-beta.220) - serialization for scrollback
- @xterm/addon-unicode11 (0.10.0-beta.220) - Unicode 11 support
- @xterm/addon-web-links (0.13.0-beta.220) - URL link detection
- @xterm/addon-webgl (0.20.0-beta.219) - GPU rendering
- @xterm/headless (6.1.0-beta.220) - headless terminal for testing
- node-pty (1.1.0) - PTY process spawning (called via IPC from main)
- react (19.2.5) - UI framework
- sonner - toast notifications
- lucide-react - icons
- Electron IPC to main process - pty control and data streaming

**Persistence**
- terminal-layout snapshot (file-based via workspace session storage)
- scrollback buffers per pane (serialized and restored across restarts)
- tab order per worktree (tabBarOrderByWorktree store slice)
- active terminal tab ID per worktree (activeTabId store slice)
- terminal tabs per worktree (tabsByWorktree store slice)
- floating terminal panel bounds (localStorage via floating-terminal-panel-bounds)
- expanded pane state (transient layout override ref)
- quick commands (TerminalQuickCommand objects stored in app state)

**Cross-platform concerns**
- macOS: Option-as-Alt mapping behavior (via keyboard-layout probe)
- macOS: LibUSA JIS Yen input handling
- Windows: ConPTY device attributes query for compatibility
- Windows: Path escaping for shell execution
- Linux: Standard monospace font fallback chain
- SSH: Session detection and error handling for remote PTY
- Web runtime: Paired web terminal tab forwarding via RPC
- Mobile: Driver-based fit override logic and mobile layout reconciliation
- Dark mode: System preference detection and theme sync
- Primary selection: Linux X11 primary clipboard support

## cli

### `CLI Subsystem`

Command-line interface and RPC client for Orca. Provides declarative command specs, argument parsing/validation, command dispatch to handler functions, and transparent local/remote runtime communication via Unix sockets or WebSocket tunneling.

**Rust portability:** tier=`mixed` · effort=`M` · target=`orca-cli: argument parsing (clap or similar), RPC client (reqwest or native async TCP), Unix socket transport (tokio + tokio-uds), Windows named pipes, command routing dispatcher`  
_Pure Rust argument parsing and dispatch is low-effort. RPC transport (socket I/O, envelope framing, keepalive logic) is moderate — tokio provides async primitives but the envelope-based protocol with keepalive frames during long-polls needs careful porting. Desktop app launching differs per platform but is OS-agnostic once exposed as a function. The handler layer itself (file ops, worktree resolution, etc) is mostly data mapping — they call into the RPC client which is truly network I/O._

**Capabilities**
- Parse POSIX-style CLI args (--flag value, --flag=value, positional args)
- Normalize positional args to flags using command specs
- Validate commands and flags against declarative spec registry
- Dispatch commands to handler functions by name
- RPC client for local runtime (Unix socket or Windows named pipe)
- RPC client for remote runtime via WebSocket + pairing codes
- Auto-resolve worktree from cwd using git path resolution
- Launch/serve Orca app via child_process
- Output formatting (JSON, human-readable text)
- Error reporting with next-step suggestions
- Terminal/PTY interaction (read, write, wait, list)
- Browser control (navigation, screenshots, snapshots, interactions, wait conditions)
- File operations (open, diff, git status)
- Computer-use actions (click, drag, scroll, type, permissions)
- Orchestration (message send/receive, task dispatch, decision gates)
- Automations (schedule, create, edit, run)
- Repository management (add, search refs, set base)
- Worktree lifecycle (create, list, remove, set metadata)
- Codex/Claude terminal detection (interactive vs non-interactive command classification)

**Public API / IPC / RPC**
- main(argv?, cwd?) - entry point
- COMMAND_SPECS - exported spec registry
- buildCurrentWorktreeSelector(cwd) - path to selector
- normalizeWorktreeSelector(selector, cwd) - resolve aliases
- dispatch(commandPath, context) - internal router
- RuntimeClient class with call<T>(method, params?, options?) - core RPC
- RuntimeClient.getCliStatus()
- RuntimeClient.openOrca(timeoutMs?)
- serveOrcaApp(args) - headless server
- parseArgs(argv) - returns ParsedArgs
- validateCommandAndFlags(specs, parsed)
- printResult(response, json, formatter)
- reportCliError(error, json)
- HANDLERS map - string -> CommandHandler routing

**External dependencies**
- node net module (createConnection)
- node child_process (spawn, spawnSync)
- node crypto (randomUUID)
- node fs (readFileSync, writeFileSync, lstatSync, statSync, etc)
- node path (resolve, dirname, relative, join)
- node os (homedir, tmpdir, platform)
- shared/runtime-bootstrap (findTransport, getRuntimeMetadataPath)
- shared/runtime-types (Runtime* types)
- shared/pairing (parsePairingCode, PairingOffer)
- shared/protocol-compat (evaluateRuntimeCompat)
- shared/protocol-version (MIN_COMPATIBLE_RUNTIME_SERVER_VERSION)
- shared/runtime-environment-store (environment CRUD)
- zod (schema validation for RPC envelopes)

**Persistence**
- reads $HOME/Library/Application Support/orca (macOS) or XDG_CONFIG_HOME/orca (Linux) for runtime metadata JSON
- reads/writes runtime-environment-store for saved remote pairings
- respects ORCA_USER_DATA_PATH env var override
- reads ORCA_PAIRING_CODE, ORCA_ENVIRONMENT, ORCA_APP_EXECUTABLE env vars

**Cross-platform concerns**
- Windows: uses named pipes (\\?\pipe) instead of Unix sockets
- macOS: app launch via open command after resolving .app bundle from Electron execPath
- Linux: app launch via execPath directly
- APPDATA on Windows for user data path fallback
- Electron ELECTRON_RUN_AS_NODE detection for dev/packaged contexts
- signal handling (SIGINT, SIGTERM) for serve mode
- cross-platform path normalization (isPathInsideOrEqual, relative path checks)

## relay

### `Relay subsystem`

Remote daemon for SSH-deployed agent execution. Manages live PTY sessions, file operations, git operations, port discovery, and agent hook forwarding via framed JSON-RPC protocol over stdin/stdout or Unix domain socket reconnection.

**Rust portability:** tier=`mixed` · effort=`L` · target=`orca-relay`  
_Core logic is pure (framing, dispatching, git output parsing, workspace session JSON). Main IO layers: (1) PTY spawning via node-pty C++ addon (replace with pty or pty-rs crate), (2) filesystem ops (straightforward via std::fs), (3) git subprocess with careful timeout/error handling, (4) loopback HTTP server for agent hooks (tokio or tiny_http), (5) Unix socket for reconnection (tokio::net or std::os::unix). Platform-specific concerns: Windows batch spawning (cmd.exe wrapper), Linux /proc parsing, POSIX login shell detection. Shell wrapper materialization is file I/O only (no subprocess needed). Effort is Medium because of git subprocess orchestration complexity and stream registry lifetime management, but no UI-native or Electron dependencies. Fully feasible with tokio + pty-rs + std::fs/std::process._

**Capabilities**
- PTY spawning and lifecycle management with node-pty (Unix/Windows)
- Graceful shutdown with configurable grace period for reconnecting clients
- Unix domain socket server for relay reconnection via --connect bridge
- Version handshake protocol to refuse mismatched --connect bridges
- Framed JSON-RPC 2.0 protocol with sequence acknowledgment over raw TCP/socket
- Filesystem operations: readdir, readFile, writeFile, stat, lstat, mkdir, rename, copy, delete, realpath, streaming reads
- File search using ripgrep (rg) with fallback to git ls-files or readdir
- File watch registration with client-scoped cleanup on disconnect
- Git operations: status, diff, stage/unstage, commit, branch ops, rebase, push, pull, worktree management, history
- Agent execution (interactive and non-interactive) with process tree termination
- External automations listing (Hermes and OpenClaw cron jobs with run history)
- Port scanning on Linux via /proc/net/tcp and /proc/net/tcp6
- Preflight agent detection via login shell which command
- Plugin overlay materialization for OpenCode and Pi agent status extensions (per-PTY)
- Agent hook HTTP server (loopback) for remote agent CLI status forwarding
- Workspace session persistence and presence tracking (JSON files per namespace)
- Shell-ready wrapper materialization for overlay env restoration (zsh/bash)
- Orca CLI forwarding via relay.sock connection
- Request abort signals for client disconnection cleanup

**Public API / IPC / RPC**
- pty.spawn
- pty.attach
- pty.resize
- pty.sendSignal
- pty.shutdown
- pty.data
- pty.ackData
- pty.clearBuffer
- pty.serialize
- pty.revive
- pty.getCwd
- pty.getDefaultShell
- pty.getProfiles
- pty.getForegroundProcess
- pty.hasChildProcesses
- pty.listProcesses
- pty.getInitialCwd
- fs.readDir
- fs.readFile
- fs.readFileStream
- fs.writeFile
- fs.stat
- fs.lstat
- fs.createFile
- fs.createDir
- fs.createDirNoClobber
- fs.rename
- fs.renameNoClobber
- fs.copy
- fs.deletePath
- fs.realpath
- fs.search
- fs.listFiles
- fs.watch
- fs.unwatch
- fs.cancelStream
- fs.tempDir
- fs.workspaceSpaceScan
- git.status
- git.checkIgnored
- git.history
- git.commit
- git.diff
- git.stage
- git.unstage
- git.bulkStage
- git.bulkUnstage
- git.discard
- git.bulkDiscard
- git.abortMerge
- git.abortRebase
- git.conflictOperation
- git.branchCompare
- git.commitCompare
- git.upstreamStatus
- git.fetch
- git.fetchRemoteTrackingRef
- git.push
- git.pull
- git.fastForward
- git.rebaseFromBase
- git.branchDiff
- git.commitDiff
- git.listWorktrees
- git.addWorktree
- git.removeWorktree
- git.worktreeIsClean
- git.renameCurrentBranch
- git.exec
- git.isGitRepo
- agent.execNonInteractive
- agent.cancelExec
- externalAutomations.list
- externalAutomations.runs
- externalAutomations.create
- externalAutomations.update
- externalAutomations.act
- ports.detect
- preflight.detectAgents
- relay.status
- session.registerRoot
- session.resolveHome
- workspace.get
- workspace.patch
- workspace.presence
- agent.hook
- agent_hook.installPlugins
- agent_hook.requestReplay
- ssh_relay.configureGraceTime
- orca.cli
- ping
- event.happened
- slow.method
- fail.method

**External dependencies**
- node-pty
- child_process
- fs
- net
- http
- crypto
- path
- os
- util
- module
- ripgrep (rg binary)
- git binary
- sqlite3 (optional)

**Persistence**
- $HOME/.orca/sessions/{namespace}.json: workspace session snapshots
- $HOME/.orca-relay/agent-hooks/endpoint.env: agent hook HTTP server coords
- $HOME/.orca-relay/opencode-overlays/{paneId}/*: OpenCode plugin per PTY
- $HOME/.orca-relay/pi-overlays/{paneId}/*: Pi agent extension per PTY
- $HOME/.orca-relay/omp-overlays/{paneId}/*: OMP agent extension per PTY
- $HOME/.orca-relay/shell-ready/zsh/*: zsh wrapper overlay
- $HOME/.orca-relay/shell-ready/bash/*: bash wrapper overlay
- Unix domain socket: for --connect reconnection
- relay.js .version file: content-hashed version marker

**Cross-platform concerns**
- macOS/Linux: PTY via node-pty
- Windows: PTY via node-pty, conpty backend, cmd.exe wrapper for .cmd/.bat, taskkill for process tree
- Linux-only: port scanning via /proc/net/tcp (returns empty on non-Linux)
- POSIX: login shell spawning for agent detection
- SSH: SIGHUP handling (ignored on macOS/Linux to preserve PTY)
- Windows batch syntax validation and quoting

## preload

### `preload`

Electron preload bridge exposing a comprehensive typed main<->renderer IPC contract. Defines the security boundary and API surface through which the sandboxed renderer communicates with privileged Electron main-process functionality.

**Rust portability:** tier=`ffi` · effort=`XL` · target=`orca-preload-bridge (new crate exposing a C-compatible IPC layer wrapping native platform bindings)`  
_The preload layer is fundamentally tied to Electron's IPC/contextBridge APIs (C++ extension modules). Porting requires: (1) replacing IPC with native RPC mechanism (ZeroMQ, tonic, or custom socket protocol), (2) binding to platform-specific APIs (macOS input source ID via IOKit, Windows window management via Win32), (3) replicating notification sound playback (CoreAudio on macOS, PulseAudio on Linux), (4) WebView management (WKWebView on macOS), (5) SSH client (ssh2-rs), (6) file dialogs (native-dialog crate or platform FFI). The subscription streaming model can be ported to async Rust channels. This is a high-effort FFI rewrite because Electron's contextBridge and ipcRenderer have no Rust equivalents—you must build a ground-up IPC serialization layer (serde JSON + binary protocols) and platform bindings for system dialogs, audio, and process management._

**Capabilities**
- IPC request/response handling via ipcRenderer.invoke() for synchronous request-reply
- IPC event subscriptions via ipcRenderer.on()/off() for async broadcasts
- Streaming RPC subscriptions with binary message support for remote runtime environments
- Native file drag-and-drop integration with HTML5 drag events + webUtils.getPathForFile
- Notification sound playback with caching and deduplication (uses HTMLAudioElement)
- SSH target management with port forwarding and credential prompting
- Git repository operations (repos, worktrees, branches, merges, stashes)
- GitHub/GitLab/Linear/Jira issue tracking integrations with mutations
- PTY/terminal spawning, data I/O, signaling, scrollback serialization
- Browser/WebView guest registration with viewport overrides and grab mode
- Settings and keybindings file I/O with live reload subscriptions
- AppIdentity and platform detection (macOS input source ID, process.platform)
- File pickers for images, audio, directories with system dialogs
- Clipboard operations (read/write text/image, primary selection support)
- Speech recognition (model catalog, dictation, transcript streaming)
- Automations lifecycle (create/update/delete/run with dispatch results)
- Mobile device pairing with QR codes and runtime access grants
- Telemetry event tracking with main-side schema validation
- Diagnostics bundle collection and upload
- Rate limit tracking for API accounts (Claude, Codex, OpenCode)
- Agent hook status forwarding (cursor, copilot, codex, droid, etc.)
- Workspace space analysis and process port scanning

**Public API / IPC / RPC**
- api.app.getIdentity()
- api.app.restart(), api.app.reload(), api.app.relaunch()
- api.app.getKeyboardInputSourceId() [macOS]
- api.repos.list(), api.repos.add(), api.repos.clone()
- api.worktrees.list(), api.worktrees.create(), api.worktrees.remove()
- api.pty.spawn(), api.pty.write(), api.pty.resize(), api.pty.signal(), api.pty.kill()
- api.gh.viewer(), api.gh.prForBranch(), api.gh.workItem(), api.gh.mergePR()
- api.gh.listWorkItems(), api.gh.createIssue(), api.gh.addIssueComment()
- api.gh.prChecks(), api.gh.rerunPRChecks(), api.gh.listLabels()
- api.gl.viewer(), api.gl.mrForBranch(), api.gl.issue(), api.gl.mergeMR()
- api.linear.connect(), api.linear.searchIssues(), api.linear.createIssue()
- api.jira.connect(), api.jira.getIssue(), api.jira.createIssue()
- api.browser.registerGuest(), api.browser.unregisterGuest(), api.browser.openDevTools()
- api.settings.get(), api.settings.set(), api.settings.listFonts()
- api.keybindings.get(), api.keybindings.setAction(), api.keybindings.openFile()
- api.shell.openPath(), api.shell.pickDirectory(), api.shell.pickImage()
- api.ssh.listTargets(), api.ssh.connect(), api.ssh.addPortForward()
- api.notifications.dispatch(), api.notifications.requestPermission(), api.notifications.playSound()
- api.ui.readClipboardText(), api.ui.writeClipboardText(), api.ui.getZoomLevel()
- api.runtime.syncWindowGraph(), api.runtime.call(), api.runtimeEnvironments.subscribe()
- api.speech.startDictation(), api.speech.feedAudio(), api.speech.stopDictation()
- api.automations.list(), api.automations.create(), api.automations.runNow()
- api.mobile.getPairingQR(), api.mobile.listDevices(), api.mobile.revokeDevice()
- api.telemetryTrack(), api.telemetrySetOptIn(), api.telemetryGetConsentState()
- api.platform.get() -> { platform: string; osRelease: string }
- gitlab:viewer, gitlab:prForBranch, gitlab:mrForBranch [IPC channels]
- runtimeEnvironments:subscribe, runtimeEnvironments:subscriptionEvent [streaming RPC]
- pty:spawn, pty:write, pty:resize, pty:signal, pty:data, pty:exit [PTY lifecycle]
- gh:prRefreshEvent, worktrees:changed, repos:changed [broadcast events]

**External dependencies**
- electron (contextBridge, ipcRenderer, webFrame, webUtils)
- @electron-toolkit/preload (electronAPI helper)
- HTMLAudioElement (native browser API for notification sound playback)
- crypto.randomUUID() (for subscription ID generation fallback)

**Persistence**
- No persistent storage - preload is purely a bridge
- Caches: notification sound blob URL + HTMLAudioElement in memory (cachedNotificationSound)
- Session state: file drag-and-drop callback array (nativeFileDropCallbacks)

**Cross-platform concerns**
- macOS: getKeyboardInputSourceId() returns AppleCurrentKeyboardLayoutInputSourceID for IME detection
- macOS: process.getSystemVersion() for OS release version
- Windows: minimize(), maximize(), isMaximized(), onMaximizeChanged() for custom title bar
- Windows: WSL distro detection and shell availability checks (pwsh, git bash, WSL)
- Darwin: webFrame.getZoomLevel/setZoomLevel (cross-platform but tested on macOS)
- Platform detection: process.platform available to renderer via api.platform.get()

## shared

### `Orca Shared Subsystem`

Cross-cutting pure logic, type definitions, and data structures shared by Orca's main process (Electron) and remote SSH relay. Houses path parsing, text search coordination, git state logic, agent protocol normalization, scheduling, validation, and secure file operations—transport-agnostic code that must not diverge between local and remote implementations.

**Rust portability:** tier=`pure` · effort=`M` · target=`orca-shared`  
_Primarily pure-logic modules with zero Electron/DOM dependencies. Path normalization, text-search parsing, cron/RRULE scheduling, URL validation, agent-hook normalization, and branch-name sanitization are all self-contained. External deps are minimal (tweetnacl for E2EE, zod for validation). The main porting effort is: (1) regex library parity for agent-status detection and submatch extraction (Rust `regex` crate), (2) cron parsing (handwritten or crate like `cronparse`), (3) secure file ops with cross-platform chmod/ACL (fs crate + cross-platform crate for capabilities), (4) WebSocket and encryption (tokio-tungstenite + libsodium or RustCrypto equivalents). No native bindings needed. The IO-performing modules (secure-file.ts, agent-hook-listener.ts request parsing) should be extracted as separate `orca-io` crate to keep the core `orca-shared` pure. ssh-relay link will use `orca-shared` + `orca-io` together; main process keeps using Electron fs for file persistence but calls `orca-shared` functions for path logic and search normalization._

**Capabilities**
- Path normalization and translation (POSIX/Windows/WSL/UNC/file URIs)
- ripgrep and git-grep JSON parsing with match window clamping and line-content truncation
- Git upstream/merge-conflict status detection from cherry-mark and git commands
- Agent status detection from OSC terminal titles with word-boundary-aware regex
- HTTP agent-hook request parsing and payload normalization with size caps and warn-once sets
- Branch name generation from LLM output: kebab-case sanitization, humanity, and creature-name collision detection
- URL validation and normalization (file, http/https, localhost, file-path routing, search-engine fallbacks)
- Cron and RRULE schedule parsing and classification (hourly, daily, weekly, weekdays, custom)
- RRULE preset expansion to concrete next-run timestamps
- Text-search glob pattern splitting (comma-separated with escape handling)
- Setup script detection: lockfile inspection for package managers (pnpm, bun, yarn, npm)
- Hex color validation with optional leading hash
- Remote-runtime RPC call queueing with separate concurrency pools for foreground vs background methods
- WebSocket E2EE handshake: NaCl key agreement, derive-shared-key from public keys
- Secure JSON and file writing: atomic writes with temp files, POSIX chmod (0o600/0o700), Windows ACL hardening via PowerShell
- Per-PTY agent-hook listener state caching: prompts, tool snapshots, status replays, completed transcripts, AMP cache keys

**Public API / IPC / RPC**
- filesystemPathToFileUri(), fileUriToFilesystemPath(), filesystemPathHrefToFileUri()
- isPathInsideOrEqual(), relativePathInsideRoot(), resolveRuntimePath(), isRuntimePathAbsolute()
- decodeGitCQuotedPath()
- parseWslUncPath(), isWslUncPath()
- buildRgArgs(), ingestRgJsonLine(), buildGitGrepArgs(), buildSubmatchRegex(), ingestGitGrepLine(), finalizeSearchResults()
- normalizeRelativePath(), splitSearchGlobPatterns()
- extractLastOscTitle(), detectAgentStatusFromTitle(), STRONG_IDLE_KEYWORDS_RE, STRONG_WORKING_KEYWORDS_RE
- buildBranchNamePrompt(), sanitizeBranchSlug(), humanizeBranchSlug(), isAutoGeneratedCreatureBranchName()
- normalizeKagiSessionLink(), redactKagiSessionToken(), normalizeBrowserNavigationUrl(), normalizeExternalBrowserUrl(), buildSearchUrl(), looksLikeSearchQuery()
- upstreamOnlyCommitsArePatchEquivalent(), shouldForcePushWithLeaseForUpstream()
- parseRrule(), parseCron(), classifyAutomationSchedule(), nextOccurrenceAfter() [inferred from usage]
- buildRgArgs(), buildGitGrepArgs(), toGitGlobPathspec()
- HEX_COLOR_RE regex constant
- createHookListenerState(), clearPaneCacheState(), ingestRemoteHookRequest(), parseAgentStatusPayload()
- RuntimeRpcCallQueuePool.enqueue(), isBackgroundRuntimeMethod()
- sendRemoteRuntimeRequest<TResult>(), RemoteRuntimeClientError
- RemoteRuntimeSubscription, RemoteRuntimeSubscriptionCallbacks types
- writeSecureFile(), writeSecureJsonFile(), hardenSecurePath(), hardenExistingSecureFile()
- ORCA_HOOK_PROTOCOL_VERSION constant

**External dependencies**
- node:fs (writeFileSync, readSync, renameSync, mkdirSync, chmodSync, unlinkSync, etc.)
- node:fs/promises
- node:path (join, relative, dirname, normalize)
- node:os (homedir)
- node:crypto (randomUUID, randomBytes, createHash)
- node:child_process (execFileSync for Windows ACL hardening via PowerShell)
- node:http (IncomingMessage type)
- node:net
- node:events
- ws (WebSocket for remote-runtime E2EE client)
- tweetnacl (NaCl for E2EE key agreement: generateKeyPair, deriveSharedKey, encrypt/decrypt)
- zod (schema validation in some modules)
- vitest (test runner—dev-only)

**Persistence**
- None. Shared modules are stateless libraries. State is persisted by callers: main process writes orca-data.json (repos, worktrees, settings), relay manages remote PTY leases via main, agent-hook listener maintains per-PTY caches in memory (HookListenerState Maps).

**Cross-platform concerns**
- POSIX vs Windows path semantics (drive letters, UNC, separators, case-insensitivity)
- WSL path translation: \\wsl.localhost\distro and \\wsl$\distro UNC patterns
- Windows ACL hardening: PowerShell script execution, SID lookup via whoami.exe
- macOS: SF Mono default terminal font fallback
- Linux: DejaVu Sans Mono default, primary-selection middle-click paste enabled
- Windows: Cascadia Mono default, 0o600/0o700 chmod unsupported (PowerShell fallback)
- Terminal title OSC parsing: cross-agent standardization (Claude, Codex, Gemini, etc.)
- Cron parsing: leap-year edge cases (9-year scan window for Feb 29 across non-leap centuries)
