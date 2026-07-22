/* tslint:disable */
/* eslint-disable */

/**
 * Prepared Quick Open index for the RENDERER: the worktree file list crosses
 * the wasm boundary ONCE (NUL-joined — file names cannot contain NUL), then
 * each keystroke sends only the query and gets the top-N `{path, score}`
 * JSON back. Preparation (slash-normalize, lowercase, UTF-16 encode) happens
 * at construction, so the per-keystroke cost is only the subsequence scans.
 */
export class QuickOpenIndex {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Exact-path and exact-basename matches for an already-lowercased query
     * (the TS `findExistingFileMatches` passes), as
     * `{"paths":[…],"basenames":[…]}` JSON in input order.
     */
    exactMatches(lower_query: string): string;
    fileCount(): number;
    constructor(nul_joined_paths: string);
    /**
     * Rank against the prepared list; returns `[{path, score}, …]` JSON,
     * best (lowest score) first, ties by original input order.
     */
    rank(query: string, limit: number): string;
}

/**
 * Relay twin of the napi `branch_is_safe_to_delete_via_executor`: gather candidate
 * base refs, refresh the relevant remotes (`fetch --prune`, the one mutation),
 * then decide whether the branch has any unmerged changes (tree-equal merge,
 * patch-equivalent commits, or a squash match — which pipes patch text to
 * `git patch-id --stable` via the executor's stdin), all over the relay's async
 * JS git executor. Resolves the boolean; the destructive `branch -d/-D` stays in
 * the relay's TS, gated on this. The decision only ever moves toward *preserve*,
 * so it can never over-delete (and never rejects — git errors degrade to safe).
 */
export function branchIsSafeToDeleteViaExecutor(executor: Function, branch_name: string): Promise<boolean>;

/**
 * Build the PR-fields generation prompt (TS `buildPullRequestFieldsPrompt`); the
 * renderer's dry-run preview dialog runs this. `context_json` is the
 * `PullRequestDraftContext` object; returns the prompt string.
 */
export function buildPullRequestFieldsPrompt(context_json: string, custom_prompt: string): string;

/**
 * Approximate added/removed line counts for a diff section; returns the
 * line-stats JSON, or `undefined` for the large-input guard (>500k combined
 * chars — splitting that in a React render would block the UI). This one is
 * consumed by the RENDERER (not the relay): the renderer has no napi access,
 * so it loads this same wasm.
 */
export function computeLineStats(original: string, modified: string, status: string): string | undefined;

/**
 * Count additions for an untracked file's contents: `undefined` for binary, 0 for
 * empty, else the trailing-newline-aware line count.
 */
export function countAdditionsInBuffer(bytes: Uint8Array): number | undefined;

/**
 * Decode a git C-quoted (octal-escaped) path. Raw (unquoted) input passes through.
 */
export function decodeGitCQuotedPath(value: string): string;

/**
 * Short generated tab title from a free-form agent prompt (first clause,
 * filler stripped, capped at a word boundary), or `undefined` when the prompt
 * has no usable title text. Consumed by the RENDERER terminal store.
 */
export function deriveGeneratedTabTitle(prompt: string): string | undefined;

/**
 * Which Pi-compatible agent a launch command starts: `"omp"` for OMP
 * (`omp` / `omp.sh`), else `"pi"`. The relay uses this to target the managed
 * extension dir for the actual agent being launched.
 */
export function detectPiAgentKindFromCommand(command?: string | null): string;

/**
 * The actionable nested-submodule rejection hidden behind a recursive-push
 * failure, or `undefined`. Consumed by the RENDERER (push-failure toasts) via
 * this same wasm.
 */
export function formatSubmodulePushFailureDetail(message: string): string | undefined;

/**
 * Combined Linear identifier+title workspace seed (dedup-aware).
 */
export function getLinearIssueWorkspaceName(identifier: string, title: string): string;

/**
 * Title → slug suggestion for a linked work item (TS takes `{ title }`; the
 * wrapper passes `.title`).
 */
export function getLinkedWorkItemSuggestedName(title: string): string;

/**
 * Display+seed for a linked work item as `{displayName, seedName}` JSON, or
 * `undefined` when no git-safe seed derives. Input is the work item as JSON.
 */
export function getLinkedWorkItemWorkspaceName(item_json: string): string | undefined;

/**
 * Relay twin of the napi `get_upstream_status_via_executor` (EXPLICIT publish
 * target): drive orca-git's upstream/ahead-behind status — `check-ref-format` →
 * `rev-parse` verify → (conditional) `rev-list` → (conditional) cherry-mark
 * `log` — over the relay's async JS git executor, with the data-dependent
 * decisions and the no-upstream swallow + error normalization owned by Rust.
 * Resolves the `GitUpstreamStatus` JSON (exact TS shape); rejects with the
 * already-normalized message (preserved as a JS `Error`). The JS-boundary
 * "Invalid PR push target …" shape guard stays in the relay's TS caller.
 */
