import type { GitStatusEntry } from '../../shared/git-status-types'
import { loadRustGitBinding, type RustGitStatusParserCtor } from '../daemon/rust-git-addon'
import { gitStreamStdout, type GitStreamOptions } from './runner'
import { StatusPorcelainParser } from './status-porcelain-parser'

/** Normalized streamed `git status` result — identical whether produced by the
 *  Trust-verified Rust parser (orca_node.node) or the TypeScript fallback. */
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

/** Stream + parse `git status --porcelain=v2 --branch`, stopping git the moment
 *  the entry count crosses `limit` (0 = no cap). Drives the verified Rust parser
 *  when the napi addon is present, else the TypeScript StatusPorcelainParser —
 *  the two are proven output-identical by orca-git-napi-parity.test.ts. */
export async function streamGitStatus(
  statusArgs: string[],
  options: StatusStreamOptions,
  limit: number
): Promise<StreamedGitStatus> {
  const binding = loadRustGitBinding()
  return binding
    ? streamViaRust(binding.GitStatusParser, statusArgs, options, limit)
    : streamViaTs(statusArgs, options, limit)
}

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

async function streamViaRust(
  Parser: RustGitStatusParserCtor,
  statusArgs: string[],
  options: StatusStreamOptions,
  limit: number
): Promise<StreamedGitStatus> {
  const parser = new Parser()
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
    // Not a git repo / git unavailable / addon error — empty, same as the TS path.
    return emptyStatus()
  }
}

async function streamViaTs(
  statusArgs: string[],
  options: StatusStreamOptions,
  limit: number
): Promise<StreamedGitStatus> {
  const parser = new StatusPorcelainParser()
  try {
    const { stoppedEarly } = await gitStreamStdout(statusArgs, {
      ...options,
      onStdout: (chunk) => parser.update(chunk, limit)
    })
    if (!stoppedEarly) {
      parser.finish()
    }
    // Why: the parser stops one entry past the limit (it checks after pushing),
    // so trim back to exactly `limit` for a stable "first N shown" contract.
    const entries = stoppedEarly ? parser.entries.slice(0, limit) : parser.entries
    const { head, branch, upstreamName, upstreamAheadBehind } = parser.branch
    return {
      entries,
      head,
      branch,
      upstreamName,
      upstreamAheadBehind,
      ignoredPaths: parser.ignoredPaths,
      unmergedLines: parser.unmergedLines,
      statusLength: parser.statusLength,
      didHitLimit: stoppedEarly,
      succeeded: true
    }
  } catch {
    return emptyStatus()
  }
}

function emptyStatus(): StreamedGitStatus {
  return {
    entries: [],
    ignoredPaths: [],
    unmergedLines: [],
    statusLength: 0,
    didHitLimit: false,
    succeeded: false
  }
}
