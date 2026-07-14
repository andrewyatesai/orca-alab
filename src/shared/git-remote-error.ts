// The git remote-error TEXT logic (normalizeGitErrorMessage,
// stripCredentialsFromMessage, isNoUpstreamError,
// formatSubmodulePushFailureDetail) moved to the Rust orca-text core: the main
// process drives it via napi (src/main/git/rust-git-remote-error.ts), the
// relay via wasm (src/relay/git-wasm.ts), and the renderer via wasm
// (src/renderer/src/lib/git-wasm/git-remote-error.ts). This shared module
// keeps the operation type plus the divergent-pull retry fallback — the latter
// is JS control flow (it wraps a runPull callback) with no Rust analog, so it
// stays in TS even though its error-text predicate mirrors the Rust core.

const DIVERGENT_PULL_RECONCILIATION_PATTERN =
  /Need to specify how to reconcile divergent branches|divergent branches and need to specify how to reconcile them/i
// Why: any of these already pin a reconciliation strategy, so the merge
// fallback must not override an explicit caller/user choice (e.g. --ff-only).
const RECONCILIATION_PULL_ARG_PATTERN =
  /^(--rebase|--no-rebase|--ff-only|--ff|--no-ff|--merge|-r)(=|$)/
// Why: merge is Git's historical pre-2.27 default. Falling back to it lets
// divergent pulls reconcile on fresh hosts that never configured pull.rebase
// or pull.ff, instead of failing outright. `--no-rebase` predates the 2.25
// baseline, so it is safe across all supported Git binaries.
export const MERGE_RECONCILIATION_PULL_ARGS = ['--no-rebase']

// Credential-URL scrub patterns. The CANONICAL normalizer lives in the Rust
// orca-text core (see the header note); this small pure-TS copy exists only so
// shared modules that can't reach an env-specific Rust binding (e.g.
// git-clone-failure-message, which runs in main, relay, and renderer) can still
// redact `https://user:token@host` before surfacing a clone error. Keep the two
// patterns in sync with the Rust core.
const USERPASS_URL_PATTERN = /([a-z][a-z0-9+.-]*:\/\/)[^\s/@:]+:[^\s/@]+@/gi
const HTTPS_TOKEN_URL_PATTERN = /(https?:\/\/)[^\s/@:]+@/gi

export function stripCredentialsFromMessage(message: string): string {
  return message.replace(USERPASS_URL_PATTERN, '$1').replace(HTTPS_TOKEN_URL_PATTERN, '$1')
}

// Why: a fresh host may lack any pull.rebase/pull.ff policy, so Git 2.27+
// refuses to reconcile divergent branches. Detect that specific failure so the
// caller can retry with an explicit merge instead of surfacing a config error.
// Match the raw message directly (no credential scrub): the divergent-branch
// phrase never carries a URL, and this only returns a boolean — the message is
// never surfaced — so scrubbing (now Rust-only) cannot change the result.
export function isDivergentPullReconciliationError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false
  }
  return DIVERGENT_PULL_RECONCILIATION_PATTERN.test(error.message)
}

// Whether the pull already specifies how to reconcile (rebase/merge/ff-only),
// in which case the caller's choice must win over the merge fallback.
export function pullArgsSpecifyReconciliation(pullArgs: string[]): boolean {
  return pullArgs.some((arg) => RECONCILIATION_PULL_ARG_PATTERN.test(arg))
}

// Why: on hosts with no pull.rebase/pull.ff policy, Git 2.27+ refuses to
// reconcile divergent branches. Retry as a merge (Git's historical default) so
// pulls succeed out of the box; callers that already forced a strategy — or
// users who configured rebase — never reach this fallback.
// Not routed through GitCapabilityCache: this is per-repo config/branch state,
// not a stable host capability, and only fires on an actual divergence error,
// so there is nothing host-scoped to cache.
export async function runPullWithDivergenceFallback(
  pullArgs: string[],
  runPull: (effectiveArgs: string[]) => Promise<void>
): Promise<void> {
  try {
    await runPull(pullArgs)
  } catch (error) {
    if (!pullArgsSpecifyReconciliation(pullArgs) && isDivergentPullReconciliationError(error)) {
      await runPull([...MERGE_RECONCILIATION_PULL_ARGS, ...pullArgs])
      return
    }
    throw error
  }
}

export type GitRemoteOperation = 'push' | 'pull' | 'fetch' | 'upstream'
