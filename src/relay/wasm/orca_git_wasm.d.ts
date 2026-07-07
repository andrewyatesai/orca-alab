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
 * Scrub credentials embedded in a git URL within `message` (keeps SSH user-info;
 * strips `user:password@` on any scheme + HTTP(S) token-only `user@`).
 */
export function stripCredentialsFromMessage(message: string): string;

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
    readonly computeLineStats: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly countAdditionsInBuffer: (a: number, b: number) => number;
    readonly decodeGitCQuotedPath: (a: number, b: number, c: number) => void;
    readonly deriveGeneratedTabTitle: (a: number, b: number, c: number) => void;
    readonly detectPiAgentKindFromCommand: (a: number, b: number, c: number) => void;
    readonly formatSubmodulePushFailureDetail: (a: number, b: number, c: number) => void;
    readonly isNoUpstreamError: (a: number, b: number) => number;
    readonly normalizeGitErrorMessage: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly parseGitHistoryLog: (a: number, b: number, c: number) => void;
    readonly parseNumstat: (a: number, b: number, c: number) => void;
    readonly parseStatusPorcelain: (a: number, b: number, c: number, d: number) => void;
    readonly parseWorktreeList: (a: number, b: number, c: number, d: number) => void;
    readonly quickopenindex_exactMatches: (a: number, b: number, c: number, d: number) => void;
    readonly quickopenindex_fileCount: (a: number) => number;
    readonly quickopenindex_new: (a: number, b: number) => number;
    readonly quickopenindex_rank: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly stripCredentialsFromMessage: (a: number, b: number, c: number) => void;
    readonly upstreamOnlyCommitsArePatchEquivalent: (a: number, b: number) => number;
    readonly validateGitPushTargetRules: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number) => void;
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
