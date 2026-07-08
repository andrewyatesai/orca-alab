import { createRequire } from 'node:module'
import { join } from 'node:path'
import { existsSync } from 'node:fs'

// Typed surface of the orca-git side of the napi addon built from
// native/orca-node (the verified `orca_git` status/numstat/line-count parsers).
// It is the SAME orca_node.node the terminal binding loads — Node-API is
// ABI-stable, so one .node serves both bindings in plain Node and Electron.
// These parsers ARE the sole path: the main process requires the addon (the
// duplicated TypeScript parsers were deleted; the relay runs the same Rust
// core via wasm).

/** Streaming `git status --porcelain=v2 --branch` parser (chunked stdout). */
export type RustGitStatusParserHandle = {
  /** Feed one raw chunk; returns true once the entry count exceeds `limit`
   *  (0 disables the cap). */
  update(chunk: Buffer, limit: number): boolean
  /** Flush a final record with no trailing newline. */
  finish(): void
  /** Consume the parser and return the status-result JSON string. */
  result(limit: number): string
}

export type RustGitStatusParserCtor = new () => RustGitStatusParserHandle

/** Stateful NDJSON byte-budget line splitter (orca_net::NdjsonSplitter). `feed`
 *  returns complete lines to JSON.parse + the byte sizes of any dropped oversized
 *  lines; the buffer is proven never to exceed `maxLineBytes` (the daemon-socket
 *  OOM guard). */
export type RustNdjsonParserHandle = {
  feed(chunk: string): { lines: string[]; oversized: number[] }
  reset(): void
}

export type RustNdjsonParserCtor = new (maxLineBytes?: number) => RustNdjsonParserHandle

/** The stateful multi-agent orchestration store (messages/tasks/dispatch/gates/
 *  coordinator runs) backed by orca-runtime's bundled SQLite. The main-process
 *  `OrchestrationDb` shim holds ONE of these and delegates every method to it;
 *  the deleted TS `node:sqlite` twin was byte-identical. Row-returning methods
 *  return the JSON string of the TS Row shape (parse on the shim side); the
 *  shim owns all JS-side nondeterminism (generated ids, ISO completion stamps,
 *  display strings) and passes it IN — every other timestamp is SQLite's
 *  `datetime('now')`. Methods throw on store errors (matching the TS twin's
 *  thrown Error text for the dispatch/task guard paths). */
export type RustOrchestrationStoreHandle = {
  // messages
  insertMessage(
    id: string,
    fromHandle: string,
    toHandle: string,
    subject: string,
    body: string,
    messageType: string,
    priority: string,
    threadId: string | null,
    payload: string | null
  ): string
  getMessageById(id: string): string | null
  getUnreadMessages(handle: string, types: string[] | undefined): string
  getUndeliveredUnreadMessages(handle: string, types: string[] | undefined): string
  getAllMessages(handle: string, limit: number): string
  getAllMessagesForHandle(handle: string, limit: number, types: string[] | undefined): string
  getInbox(limit: number): string
  getThreadMessagesFor(threadId: string, toHandle: string, afterSequence: number | undefined): string
  markAsRead(ids: string[]): void
  markAsDelivered(ids: string[]): void
  // tasks
  createTask(
    id: string,
    spec: string,
    parentId: string | null,
    deps: string[],
    createdBy: string | null,
    taskTitle: string | null,
    displayName: string | null
  ): string
  getTask(id: string): string | null
  listTasks(status: string | undefined): string
  listTasksWithDispatch(status: string | undefined): string
  updateTaskStatus(id: string, status: string, result: string | null, completedAt: string | null): string | null
  // dispatch contexts
  createDispatchContext(taskId: string, assigneeHandle: string, id: string): string
  getDispatchContext(taskId: string): string | null
  getDispatchContextById(id: string): string | null
  getActiveDispatchForTerminal(handle: string): string | null
  getLatestDispatchForTerminal(handle: string): string | null
  completeDispatch(id: string): void
  completeActiveDispatchForTask(taskId: string): void
  failActiveDispatchForTask(taskId: string, error: string): string | null
  failDispatch(id: string, error: string): string | null
  recordHeartbeat(id: string, at: string): void
  getStaleDispatches(thresholdIso: string): string
  setDispatchTimestamps(id: string, dispatchedAt: string | null, lastHeartbeatAt: string | null): void
  // decision gates
  createGate(id: string, taskId: string, question: string, options: string[]): string
  resolveGate(id: string, resolution: string): string | null
  timeoutGate(id: string): string | null
  listGates(taskId: string | undefined, status: string | undefined): string
  getGate(id: string): string | null
  // coordinator runs
  createCoordinatorRun(id: string, spec: string, coordinatorHandle: string, pollIntervalMs: number | undefined): string
  getCoordinatorRun(id: string): string | null
  updateCoordinatorRun(id: string, status: string, completedAt: string | null): string | null
  getActiveCoordinatorRun(): string | null
  // queries + lifecycle
  getIdleTerminals(excludeHandles: string[]): string
  resetAll(): void
  resetTasks(): void
  resetMessages(): void
  /** Raw all-tables dump (real ids/timestamps) for the parity state harness. */
  dumpTablesJson(): string
  close(): void
}

