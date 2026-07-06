import { createRequire } from 'node:module'
import { join } from 'node:path'
import { existsSync } from 'node:fs'

// Typed surface of the orca-git side of the napi addon built from
// native/orca-node (the verified `orca_git` status/numstat/line-count parsers).
// It is the SAME orca_node.node the terminal binding loads — Node-API is
// ABI-stable, so one .node serves both bindings in plain Node and Electron.
// These parsers ARE the live path: git-status-stream drives them in production
// whenever the addon loads, with the TypeScript parsers as the proven-identical
// fallback when it does not.

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
  /** Approximate added/removed line counts JSON, or null for the large guard. */
  computeLineStats(original: string, modified: string, status: string): string | null
  /** Decode a git C-quoted (octal-escaped) path. */
  decodeGitCQuotedPath(value: string): string
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
