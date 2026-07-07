// TS dispatch for the git-remote-error parity module. The shared TS reference
// was DELETED (the Rust orca-text core is the sole impl — napi in main, wasm
// in the relay/renderer), so this adapter drives the napi binding: the vectors'
// recorded goldens now pin the napi surface absolutely, and the harness's
// TS-vs-Rust diff degenerates to napi-vs-binary (drift between the two Rust
// entry points would still surface here). Requires the built addon, like the
// napi-parity suite.

import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'stripCredentialsFromMessage':
      return requireRustGitBinding().stripCredentialsFromMessage(input as string)
    case 'normalizeGitErrorMessage': {
      const { message, operation } = input as {
        message: string | null
        operation?: string
      }
      // A null message models a non-Error throw (Rust `None`); a string models
      // the Error path — mirroring the Rust dispatch's `Option<&str>` mapping.
      return requireRustGitBinding().normalizeGitErrorMessage(message ?? undefined, operation)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