export type RustOrchestrationStoreCtor = new (path: string) => RustOrchestrationStoreHandle

/** The JS git executor the "A bridge" calls back into. It MUST resolve (never
 *  reject) for a git that spawned and exited, carrying its `exitCode`, so Rust can
 *  classify a non-zero exit exactly like the native runner; a rejection means the
 *  spawn itself failed. `stdin` (when non-null) is piped to git — e.g. for
 *  `git patch-id --stable`. This is where `runner.ts`'s SSH/WSL/env routing lives. */
export type RustGitExecutor = (
  args: string[],
  stdin: string | null
) => Promise<{ stdout: string; stderr: string; exitCode: number }>

export type RustGitBinding = {
  GitStatusParser: RustGitStatusParserCtor
  /** NDJSON byte-budget line splitter (orca-net) — the daemon-socket OOM guard. */
  NdjsonParser: RustNdjsonParserCtor
  /** The stateful orchestration store (orca-runtime SQLite) the main-process
   *  `OrchestrationDb` shim delegates to (the TS `node:sqlite` twin was deleted). */
  OrchestrationStore: RustOrchestrationStoreCtor
  /** One-shot status scan; the cap is applied during the scan. Returns JSON. */
  parseStatusPorcelain(stdout: Buffer, limit: number): string
  /** `git diff --numstat` (text or `-z`) → `{path: {added?, removed?}}` JSON. */
  parseNumstat(stdout: Buffer): string
  /** `git worktree list --porcelain` (or `-z`) → `GitWorktreeInfo[]` JSON. */
  parseWorktreeList(output: string, nulDelimited: boolean): string
  /** NUL-delimited `git log` (GIT_HISTORY_COMMIT_FORMAT) → `GitHistoryItem[]` JSON. */
  parseGitHistoryLog(stdout: string): string
  /** Untracked-file additions: null for binary, 0 for empty, else line count. */
  countAdditionsInBuffer(bytes: Buffer): number | null
  /** Push-target *value* rules (remote/branch/URL) — null when valid, else the
   *  TS-identical error message. The unknown→typed guards stay in JS. */
  validateGitPushTargetRules(
    remoteName: string,
    branchName: string,
    remoteUrl: string | null
  ): string | null
  /** IO-tier "A bridge" proof: Rust drives `validate_git_push_target` (shape check
   *  + `git check-ref-format`) over a JS-supplied async git `executor`, so
   *  `runner.ts` still executes git (SSH-safe). Resolves null when valid, else the
   *  error message. */
  validateGitPushTargetViaExecutor(
    remoteName: string,
    branchName: string,
    remoteUrl: string | null,
    executor: RustGitExecutor
  ): Promise<string | null>
  /** IO-tier "A bridge" cutover: Rust drives the multi-round upstream/ahead-behind
   *  status for an explicit publish target (validate → rev-parse → rev-list → log)
   *  over the JS `executor`, applying the no-upstream swallow + normalization
   *  in-process. Resolves the `GitUpstreamStatus` JSON string, or rejects with the
   *  normalized error message. */
  getUpstreamStatusViaExecutor(
    remoteName: string,
    branchName: string,
    remoteUrl: string | null,
    executor: RustGitExecutor
  ): Promise<string>
  /** IO-tier "A bridge" cutover: Rust drives the EFFECTIVE upstream/ahead-behind
   *  status (no explicit target — resolves the configured upstream, then ahead/behind
   *  + patch equivalence) over the JS `executor`, applying the no-upstream swallow +
   *  normalization in-process. Resolves the `GitUpstreamStatus` JSON string, or rejects
   *  with the normalized error message. */
  getEffectiveUpstreamStatusViaExecutor(executor: RustGitExecutor): Promise<string>
  /** IO-tier "A bridge" cutover: Rust drives the read-only rebase-source resolver
   *  (`git remote` → longest match → `check-ref-format`) over the JS `executor`.
   *  Resolves `{remoteName, branchName, displayName}` JSON, or rejects with the RAW
   *  resolver message (the caller normalizes). `git pull --rebase` stays in TS. */
  resolveGitRemoteRebaseSourceViaExecutor(
    baseRef: string,
    executor: RustGitExecutor
  ): Promise<string>
  /** IO-tier "A bridge" cutover: Rust drives the branch-cleanup safe-to-delete
   *  DECISION (gather base refs → non-fatal fetch → tree/merge/patch/squash checks,
   *  the squash path piping patch text to `git patch-id --stable` via executor stdin)
   *  over the JS `executor`. Resolves the boolean; the destructive `git branch -d`
   *  stays in TS, gated on it. Only ever moves toward *preserve* — never over-deletes. */
  branchIsSafeToDeleteViaExecutor(branchName: string, executor: RustGitExecutor): Promise<boolean>
  /** IO-tier "A bridge" cutover (the one destructive op): Rust drives `git push`
   *  — validate an explicit target, resolve the refspec (explicit, else the branch's
   *  fully-resolved configured push remote, else first-publish origin/HEAD), run
   *  `git push [--force-with-lease] --set-upstream …` — over the JS `executor`. An
   *  explicit target needs both remoteName+branchName (else the configured path).
   *  Resolves void, or rejects with the already-normalized error message. */
  gitPushViaExecutor(
    remoteName: string | null,
    branchName: string | null,
    remoteUrl: string | null,
    forceWithLease: boolean,
    executor: RustGitExecutor
  ): Promise<void>
  /** Approximate added/removed line counts JSON, or null for the large guard. */
  computeLineStats(original: string, modified: string, status: string): string | null
  /** Decode a git C-quoted (octal-escaped) path. */
  decodeGitCQuotedPath(value: string): string
  /** True when a fetch/pull error message means the remote ref does not exist. */
  isMissingRemoteRefGitError(message: string): boolean
  /** Default `git clone` folder name from a URL; throws on unsafe names. */
  deriveCloneRepoNameFromUrl(url: string): string
  /** `<destination>/<repoName>` with escape validation; `platform` is the
   *  Node `process.platform` value ("win32" → Windows path rules, else POSIX).
   *  Throws the TS-identical message on invalid input. */
  deriveValidatedClonePath(url: string, destination: string, platform: string): string
  /** Stable clone-path comparison key (WSL-UNC aware); pass a resolved path. */
  getClonePathComparisonKey(clonePath: string): string
  /** User-facing message for a git remote-operation error. `message` is
   *  undefined for a non-Error throw; `operation` is "push"|"pull"|"fetch"|
   *  "upstream". Mirrors the wasm export the relay runs. */
  normalizeGitErrorMessage(message: string | undefined, operation?: string): string
  /** True only for clearly-no-upstream signals (an expected state). */
  isNoUpstreamError(message: string | undefined): boolean
  /** Scrub credentials embedded in a git URL within `message`. */
  stripCredentialsFromMessage(message: string): string
  /** "omp" when the launch command starts OMP, else "pi". */
  detectPiAgentKindFromCommand(command: string | undefined): string
  /** Skill markdown frontmatter summary (name/description) as JSON. */
  summarizeSkillMarkdown(markdown: string): string
  /** Commit-message spawn plan as `CommitMessagePlanResult` JSON. Input is the
   *  `CommitMessagePlanInput` object as JSON + the prompt. */
  planCommitMessageGeneration(planInputJson: string, prompt: string): string
  /** Spawn binary + prefix args from an optional command override, as
   *  `{ok:true, binary, prefixArgs} | {ok:false, error}` JSON. */
  planAgentBinary(defaultBinary: string, commandOverride: string | undefined): string
  /** PR-fields generation prompt (TS `buildPullRequestFieldsPrompt`). `contextJson`
   *  is the `PullRequestDraftContext` object; returns the prompt string. */
  buildPullRequestFieldsPrompt(contextJson: string, customPrompt: string): string
  /** Parse an agent's PR-fields reply (TS `parseGeneratedPullRequestFields`) as
   *  `{ok:true, fields:{base,title,body,draft}} | {ok:false, error}` JSON;
   *  `fallbackJson` supplies the current fields for missing/blank values. */
  parseGeneratedPullRequestFields(raw: string, fallbackJson: string): string
  /** Validate raw session JSON as a `WorkspaceSessionState`, returning the
   *  `ParsedWorkspaceSession` union (`{ok:true, value} | {ok:false, error}`) JSON. */
  parseWorkspaceSession(rawJson: string): string
  /** Parse an OpenSSH config file into `SshConfigHost[]` JSON. `home` is the
   *  `~`-expansion base (the caller's `os.homedir()`). */
  parseSshConfig(content: string, home: string): string
  gitEngine(): string
}