export function getUpstreamStatusViaExecutor(executor: Function, remote_name: string, branch_name: string, remote_url?: string | null): Promise<string>;

/**
 * First-create intent display+seed as `{displayName, seedName}` JSON, or
 * `undefined`. Input is `{sourceText?, workItem?, fallbackName?}` JSON.
 */
export function getWorkspaceIntentName(args_json: string): string | undefined;

/**
 * Relay twin of the napi `git_fetch_via_executor`: validate an explicit target
 * (`check-ref-format`) then `fetch --prune [<remote>]` over the relay's async JS
 * git executor. An explicit target needs BOTH remote+branch; otherwise a plain
 * prune-fetch. `git_fetch` normalizes errors internally, so this rejects with
 * the already-normalized message (preserved as a JS `Error`). The JS-boundary
 * shape guard stays in the caller.
 */
export function gitFetchViaExecutor(executor: Function, remote_name?: string | null, branch_name?: string | null, remote_url?: string | null): Promise<void>;

/**
 * Relay twin of the napi `git_pull_rebase_from_base_via_executor`: resolve the
 * rebase source (read-only `git remote` → longest match → `check-ref-format`),
 * then run the mutating `pull --rebase <remote> <branch>` over the relay's async
 * JS git executor — one call, collapsing the old resolve-in-Rust / pull-in-TS
 * split. `git_pull_rebase_from_base_async` normalizes as `pull` internally (the
 * raw "Choose a remote base branch…" resolver message tails identically), so this
 * rejects with the already-normalized message (preserved as a JS `Error`).
 */
export function gitPullRebaseFromBaseViaExecutor(executor: Function, base_ref: string): Promise<void>;

/**
 * Relay twin of the napi `git_push_via_executor` — the one destructive IO-tier op:
 * validate an explicit target, resolve the refspec (explicit; else the branch's
 * configured push remote so a fork-tracking worktree doesn't send review commits
 * upstream; else first-publish `origin HEAD`), then run
 * `git push [--force-with-lease] --set-upstream …` over the relay's async JS git
 * executor. An explicit target needs BOTH remote+branch; otherwise the configured
 * path. `git_push` normalizes errors internally, so this rejects with the
 * already-normalized message (preserved as a JS `Error` for the caller's
 * non-fast-forward classifier). The JS-boundary shape guard stays in the caller.
 */
export function gitPushViaExecutor(executor: Function, remote_name: string | null | undefined, branch_name: string | null | undefined, remote_url: string | null | undefined, force_with_lease: boolean): Promise<void>;

/**
 * True only for clearly-no-upstream signals (an expected state, gated on a
 * `fatal:` prefix). `undefined` message -> false (a non-Error throw in TS).
 */
export function isNoUpstreamError(message?: string | null): boolean;

/**
 * Normalise a git remote-operation error into a user-facing message. `message`
 * is `undefined` for a non-Error throw (returns the fixed fallback). `operation`
 * is `"push" | "pull" | "fetch" | "upstream"` (or `undefined`); an unrecognised
 * value maps to `None`, matching the TS default-parameter behaviour.
 */
export function normalizeGitErrorMessage(message?: string | null, operation?: string | null): string;

/**
 * Aggregate pure-module dispatch — the relay/renderer twin of the napi
 * `orcaDispatch`, running the IDENTICAL registry so output is byte-identical.
 * `input_json` empty/invalid → JSON null (a no-arg call). Returns the module's
 * JSON result, or an `__dispatch_error__` object for an unregistered module.
 */
export function orcaDispatch(module: string, _function: string, input_json: string): string;

/**
 * Parse an agent's PR-fields JSON reply (TS `parseGeneratedPullRequestFields`) as
 * `{ok:true, fields:{base,title,body,draft}} | {ok:false, error}` JSON. Exported for
 * parity/surface symmetry (the renderer only calls build; parse runs in main via napi).
 */
export function parseGeneratedPullRequestFields(raw: string, fallback_json: string): string;

/**
 * NUL-delimited `git log` (in `GIT_HISTORY_COMMIT_FORMAT`) parsed to the
 * `GitHistoryItem[]` JSON the TS `parseGitHistoryLog` produced.
 */
export function parseGitHistoryLog(stdout: string): string;

/**
 * `git diff --numstat` (text or `-z`) parsed to `{path: {added?, removed?}}` JSON.
 */
export function parseNumstat(stdout: Uint8Array): string;

/**
 * One-shot status scan (the relay's `parseStatusOutput` replacement): the cap is
 * applied DURING the scan, so `entries` is bounded by `limit`. Returns the
 * status-parse-result JSON.
 */
export function parseStatusPorcelain(stdout: Uint8Array, limit: number): string;

/**
 * `git worktree list --porcelain` (or the `-z` NUL form) parsed to the
 * `GitWorktreeInfo[]` JSON the TS `parseWorktreeList` produced.
 */
export function parseWorktreeList(output: string, nul_delimited: boolean): string;

