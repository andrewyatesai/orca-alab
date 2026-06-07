// TS dispatch for the cross-platform-path parity module: maps the shared vector
// function names to the real `src/shared/cross-platform-path.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  getRuntimePathBasename,
  isPathInsideOrEqual,
  isWindowsAbsolutePathLike,
  normalizeRuntimePathForComparison,
  normalizeRuntimePathSeparators,
  relativePathInsideRoot,
  resolveRuntimePath
} from '../../../src/shared/cross-platform-path'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isWindowsAbsolutePathLike': {
      const { value } = input as { value: string }
      return isWindowsAbsolutePathLike(value)
    }
    case 'normalizeRuntimePathSeparators': {
      const { value } = input as { value: string }
      return normalizeRuntimePathSeparators(value)
    }
    case 'normalizeRuntimePathForComparison': {
      const { value } = input as { value: string }
      return normalizeRuntimePathForComparison(value)
    }
    case 'resolveRuntimePath': {
      const { basePath, targetPath } = input as { basePath: string; targetPath: string }
      return resolveRuntimePath(basePath, targetPath)
    }
    case 'getRuntimePathBasename': {
      const { value } = input as { value: string }
      return getRuntimePathBasename(value)
    }
    case 'isPathInsideOrEqual': {
      const { rootPath, candidatePath } = input as { rootPath: string; candidatePath: string }
      return isPathInsideOrEqual(rootPath, candidatePath)
    }
    case 'relativePathInsideRoot': {
      const { rootPath, candidatePath } = input as { rootPath: string; candidatePath: string }
      return relativePathInsideRoot(rootPath, candidatePath)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
