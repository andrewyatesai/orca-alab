// TS dispatch for the workspace-session-schema parity module. The shared TS zod
// twin was DELETED (the Rust orca-config core is the sole impl — napi in main,
// the only process persistence.ts parses sessions in), so this adapter drives
// the napi binding: the vectors' recorded goldens now pin that surface
// absolutely, and the harness's TS-vs-Rust diff degenerates to napi-vs-binary.
// Requires the built addon, like the napi-parity suite.

import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseWorkspaceSession':
      // The vector input is the raw session JSON value; the return is the
      // discriminated union ({ok:true,value} | {ok:false,error}) as plain JSON.
      return JSON.parse(requireRustGitBinding().parseWorkspaceSession(JSON.stringify(input)))
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