/**
 * Resolve the spawn binary + prefix args from an optional command override, as
 * `{ok:true, binary, prefixArgs} | {ok:false, error}` JSON.
 */
export function planAgentBinary(default_binary: string, command_override?: string | null): string;

/**
 * Plan a commit-message generation as `{ok:true, plan:{binary,args,stdinPayload,
 * label}} | {ok:false, error}` JSON (the TS `CommitMessagePlanResult` union).
 * Input is the `CommitMessagePlanInput` object as JSON + the prompt.
 */
export function planCommitMessageGeneration(plan_input_json: string, prompt: string): string;

/**
 * Slugify free text into a git-ref-safe workspace seed.
 */
export function slugifyForWorkspaceName(input: string): string;

/**
 * Scrub credentials embedded in a git URL within `message` (keeps SSH user-info;
 * strips `user:password@` on any scheme + HTTP(S) token-only `user@`).
 */
export function stripCredentialsFromMessage(message: string): string;

/**
 * Run one terminal quick-command helper by name over its JSON input, returning
 * JSON (TS `terminal-quick-commands.ts`). The renderer drives normalize + the
 * typed-object accessors through this — see `orca_agents::terminal_quick_command_json`.
 */
export function terminalQuickCommandOp(_function: string, input_json: string): string;

/**
 * Dispatch one TUI agent-startup plan builder by name over its camelCase JSON
 * (TS `tui-agent-startup.ts`). The renderer drives buildAgentStartupPlan /
 * …Resume… / …Draft… through this — see `orca_agents::tui_agent_startup_json`.
 */
export function tuiAgentStartupOp(_function: string, input_json: string): string;

/**
 * True when `git cherry <upstream> HEAD`-style mark output shows at least one
 * commit and every commit is patch-equivalent (`=`). The relay's
 * behind-commits-are-patch-equivalent probe.
 */
export function upstreamOnlyCommitsArePatchEquivalent(cherry_mark_output: string): boolean;

/**
 * Validate a persisted push target's *value* rules (path-traversal safety for a
 * remote name / branch name / optional GitHub URL). Returns the TS-identical
 * error message, or `undefined` when valid. The `unknown`->typed guards (the
 * "Invalid PR push target …" messages) stay in JS.
 */
export function validateGitPushTargetRules(remote_name: string, branch_name: string, remote_url?: string | null): string | undefined;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_quickopenindex_free: (a: number, b: number) => void;
    readonly branchIsSafeToDeleteViaExecutor: (a: number, b: number, c: number) => number;
    readonly buildPullRequestFieldsPrompt: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly computeLineStats: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly countAdditionsInBuffer: (a: number, b: number) => number;
    readonly decodeGitCQuotedPath: (a: number, b: number, c: number) => void;
    readonly deriveGeneratedTabTitle: (a: number, b: number, c: number) => void;
    readonly detectPiAgentKindFromCommand: (a: number, b: number, c: number) => void;
    readonly formatSubmodulePushFailureDetail: (a: number, b: number, c: number) => void;
    readonly getLinearIssueWorkspaceName: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly getLinkedWorkItemSuggestedName: (a: number, b: number, c: number) => void;
    readonly getLinkedWorkItemWorkspaceName: (a: number, b: number, c: number) => void;
    readonly getUpstreamStatusViaExecutor: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
    readonly getWorkspaceIntentName: (a: number, b: number, c: number) => void;
    readonly gitFetchViaExecutor: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
    readonly gitPullRebaseFromBaseViaExecutor: (a: number, b: number, c: number) => number;
    readonly gitPushViaExecutor: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => number;
    readonly isNoUpstreamError: (a: number, b: number) => number;
    readonly normalizeGitErrorMessage: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly orcaDispatch: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly parseGeneratedPullRequestFields: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly parseGitHistoryLog: (a: number, b: number, c: number) => void;
    readonly parseNumstat: (a: number, b: number, c: number) => void;
    readonly parseStatusPorcelain: (a: number, b: number, c: number, d: number) => void;
    readonly parseWorktreeList: (a: number, b: number, c: number, d: number) => void;
    readonly planAgentBinary: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly planCommitMessageGeneration: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly quickopenindex_exactMatches: (a: number, b: number, c: number, d: number) => void;
    readonly quickopenindex_fileCount: (a: number) => number;
    readonly quickopenindex_new: (a: number, b: number) => number;
    readonly quickopenindex_rank: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly slugifyForWorkspaceName: (a: number, b: number, c: number) => void;
    readonly stripCredentialsFromMessage: (a: number, b: number, c: number) => void;
    readonly terminalQuickCommandOp: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly tuiAgentStartupOp: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly upstreamOnlyCommitsArePatchEquivalent: (a: number, b: number) => number;
    readonly validateGitPushTargetRules: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly __wasm_bindgen_func_elem_1641: (a: number, b: number) => void;
    readonly __wasm_bindgen_func_elem_1707: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1643: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
