// TS dispatch for the quick-open-filter parity module: maps the shared vector
// function names to the real `src/shared/quick-open-filter.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  buildExcludePathPrefixes,
  buildGitLsFilesArgsForQuickOpen,
  buildHiddenDirExcludeGlobs,
  buildRgArgsForQuickOpen,
  normalizeQuickOpenRgLine,
  shouldExcludeQuickOpenRelPath,
  shouldIncludeQuickOpenPath,
  type RgArgsOptions,
  type RgOutputMode
} from '../../../src/shared/quick-open-filter'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'shouldIncludeQuickOpenPath': {
      const { path } = input as { path: string }
      return shouldIncludeQuickOpenPath(path)
    }
    case 'buildExcludePathPrefixes': {
      const { rootPath, excludePaths } = input as { rootPath: string; excludePaths?: unknown }
      return buildExcludePathPrefixes(rootPath, excludePaths)
    }
    case 'shouldExcludeQuickOpenRelPath': {
      const { relPath, excludePathPrefixes } = input as {
        relPath: string
        excludePathPrefixes: readonly string[]
      }
      return shouldExcludeQuickOpenRelPath(relPath, excludePathPrefixes)
    }
    case 'buildHiddenDirExcludeGlobs':
      return buildHiddenDirExcludeGlobs()
    case 'buildRgArgsForQuickOpen':
      return buildRgArgsForQuickOpen(input as RgArgsOptions)
    case 'normalizeQuickOpenRgLine': {
      const { rawLine, outputMode } = input as { rawLine: string; outputMode: RgOutputMode }
      return normalizeQuickOpenRgLine(rawLine, outputMode)
    }
    case 'buildGitLsFilesArgsForQuickOpen': {
      // Default `[]` applies when the key is omitted (undefined).
      const { excludePathPrefixes } = input as { excludePathPrefixes?: readonly string[] }
      return buildGitLsFilesArgsForQuickOpen(excludePathPrefixes)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
