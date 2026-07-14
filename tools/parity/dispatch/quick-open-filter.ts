// TS dispatch for the quick-open-filter parity module. buildGitLsFilesArgsForQuickOpen
// was cut over to the Rust core (main via napi, relay via wasm through the
// dispatch seam), so this adapter drives the SAME wasm for it — the diff
// degenerates to wasm-vs-binary and the goldens pin correctness. Everything else
// stays live TS (still the production impl), compared against Rust for real
// parity — including buildExcludePathPrefixes, which leans on node:path's OS-aware
// relative() semantics the zero-dep Rust port can't reproduce.
import {
  buildExcludePathPrefixes,
  buildHiddenDirExcludeGlobs,
  buildRgArgsForQuickOpen,
  normalizeQuickOpenRgLine,
  shouldExcludeQuickOpenRelPath,
  shouldIncludeQuickOpenPath,
  type RgArgsOptions,
  type RgOutputMode
} from '../../../src/shared/quick-open-filter'
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildGitLsFilesArgsForQuickOpen':
      return JSON.parse(
        gitWasmOracle().orcaDispatch('quick-open-filter', fn, JSON.stringify(input ?? null))
      )
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
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
