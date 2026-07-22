/* @ts-self-types="./orca_git_wasm.d.ts" */

/**
 * Prepared Quick Open index for the RENDERER: the worktree file list crosses
 * the wasm boundary ONCE (NUL-joined — file names cannot contain NUL), then
 * each keystroke sends only the query and gets the top-N `{path, score}`
 * JSON back. Preparation (slash-normalize, lowercase, UTF-16 encode) happens
 * at construction, so the per-keystroke cost is only the subsequence scans.
 */
export class QuickOpenIndex {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        QuickOpenIndexFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_quickopenindex_free(ptr, 0);
    }
    /**
     * Exact-path and exact-basename matches for an already-lowercased query
     * (the TS `findExistingFileMatches` passes), as
     * `{"paths":[…],"basenames":[…]}` JSON in input order.
     * @param {string} lower_query
     * @returns {string}
     */
    exactMatches(lower_query) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(lower_query, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.quickopenindex_exactMatches(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    fileCount() {
        const ret = wasm.quickopenindex_fileCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {string} nul_joined_paths
     */
    constructor(nul_joined_paths) {
        const ptr0 = passStringToWasm0(nul_joined_paths, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.quickopenindex_new(ptr0, len0);
        this.__wbg_ptr = ret >>> 0;
        QuickOpenIndexFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Rank against the prepared list; returns `[{path, score}, …]` JSON,
     * best (lowest score) first, ties by original input order.
     * @param {string} query
     * @param {number} limit
     * @returns {string}
     */
    rank(query, limit) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(query, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.quickopenindex_rank(retptr, this.__wbg_ptr, ptr0, len0, limit);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
        }
    }
}
if (Symbol.dispose) QuickOpenIndex.prototype[Symbol.dispose] = QuickOpenIndex.prototype.free;

/**
 * Relay twin of the napi `branch_is_safe_to_delete_via_executor`: gather candidate
 * base refs, refresh the relevant remotes (`fetch --prune`, the one mutation),
 * then decide whether the branch has any unmerged changes (tree-equal merge,
 * patch-equivalent commits, or a squash match — which pipes patch text to
 * `git patch-id --stable` via the executor's stdin), all over the relay's async
 * JS git executor. Resolves the boolean; the destructive `branch -d/-D` stays in
 * the relay's TS, gated on this. The decision only ever moves toward *preserve*,
 * so it can never over-delete (and never rejects — git errors degrade to safe).
 * @param {Function} executor
 * @param {string} branch_name
 * @returns {Promise<boolean>}
 */
export function branchIsSafeToDeleteViaExecutor(executor, branch_name) {
    const ptr0 = passStringToWasm0(branch_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.branchIsSafeToDeleteViaExecutor(addHeapObject(executor), ptr0, len0);
    return takeObject(ret);
}

/**
 * Build the PR-fields generation prompt (TS `buildPullRequestFieldsPrompt`); the
 * renderer's dry-run preview dialog runs this. `context_json` is the
 * `PullRequestDraftContext` object; returns the prompt string.
 * @param {string} context_json
 * @param {string} custom_prompt
 * @returns {string}
 */
export function buildPullRequestFieldsPrompt(context_json, custom_prompt) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(context_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(custom_prompt, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        wasm.buildPullRequestFieldsPrompt(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Approximate added/removed line counts for a diff section; returns the
 * line-stats JSON, or `undefined` for the large-input guard (>500k combined
 * chars — splitting that in a React render would block the UI). This one is
 * consumed by the RENDERER (not the relay): the renderer has no napi access,
 * so it loads this same wasm.
 * @param {string} original
 * @param {string} modified
 * @param {string} status
 * @returns {string | undefined}
 */
export function computeLineStats(original, modified, status) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(original, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(modified, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(status, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len2 = WASM_VECTOR_LEN;
        wasm.computeLineStats(retptr, ptr0, len0, ptr1, len1, ptr2, len2);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v4;
        if (r0 !== 0) {
            v4 = getStringFromWasm0(r0, r1).slice();
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v4;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * Count additions for an untracked file's contents: `undefined` for binary, 0 for
 * empty, else the trailing-newline-aware line count.
 * @param {Uint8Array} bytes
 * @returns {number | undefined}
 */
export function countAdditionsInBuffer(bytes) {
    const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_export);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.countAdditionsInBuffer(ptr0, len0);
    return ret === 0x100000001 ? undefined : ret;
}

/**
 * Decode a git C-quoted (octal-escaped) path. Raw (unquoted) input passes through.
 * @param {string} value
 * @returns {string}
 */
export function decodeGitCQuotedPath(value) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(value, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.decodeGitCQuotedPath(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Short generated tab title from a free-form agent prompt (first clause,
 * filler stripped, capped at a word boundary), or `undefined` when the prompt
 * has no usable title text. Consumed by the RENDERER terminal store.
 * @param {string} prompt
 * @returns {string | undefined}
 */
export function deriveGeneratedTabTitle(prompt) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(prompt, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.deriveGeneratedTabTitle(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v2;
        if (r0 !== 0) {
            v2 = getStringFromWasm0(r0, r1).slice();
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v2;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * Which Pi-compatible agent a launch command starts: `"omp"` for OMP
 * (`omp` / `omp.sh`), else `"pi"`. The relay uses this to target the managed
 * extension dir for the actual agent being launched.
 * @param {string | null} [command]
 * @returns {string}
 */
export function detectPiAgentKindFromCommand(command) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        var ptr0 = isLikeNone(command) ? 0 : passStringToWasm0(command, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len0 = WASM_VECTOR_LEN;
        wasm.detectPiAgentKindFromCommand(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * The actionable nested-submodule rejection hidden behind a recursive-push
 * failure, or `undefined`. Consumed by the RENDERER (push-failure toasts) via
 * this same wasm.
 * @param {string} message
 * @returns {string | undefined}
 */
export function formatSubmodulePushFailureDetail(message) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.formatSubmodulePushFailureDetail(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v2;
        if (r0 !== 0) {
            v2 = getStringFromWasm0(r0, r1).slice();
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v2;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * Combined Linear identifier+title workspace seed (dedup-aware).
 * @param {string} identifier
 * @param {string} title
 * @returns {string}
 */
export function getLinearIssueWorkspaceName(identifier, title) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(identifier, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(title, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        wasm.getLinearIssueWorkspaceName(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Title → slug suggestion for a linked work item (TS takes `{ title }`; the
 * wrapper passes `.title`).
 * @param {string} title
 * @returns {string}
 */
export function getLinkedWorkItemSuggestedName(title) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(title, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.getLinkedWorkItemSuggestedName(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Display+seed for a linked work item as `{displayName, seedName}` JSON, or
 * `undefined` when no git-safe seed derives. Input is the work item as JSON.
 * @param {string} item_json
 * @returns {string | undefined}
 */
export function getLinkedWorkItemWorkspaceName(item_json) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(item_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.getLinkedWorkItemWorkspaceName(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v2;
        if (r0 !== 0) {
            v2 = getStringFromWasm0(r0, r1).slice();
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v2;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * Relay twin of the napi `get_upstream_status_via_executor` (EXPLICIT publish
 * target): drive orca-git's upstream/ahead-behind status — `check-ref-format` →
 * `rev-parse` verify → (conditional) `rev-list` → (conditional) cherry-mark
 * `log` — over the relay's async JS git executor, with the data-dependent
 * decisions and the no-upstream swallow + error normalization owned by Rust.
 * Resolves the `GitUpstreamStatus` JSON (exact TS shape); rejects with the
 * already-normalized message (preserved as a JS `Error`). The JS-boundary
 * "Invalid PR push target …" shape guard stays in the relay's TS caller.
 * @param {Function} executor
 * @param {string} remote_name
 * @param {string} branch_name
 * @param {string | null} [remote_url]
 * @returns {Promise<string>}
 */
export function getUpstreamStatusViaExecutor(executor, remote_name, branch_name, remote_url) {
    const ptr0 = passStringToWasm0(remote_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(branch_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len1 = WASM_VECTOR_LEN;
    var ptr2 = isLikeNone(remote_url) ? 0 : passStringToWasm0(remote_url, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len2 = WASM_VECTOR_LEN;
    const ret = wasm.getUpstreamStatusViaExecutor(addHeapObject(executor), ptr0, len0, ptr1, len1, ptr2, len2);
    return takeObject(ret);
}

/**
 * First-create intent display+seed as `{displayName, seedName}` JSON, or
 * `undefined`. Input is `{sourceText?, workItem?, fallbackName?}` JSON.
 * @param {string} args_json
 * @returns {string | undefined}
 */
export function getWorkspaceIntentName(args_json) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(args_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.getWorkspaceIntentName(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v2;
        if (r0 !== 0) {
            v2 = getStringFromWasm0(r0, r1).slice();
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v2;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

/**
 * Relay twin of the napi `git_fetch_via_executor`: validate an explicit target
 * (`check-ref-format`) then `fetch --prune [<remote>]` over the relay's async JS
 * git executor. An explicit target needs BOTH remote+branch; otherwise a plain
 * prune-fetch. `git_fetch` normalizes errors internally, so this rejects with
 * the already-normalized message (preserved as a JS `Error`). The JS-boundary
 * shape guard stays in the caller.
 * @param {Function} executor
 * @param {string | null} [remote_name]
 * @param {string | null} [branch_name]
 * @param {string | null} [remote_url]
 * @returns {Promise<void>}
 */
export function gitFetchViaExecutor(executor, remote_name, branch_name, remote_url) {
    var ptr0 = isLikeNone(remote_name) ? 0 : passStringToWasm0(remote_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len0 = WASM_VECTOR_LEN;
    var ptr1 = isLikeNone(branch_name) ? 0 : passStringToWasm0(branch_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len1 = WASM_VECTOR_LEN;
    var ptr2 = isLikeNone(remote_url) ? 0 : passStringToWasm0(remote_url, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len2 = WASM_VECTOR_LEN;
    const ret = wasm.gitFetchViaExecutor(addHeapObject(executor), ptr0, len0, ptr1, len1, ptr2, len2);
    return takeObject(ret);
}

/**
 * Relay twin of the napi `git_pull_rebase_from_base_via_executor`: resolve the
 * rebase source (read-only `git remote` → longest match → `check-ref-format`),
 * then run the mutating `pull --rebase <remote> <branch>` over the relay's async
 * JS git executor — one call, collapsing the old resolve-in-Rust / pull-in-TS
 * split. `git_pull_rebase_from_base_async` normalizes as `pull` internally (the
 * raw "Choose a remote base branch…" resolver message tails identically), so this
 * rejects with the already-normalized message (preserved as a JS `Error`).
 * @param {Function} executor
 * @param {string} base_ref
 * @returns {Promise<void>}
 */
export function gitPullRebaseFromBaseViaExecutor(executor, base_ref) {
    const ptr0 = passStringToWasm0(base_ref, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.gitPullRebaseFromBaseViaExecutor(addHeapObject(executor), ptr0, len0);
    return takeObject(ret);
}

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
 * @param {Function} executor
 * @param {string | null | undefined} remote_name
 * @param {string | null | undefined} branch_name
 * @param {string | null | undefined} remote_url
 * @param {boolean} force_with_lease
 * @returns {Promise<void>}
 */
export function gitPushViaExecutor(executor, remote_name, branch_name, remote_url, force_with_lease) {
    var ptr0 = isLikeNone(remote_name) ? 0 : passStringToWasm0(remote_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len0 = WASM_VECTOR_LEN;
    var ptr1 = isLikeNone(branch_name) ? 0 : passStringToWasm0(branch_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len1 = WASM_VECTOR_LEN;
    var ptr2 = isLikeNone(remote_url) ? 0 : passStringToWasm0(remote_url, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len2 = WASM_VECTOR_LEN;
    const ret = wasm.gitPushViaExecutor(addHeapObject(executor), ptr0, len0, ptr1, len1, ptr2, len2, force_with_lease);
    return takeObject(ret);
}

/**
 * True only for clearly-no-upstream signals (an expected state, gated on a
 * `fatal:` prefix). `undefined` message -> false (a non-Error throw in TS).
 * @param {string | null} [message]
 * @returns {boolean}
 */
export function isNoUpstreamError(message) {
    var ptr0 = isLikeNone(message) ? 0 : passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    var len0 = WASM_VECTOR_LEN;
    const ret = wasm.isNoUpstreamError(ptr0, len0);
    return ret !== 0;
}

/**
 * Normalise a git remote-operation error into a user-facing message. `message`
 * is `undefined` for a non-Error throw (returns the fixed fallback). `operation`
 * is `"push" | "pull" | "fetch" | "upstream"` (or `undefined`); an unrecognised
 * value maps to `None`, matching the TS default-parameter behaviour.
 * @param {string | null} [message]
 * @param {string | null} [operation]
 * @returns {string}
 */
export function normalizeGitErrorMessage(message, operation) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        var ptr0 = isLikeNone(message) ? 0 : passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len0 = WASM_VECTOR_LEN;
        var ptr1 = isLikeNone(operation) ? 0 : passStringToWasm0(operation, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len1 = WASM_VECTOR_LEN;
        wasm.normalizeGitErrorMessage(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Aggregate pure-module dispatch — the relay/renderer twin of the napi
 * `orcaDispatch`, running the IDENTICAL registry so output is byte-identical.
 * `input_json` empty/invalid → JSON null (a no-arg call). Returns the module's
 * JSON result, or an `__dispatch_error__` object for an unregistered module.
 * @param {string} module
 * @param {string} _function
 * @param {string} input_json
 * @returns {string}
 */
export function orcaDispatch(module, _function, input_json) {
    let deferred4_0;
    let deferred4_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(module, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(_function, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(input_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len2 = WASM_VECTOR_LEN;
        wasm.orcaDispatch(retptr, ptr0, len0, ptr1, len1, ptr2, len2);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred4_0 = r0;
        deferred4_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred4_0, deferred4_1, 1);
    }
}

/**
 * Parse an agent's PR-fields JSON reply (TS `parseGeneratedPullRequestFields`) as
 * `{ok:true, fields:{base,title,body,draft}} | {ok:false, error}` JSON. Exported for
 * parity/surface symmetry (the renderer only calls build; parse runs in main via napi).
 * @param {string} raw
 * @param {string} fallback_json
 * @returns {string}
 */
export function parseGeneratedPullRequestFields(raw, fallback_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(raw, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(fallback_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        wasm.parseGeneratedPullRequestFields(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * NUL-delimited `git log` (in `GIT_HISTORY_COMMIT_FORMAT`) parsed to the
 * `GitHistoryItem[]` JSON the TS `parseGitHistoryLog` produced.
 * @param {string} stdout
 * @returns {string}
 */
export function parseGitHistoryLog(stdout) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(stdout, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.parseGitHistoryLog(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * `git diff --numstat` (text or `-z`) parsed to `{path: {added?, removed?}}` JSON.
 * @param {Uint8Array} stdout
 * @returns {string}
 */
export function parseNumstat(stdout) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passArray8ToWasm0(stdout, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.parseNumstat(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * One-shot status scan (the relay's `parseStatusOutput` replacement): the cap is
 * applied DURING the scan, so `entries` is bounded by `limit`. Returns the
 * status-parse-result JSON.
 * @param {Uint8Array} stdout
 * @param {number} limit
 * @returns {string}
 */
export function parseStatusPorcelain(stdout, limit) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passArray8ToWasm0(stdout, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        wasm.parseStatusPorcelain(retptr, ptr0, len0, limit);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * `git worktree list --porcelain` (or the `-z` NUL form) parsed to the
 * `GitWorktreeInfo[]` JSON the TS `parseWorktreeList` produced.
 * @param {string} output
 * @param {boolean} nul_delimited
 * @returns {string}
 */
export function parseWorktreeList(output, nul_delimited) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(output, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.parseWorktreeList(retptr, ptr0, len0, nul_delimited);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Resolve the spawn binary + prefix args from an optional command override, as
 * `{ok:true, binary, prefixArgs} | {ok:false, error}` JSON.
 * @param {string} default_binary
 * @param {string | null} [command_override]
 * @returns {string}
 */
export function planAgentBinary(default_binary, command_override) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(default_binary, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        var ptr1 = isLikeNone(command_override) ? 0 : passStringToWasm0(command_override, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len1 = WASM_VECTOR_LEN;
        wasm.planAgentBinary(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Plan a commit-message generation as `{ok:true, plan:{binary,args,stdinPayload,
 * label}} | {ok:false, error}` JSON (the TS `CommitMessagePlanResult` union).
 * Input is the `CommitMessagePlanInput` object as JSON + the prompt.
 * @param {string} plan_input_json
 * @param {string} prompt
 * @returns {string}
 */
export function planCommitMessageGeneration(plan_input_json, prompt) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(plan_input_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(prompt, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        wasm.planCommitMessageGeneration(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Slugify free text into a git-ref-safe workspace seed.
 * @param {string} input
 * @returns {string}
 */
export function slugifyForWorkspaceName(input) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(input, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.slugifyForWorkspaceName(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Scrub credentials embedded in a git URL within `message` (keeps SSH user-info;
 * strips `user:password@` on any scheme + HTTP(S) token-only `user@`).
 * @param {string} message
 * @returns {string}
 */
export function stripCredentialsFromMessage(message) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(message, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.stripCredentialsFromMessage(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Run one terminal quick-command helper by name over its JSON input, returning
 * JSON (TS `terminal-quick-commands.ts`). The renderer drives normalize + the
 * typed-object accessors through this — see `orca_agents::terminal_quick_command_json`.
 * @param {string} _function
 * @param {string} input_json
 * @returns {string}
 */
export function terminalQuickCommandOp(_function, input_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(_function, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(input_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        wasm.terminalQuickCommandOp(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Dispatch one TUI agent-startup plan builder by name over its camelCase JSON
 * (TS `tui-agent-startup.ts`). The renderer drives buildAgentStartupPlan /
 * …Resume… / …Draft… through this — see `orca_agents::tui_agent_startup_json`.
 * @param {string} _function
 * @param {string} input_json
 * @returns {string}
 */
export function tuiAgentStartupOp(_function, input_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(_function, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(input_json, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        wasm.tuiAgentStartupOp(retptr, ptr0, len0, ptr1, len1);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred3_0 = r0;
        deferred3_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred3_0, deferred3_1, 1);
    }
}

/**
 * True when `git cherry <upstream> HEAD`-style mark output shows at least one
 * commit and every commit is patch-equivalent (`=`). The relay's
 * behind-commits-are-patch-equivalent probe.
 * @param {string} cherry_mark_output
 * @returns {boolean}
 */
export function upstreamOnlyCommitsArePatchEquivalent(cherry_mark_output) {
    const ptr0 = passStringToWasm0(cherry_mark_output, wasm.__wbindgen_export, wasm.__wbindgen_export2);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.upstreamOnlyCommitsArePatchEquivalent(ptr0, len0);
    return ret !== 0;
}

/**
 * Validate a persisted push target's *value* rules (path-traversal safety for a
 * remote name / branch name / optional GitHub URL). Returns the TS-identical
 * error message, or `undefined` when valid. The `unknown`->typed guards (the
 * "Invalid PR push target …" messages) stay in JS.
 * @param {string} remote_name
 * @param {string} branch_name
 * @param {string | null} [remote_url]
 * @returns {string | undefined}
 */
export function validateGitPushTargetRules(remote_name, branch_name, remote_url) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(remote_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(branch_name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        var ptr2 = isLikeNone(remote_url) ? 0 : passStringToWasm0(remote_url, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        var len2 = WASM_VECTOR_LEN;
        wasm.validateGitPushTargetRules(retptr, ptr0, len0, ptr1, len1, ptr2, len2);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v4;
        if (r0 !== 0) {
            v4 = getStringFromWasm0(r0, r1).slice();
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v4;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_debug_string_0bc8482c6e3508ae: function(arg0, arg1) {
            const ret = debugString(getObject(arg1));
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_function_0095a73b8b156f76: function(arg0) {
            const ret = typeof(getObject(arg0)) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_undefined_9e4d92534c42d778: function(arg0) {
            const ret = getObject(arg0) === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_8ff4255516ccad3e: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_72fb696202c56729: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_be289d5034ed271b: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_d9b87ff7982e3b21: function(arg0) {
            getObject(arg0)._wbg_cb_unref();
        },
        __wbg_call_389efe28435a9388: function() { return handleError(function (arg0, arg1) {
            const ret = getObject(arg0).call(getObject(arg1));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_call_4708e0c13bdc8e95: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = getObject(arg0).call(getObject(arg1), getObject(arg2));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_call_812d25f1510c13c8: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = getObject(arg0).call(getObject(arg1), getObject(arg2), getObject(arg3));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_get_b3ed3ad4be2bc8ac: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(getObject(arg0), getObject(arg1));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_instanceof_Error_8573fe0b0b480f46: function(arg0) {
            let result;
            try {
                result = getObject(arg0) instanceof Error;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_message_9ddc4b9a62a7c379: function(arg0) {
            const ret = getObject(arg0).message;
            return addHeapObject(ret);
        },
        __wbg_new_3eb36ae241fe6f44: function() {
            const ret = new Array();
            return addHeapObject(ret);
        },
        __wbg_new_72b49615380db768: function(arg0, arg1) {
            const ret = new Error(getStringFromWasm0(arg0, arg1));
            return addHeapObject(ret);
        },
        __wbg_new_b5d9e2fb389fef91: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return __wasm_bindgen_func_elem_1707(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return addHeapObject(ret);
            } finally {
                state0.a = state0.b = 0;
            }
        },
        __wbg_new_no_args_1c7c842f08d00ebb: function(arg0, arg1) {
            const ret = new Function(getStringFromWasm0(arg0, arg1));
            return addHeapObject(ret);
        },
        __wbg_push_8ffdcb2063340ba5: function(arg0, arg1) {
            const ret = getObject(arg0).push(getObject(arg1));
            return ret;
        },
        __wbg_queueMicrotask_0aa0a927f78f5d98: function(arg0) {
            const ret = getObject(arg0).queueMicrotask;
            return addHeapObject(ret);
        },
        __wbg_queueMicrotask_5bb536982f78a56f: function(arg0) {
            queueMicrotask(getObject(arg0));
        },
        __wbg_resolve_002c4b7d9d8f6b64: function(arg0) {
            const ret = Promise.resolve(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg_static_accessor_GLOBAL_12837167ad935116: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_e628e89ab3b1c95f: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_static_accessor_SELF_a621d3dfbb60d0ce: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_static_accessor_WINDOW_f8727f0cf888e0bd: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_then_0d9fe2c7b1857d32: function(arg0, arg1, arg2) {
            const ret = getObject(arg0).then(getObject(arg1), getObject(arg2));
            return addHeapObject(ret);
        },
        __wbg_then_b9e7b3b5f1a9e1b5: function(arg0, arg1) {
            const ret = getObject(arg0).then(getObject(arg1));
            return addHeapObject(ret);
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 72, function: Function { arguments: [Externref], shim_idx: 73, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm.__wasm_bindgen_func_elem_1641, __wasm_bindgen_func_elem_1643);
            return addHeapObject(ret);
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return addHeapObject(ret);
        },
        __wbindgen_object_clone_ref: function(arg0) {
            const ret = getObject(arg0);
            return addHeapObject(ret);
        },
        __wbindgen_object_drop_ref: function(arg0) {
            takeObject(arg0);
        },
    };
    return {
        __proto__: null,
        "./orca_git_wasm_bg.js": import0,
    };
}

function __wasm_bindgen_func_elem_1643(arg0, arg1, arg2) {
    wasm.__wasm_bindgen_func_elem_1643(arg0, arg1, addHeapObject(arg2));
}

function __wasm_bindgen_func_elem_1707(arg0, arg1, arg2, arg3) {
    wasm.__wasm_bindgen_func_elem_1707(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

const QuickOpenIndexFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_quickopenindex_free(ptr >>> 0, 1));

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => state.dtor(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function dropObject(idx) {
    if (idx < 132) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getObject(idx) { return heap[idx]; }

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        wasm.__wbindgen_export3(addHeapObject(e));
    }
}

let heap = new Array(128).fill(undefined);
heap.push(undefined, null, true, false);

let heap_next = heap.length;

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, dtor, f) {
    const state = { a: arg0, b: arg1, cnt: 1, dtor };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            state.dtor(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasm;
function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('orca_git_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
