// TS dispatch for the repo-clone-path parity module: maps the shared vector
// function names to the real `src/main/git/repo-clone-path.ts` exports so the
// harness compares the live TS reference against the Rust port.
//
// `deriveValidatedClonePath` reads `process.platform` and throws on invalid
// input; we run it on the (posix) harness host and wrap the result into a
// `{ clonePath }` / `{ error }` value to match the Rust `Result` image.

import {
  deriveValidatedClonePath,
  getClonePathComparisonKey
} from '../../../src/main/git/repo-clone-path'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'deriveValidatedClonePath': {
      const { url, destination } = input as { url: string; destination: string }
      try {
        return { clonePath: deriveValidatedClonePath({ url, destination }) }
      } catch (error) {
        return { error: (error as Error).message }
      }
    }
    case 'getClonePathComparisonKey': {
      const { clonePath } = input as { clonePath: string }
      return getClonePathComparisonKey(clonePath)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
