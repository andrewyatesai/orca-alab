// TS dispatch for the git-remote-error parity module: maps the shared vector
// function names to the real `src/shared/git-remote-error.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  normalizeGitErrorMessage,
  stripCredentialsFromMessage,
  type GitRemoteOperation
} from '../../../src/shared/git-remote-error'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'stripCredentialsFromMessage':
      return stripCredentialsFromMessage(input as string)
    case 'normalizeGitErrorMessage': {
      const { message, operation } = input as {
        message: string | null
        operation?: GitRemoteOperation
      }
      // A null message models a non-Error throw (Rust `None`); a string models
      // the Error path — mirroring the Rust dispatch's `Option<&str>` mapping.
      const error = message == null ? undefined : new Error(message)
      return normalizeGitErrorMessage(error, operation)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
