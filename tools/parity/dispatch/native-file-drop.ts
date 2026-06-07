// TS dispatch for the native-file-drop parity module: maps the shared vector
// function names to the real `src/shared/native-file-drop.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  hasNativeFileDragTypes,
  resolveNativeFileDropPath,
  type NativeFileDropPathEntry
} from '../../../src/shared/native-file-drop'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'hasNativeFileDragTypes':
      return hasNativeFileDragTypes(input as readonly string[] | null)
    case 'resolveNativeFileDropPath':
      return resolveNativeFileDropPath(input as readonly NativeFileDropPathEntry[])
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
