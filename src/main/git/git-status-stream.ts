import type { GitStatusEntry } from '../../shared/git-status-types'
import { requireRustGitBinding } from '../daemon/rust-git-addon'
import { gitStreamStdout, type GitStreamOptions } from './runner'

/** Normalized streamed `git status` result, produced by the verified Rust
 *  `orca-git` streaming parser (orca_node.node). */
export type StreamedGitStatus = {
  /** Changed-file entries, already capped to `limit` when the cap was hit. */
  entries: GitStatusEntry[]
  head?: string
  branch?: string
  upstreamName?: string
  upstreamAheadBehind?: { ahead: number; behind: number }
  ignoredPaths: string[]
  /** Raw `u ` records for the caller to resolve via per-file git lookups. */
  unmergedLines: string[]
  /** Total changed entries observed (incl. any past the cap). */
  statusLength: number
  didHitLimit: boolean
  succeeded: boolean
}

type StatusStreamOptions = Pick<GitStreamOptions, 'cwd' | 'env' | 'wslDistro' | 'signal'>

type RustStatusResult = {
  entries: GitStatusEntry[]
  ignoredPaths: string[]
  unmergedLines: string[]
  statusLength: number
  head?: string
  branch?: string
  upstreamName?: string
  ahead?: number
  behind?: number
}

/** Stream + parse `git status --porcelain=v2 --branch`, stopping git the moment
 *  the entry count crosses `limit` (0 = no cap). Drives the Rust `orca-git`
 *  streaming parser — the napi addon is a required main-process dependency, so
 *  there is no TypeScript fallback (the former StatusPorcelainParser was deleted). */
export async function streamGitStatus(
  statusArgs: string[],
  options: StatusStreamOptions,
  limit: number
): Promise<StreamedGitStatus> {
  const parser = new (requireRustGitBinding().GitStatusParser)()
  try {
    // Raw bytes: git runs with core.quotePath=false, so filename bytes may be
    // invalid UTF-8; the Rust parser carries bytes + decodes lossily itself.
    const { stoppedEarly } = await gitStreamStdout(statusArgs, {
      ...options,
      onStdoutBytes: (chunk) => parser.update(chunk, limit)
    })
    if (!stoppedEarly) {
      parser.finish()
    }
    const r = JSON.parse(parser.result(limit)) as RustStatusResult
    return {
      entries: r.entries,
      head: r.head,
      branch: r.branch,
      upstreamName: r.upstreamName,
      upstreamAheadBehind:
        r.ahead !== undefined || r.behind !== undefined
          ? { ahead: r.ahead ?? 0, behind: r.behind ?? 0 }
          : undefined,
      ignoredPaths: r.ignoredPaths,
      unmergedLines: r.unmergedLines,
      statusLength: r.statusLength,
      didHitLimit: stoppedEarly,
      succeeded: true
    }
  } catch {
    // Not a git repo / git unavailable — an empty, non-succeeded status.
    return {
      entries: [],
      ignoredPaths: [],
      unmergedLines: [],
      statusLength: 0,
      didHitLimit: false,
      succeeded: false
    }
  }
}
