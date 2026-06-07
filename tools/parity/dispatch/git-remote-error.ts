// TS dispatch for the git-remote-error parity module: maps the shared vector
// function names to the real `src/shared/git-remote-error.ts` exports so the
// harness compares the live TS reference against the Rust port.

import { stripCredentialsFromMessage } from '../../../src/shared/git-remote-error'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'stripCredentialsFromMessage':
      return stripCredentialsFromMessage(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
