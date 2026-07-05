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

/** Load the orca-git addon, or return null if it is unavailable or fails to
 *  load. Never throws — callers fall back to the TypeScript git parsers. */
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
