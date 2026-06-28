import { createRequire } from 'module'
import { join } from 'path'
import { existsSync } from 'fs'

// Typed surface of the orca-git side of the napi addon built from
// native/orca-node (the verified `orca_git` status/numstat/line-count parsers).
// It is the SAME orca_node.node the terminal binding loads — Node-API is
// ABI-stable, so one .node serves both bindings in plain Node and Electron.
// This only EXPOSES the parsers (for parity proofs); the live git-status paths
// are unchanged.

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

export type RustGitBinding = {
  GitStatusParser: RustGitStatusParserCtor
  /** One-shot status scan; the cap is applied during the scan. Returns JSON. */
  parseStatusPorcelain(stdout: Buffer, limit: number): string
  /** `git diff --numstat` (text or `-z`) → `{path: {added?, removed?}}` JSON. */
  parseNumstat(stdout: Buffer): string
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