function candidatePaths(): string[] {
  const paths: string[] = []
  // Git-specific override first, then the terminal override (it points at the
  // same .node), then the standard dev/packaged locations.
  const override = process.env.ORCA_RUST_GIT_ADDON ?? process.env.ORCA_RUST_TERMINAL_ADDON
  if (override) {
    paths.push(override)
  }
  // Dev tree layout: <repo>/native/orca-node/orca_node.node. process.cwd() is
  // the repo root under `pnpm dev`.
  paths.push(join(process.cwd(), 'native', 'orca-node', 'orca_node.node'))
  // Packaged layout: alongside other unpacked native resources. resourcesPath
  // is Electron-only, so read it defensively rather than via the global type.
  const resourcesPath = (process as { resourcesPath?: string }).resourcesPath
  if (resourcesPath) {
    paths.push(join(resourcesPath, 'orca_node.node'))
  }
  return paths
}

let cached: RustGitBinding | null | undefined

/** Load the orca-git addon, or return null if it is unavailable. Prefer
 *  {@link requireRustGitBinding} in the main process, where the addon is a hard
 *  requirement — this null-returning form exists for the few callers that must
 *  probe availability (e.g. the parity tests, which skip when it is absent). */
export function loadRustGitBinding(): RustGitBinding | null {
  if (cached !== undefined) {
    return cached
  }
  const req = createRequire(import.meta.url)
  for (const path of candidatePaths()) {
    if (!existsSync(path)) {
      continue
    }
    try {
      const binding = req(path) as RustGitBinding
      if (binding && typeof binding.GitStatusParser === 'function') {
        cached = binding
        return cached
      }
    } catch {
      // try the next candidate; a bad/incompatible addon must not break startup
    }
  }
  cached = null
  return cached
}

/** Load the orca-git addon or throw. Use this in the main process, where the
 *  native addon is a mandatory dependency (the terminal daemon already requires
 *  it) — git parsing runs through Rust with no TypeScript fallback. */
export function requireRustGitBinding(): RustGitBinding {
  const binding = loadRustGitBinding()
  if (!binding) {
    throw new Error(
      'orca-git native addon (orca_node.node) failed to load; it is a required dependency of the main process'
    )
  }
  return binding
}
