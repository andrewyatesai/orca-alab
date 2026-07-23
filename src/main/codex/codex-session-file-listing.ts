import { readdirSync } from 'node:fs'
import { opendir } from 'node:fs/promises'
import { join } from 'node:path'

export type CodexSessionBridgeIncrementalOptions = {
  /** Directory entries to process before yielding back to the event loop. */
  batchSize?: number
  /** Delay after each processed batch; zero still yields on a timer turn. */
  yieldMs?: number
}

const INCREMENTAL_BRIDGE_BATCH_SIZE = 64
const INCREMENTAL_BRIDGE_YIELD_MS = 10

const isJsonlSessionFile = (fileName: string): boolean => fileName.endsWith('.jsonl')

// Why: current Codex compresses rollouts to `.jsonl.zst` but still resumes from
// either physical representation (#9696); bridge both or compressed sessions drop.
const isRolloutSessionFile = (fileName: string): boolean =>
  fileName.endsWith('.jsonl') || fileName.endsWith('.jsonl.zst')

/**
 * Recursively lists session JSONL files below a root directory.
 *
 * This synchronous variant preserves the historical bridge behavior for callers
 * that run outside the CLI launch path.
 */
export function listCodexSessionJsonlFiles(rootPath: string): string[] {
  return listCodexSessionFiles(rootPath, isJsonlSessionFile)
}

/** Synchronous variant that also lists compressed rollout representations. */
export function listCodexSessionRolloutFiles(rootPath: string): string[] {
  return listCodexSessionFiles(rootPath, isRolloutSessionFile)
}

function listCodexSessionFiles(
  rootPath: string,
  isSessionFile: (fileName: string) => boolean
): string[] {
  const files: string[] = []
  try {
    for (const entry of readdirSync(rootPath, { withFileTypes: true })) {
      const childPath = join(rootPath, entry.name)
      if (entry.isDirectory()) {
        appendSessionFilePaths(files, listCodexSessionFiles(childPath, isSessionFile))
        continue
      }
      if (entry.isFile() && isSessionFile(entry.name)) {
        files.push(childPath)
      }
    }
  } catch (error) {
    console.warn('[codex-session-bridge] Failed to list system Codex sessions:', error)
  }
  return files.sort()
}

/**
 * Appends session paths without spreading large arrays into a single call.
 */
function appendSessionFilePaths(target: string[], source: readonly string[]): void {
  // Why: existing Codex homes can accumulate enough nested sessions to exceed
  // V8's argument limit if child arrays are spread into push().
  for (const filePath of source) {
    target.push(filePath)
  }
}

/**
 * Yields session JSONL files incrementally while walking a directory tree.
 *
 * The generator yields control between batches so large history directories do
 * not monopolize startup work.
 */
export async function* listCodexSessionJsonlFilesIncrementally(
  rootPath: string,
  options: CodexSessionBridgeIncrementalOptions
): AsyncGenerator<string> {
  yield* listCodexSessionFilesIncrementally(rootPath, options, isJsonlSessionFile)
}

/** Yields both physical representations that current Codex can resume. */
export async function* listCodexSessionRolloutFilesIncrementally(
  rootPath: string,
  options: CodexSessionBridgeIncrementalOptions
): AsyncGenerator<string> {
  yield* listCodexSessionFilesIncrementally(rootPath, options, isRolloutSessionFile)
}

async function* listCodexSessionFilesIncrementally(
  rootPath: string,
  options: CodexSessionBridgeIncrementalOptions,
  isSessionFile: (fileName: string) => boolean
): AsyncGenerator<string> {
  const batchSize = Math.max(1, options.batchSize ?? INCREMENTAL_BRIDGE_BATCH_SIZE)
  const yieldMs = Math.max(0, options.yieldMs ?? INCREMENTAL_BRIDGE_YIELD_MS)
  const pendingDirectories = [rootPath]
  let entriesSinceYield = 0

  while (pendingDirectories.length > 0) {
    const currentDirectory = pendingDirectories.pop()
    if (!currentDirectory) {
      continue
    }
    try {
      const directory = await opendir(currentDirectory)
      for await (const entry of directory) {
        const childPath = join(currentDirectory, entry.name)
        if (entry.isDirectory()) {
          pendingDirectories.push(childPath)
        } else if (entry.isFile() && isSessionFile(entry.name)) {
          yield childPath
        }
        entriesSinceYield += 1
        if (entriesSinceYield >= batchSize) {
          entriesSinceYield = 0
          await delayIncrementalBridge(yieldMs)
        }
      }
    } catch (error) {
      console.warn('[codex-session-bridge] Failed to list system Codex sessions:', error)
    }
  }
}

/**
 * Defers incremental bridge work to a later timer turn.
 */
function delayIncrementalBridge(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms)
  })
}
